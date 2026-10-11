//! A disconnected shared client must discard an already rendered candidate.
use super::*;
#[test]
fn ready_candidate_cannot_publish_after_connection_invalidation() {
    let mut app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), crate::Services::default());
    let photo = app.session.active().unwrap();
    let original = app.session.develop_of(photo).unwrap();
    let generation = app.session.library_generation();
    let checkpoint = Checkpoint {
        photo,
        generation,
        original: original.clone(),
        expected: original.clone(),
        revision: app.session.photo_revision(photo),
        content: lightcraft_engine::media::content_key(app.session.catalog.photo(photo).unwrap()),
        eligible: true,
    };
    let mut target = (*original).clone();
    target.light.exposure = 0.5;
    let target = Arc::new(target);
    let cancel = Arc::new(AtomicBool::new(false));
    let result = render_snapshot(app.session.candidate_job(photo, target.clone(), 640).unwrap(), &cancel).unwrap();
    let mut state = State::default();
    state.conversations.insert(photo, Conversation::default());
    state.jobs.insert(
        photo,
        Job {
            seq: 1,
            round: "known-round".into(),
            state: "awaiting_initial_execution".into(),
            prompt: "brighten".into(),
            user_message: 0,
            history: vec![],
            checkpoint,
            cancel,
            snapshot: None,
            plans: BTreeMap::new(),
            stopped: false,
            inflight_plan: Some("plan".into()),
        },
    );
    let error = state
        .prepared(
            &mut app,
            &egui::Context::default(),
            WorkerMessage::Prepared { photo, generation, seq: 1, plan: Some(json!({"plan_id":"plan"})), settings: target, result: Ok(result) },
        )
        .unwrap_err();
    assert!(error.contains("未发布迟到候选"));
    assert_eq!(*app.session.develop_of(photo).unwrap(), *original);
}
