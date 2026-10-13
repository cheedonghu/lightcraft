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
            identity: RoundIdentity {
                scope: PhotoScope { library_generation: generation, photo },
                local_round_id: LocalRoundId::new().unwrap(),
                submission_seq: 1,
                connection_generation: 0,
            },
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
            WorkerMessage::Prepared {
                identity: state.jobs.get(&photo).unwrap().identity.clone(),
                plan: Some(json!({"plan_id":"plan"})),
                settings: target,
                result: Ok(result),
            },
        )
        .unwrap_err();
    assert!(error.contains("未发布迟到候选"));
    assert_eq!(*app.session.develop_of(photo).unwrap(), *original);
}

#[test]
fn explanation_keeps_stable_local_identity_after_selection_and_backend_admission() {
    let mut app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), crate::Services::default());
    let photo = app.session.active().unwrap();
    let other = app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    let original = app.session.develop_of(photo).unwrap();
    let generation = app.session.library_generation();
    let mut state = State::default();
    state.conversations.insert(photo, Conversation::default());
    state.jobs.insert(
        photo,
        Job {
            identity: RoundIdentity {
                scope: PhotoScope { library_generation: generation, photo },
                local_round_id: LocalRoundId::new().unwrap(),
                submission_seq: 1,
                connection_generation: 0,
            },
            round: String::new(),
            state: "submitting".into(),
            prompt: "explain".into(),
            user_message: 0,
            history: vec![],
            checkpoint: Checkpoint {
                photo,
                generation,
                original: original.clone(),
                expected: original.clone(),
                revision: app.session.photo_revision(photo),
                content: lightcraft_engine::media::content_key(app.session.catalog.photo(photo).unwrap()),
                eligible: true,
            },
            cancel: Arc::new(AtomicBool::new(false)),
            snapshot: None,
            plans: BTreeMap::new(),
            stopped: false,
            inflight_plan: None,
        },
    );
    let before_admission = state.jobs.get(&photo).unwrap().identity.local_round_id.clone();
    app.session.execute("library.select", &json!({"ids":[other.0],"active":other.0})).unwrap();
    let ctx = egui::Context::default();
    state.server(&mut app, &ctx, "round.state", &json!({"round_id":"backend-round","payload":{"submission_seq":1,"photo_ref":{"photo_id":photo.0,"library_generation":generation},"state":"queued"}})).unwrap();
    state
        .server(
            &mut app,
            &ctx,
            "text.delta",
            &json!({"round_id":"backend-round","payload":{"message_id":"answer","delta":"Explanation for original photo"}}),
        )
        .unwrap();
    state.server(&mut app, &ctx, "message.completed", &json!({"round_id":"backend-round","payload":{"message_id":"answer"}})).unwrap();
    state.server(&mut app, &ctx, "round.ended", &json!({"round_id":"backend-round","payload":{"terminal_state":"completed"}})).unwrap();
    let archive = app.session.photo_companion(photo).unwrap().unwrap();
    assert_eq!(archive["messages"][0]["text"], "Explanation for original photo");
    let local = archive["messages"][0]["local_round_id"].as_str().expect("new replies must retain a local round identity").to_owned();
    assert_eq!(local, before_admission.as_str());
    assert_ne!(local, "backend-round");
    assert!(!local.is_empty());
    assert!(app.session.photo_companion(other).unwrap().is_none());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *original);
    state.conversations.clear();
    state.load(&app, photo).unwrap();
    state.save(&mut app, photo).unwrap();
    assert_eq!(app.session.photo_companion(photo).unwrap().unwrap()["messages"][0]["local_round_id"], local);
}
