//! Exercise the existing bridge with real candidate rendering and photo commits.
use super::*;

fn started() -> (LightcraftApp, State, PhotoId, mpsc::Receiver<network::Request>, mpsc::SyncSender<network::Event>) {
    let mut app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), crate::Services::default());
    let photo = app.session.active().unwrap();
    let (connection, requests, events) = network::test_connection();
    let mut state = State { consent: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
    let response = state.start(&mut app).unwrap();
    let request = requests.try_recv().unwrap();
    assert_eq!(request.path, "/v1/rounds");
    assert_eq!(response["localRoundId"], request.identity.local_round_id.as_str());
    assert!(request.body.get("preview").is_none());
    assert!(request.body.get("local_round_id").is_none(), "v1 wire format is unchanged");
    assert!(state.jobs.get(&photo).unwrap().round.is_empty(), "local identity exists before admission");
    (app, state, photo, requests, events)
}

fn state_event(state: &mut State, app: &mut LightcraftApp, photo: PhotoId, phase: &str) {
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    state.server(app, &egui::Context::default(), "round.state", &json!({"round_id":"backend-round","payload":{"submission_seq":identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":identity.scope.library_generation},"state":phase}})).unwrap();
}

fn publish_worker(state: &mut State, app: &mut LightcraftApp) {
    let message = state.rx.recv_timeout(Duration::from_secs(30)).unwrap();
    state.prepared(app, &egui::Context::default(), message).unwrap();
}

#[test]
fn initial_and_correction_share_identity_and_stop_keeps_real_result() {
    let (mut app, mut state, photo, requests, _) = started();
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    let other = app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    let other_original = app.session.develop_of(other).unwrap();
    app.session.execute("library.select", &json!({"ids":[other.0],"active":other.0})).unwrap();
    app.ai = state;
    let status = app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["photo"], other.0);
    assert_eq!(status["activeRounds"][0]["photo"], photo.0);
    assert_eq!(status["activeRounds"][0]["localRoundId"], identity.local_round_id.as_str());
    state = std::mem::take(&mut app.ai);
    state_event(&mut state, &mut app, photo, "awaiting_input");
    publish_worker(&mut state, &mut app);
    let input = requests.try_recv().unwrap();
    assert!(input.path.ends_with("/input"));
    assert_eq!(input.identity, identity);
    for (plan_id, phase, exposure) in [("initial", "awaiting_initial_execution", 0.4), ("correction", "awaiting_correction_execution", 0.2)] {
        state_event(&mut state, &mut app, photo, phase);
        let data = json!({"round_id":"backend-round","payload":{"plan_id":plan_id,"capability_version":"lightcraft-v2","expected_revision":app.session.photo_revision(photo),"operations":[{"type":"set_controls","values":{"light.exposure":exposure}}]}});
        state.server(&mut app, &egui::Context::default(), "edit.plan", &data).unwrap();
        publish_worker(&mut state, &mut app);
        let feedback = requests.try_recv().unwrap();
        assert_eq!(feedback.identity, identity);
        assert_eq!(feedback.body["outcome"], "succeeded");
        assert_eq!(app.session.develop_of(photo).unwrap().light.exposure, exposure);
        let revision = app.session.photo_revision(photo);
        state.server(&mut app, &egui::Context::default(), "edit.plan", &data).unwrap();
        assert_eq!(requests.try_recv().unwrap().body, feedback.body);
        assert_eq!(app.session.photo_revision(photo), revision, "duplicate delivery cannot reapply");
    }
    state.stop(photo).unwrap();
    state.save(&mut app, photo).unwrap();
    assert!(requests.try_recv().unwrap().path.ends_with("/stop"));
    let archive = app.session.photo_companion(photo).unwrap().unwrap();
    assert_eq!(archive["schema_version"], 1);
    let rounds = archive["rounds"].as_array().unwrap();
    assert_eq!(rounds.len(), 2);
    for round in rounds {
        assert_eq!(round["local_round_id"], identity.local_round_id.as_str());
    }
    assert_eq!(app.session.develop_of(photo).unwrap().light.exposure, 0.2);
    assert_eq!(*app.session.develop_of(other).unwrap(), *other_original);
    assert!(state.history(&mut app, photo).unwrap().iter().all(|m| m.get("local_round_id").is_none()));
    state
        .prepared(
            &mut app,
            &egui::Context::default(),
            WorkerMessage::Prepared { identity, plan: None, settings: other_original, result: Err("late stopped worker".into()) },
        )
        .unwrap();
    assert_eq!(app.session.develop_of(photo).unwrap().light.exposure, 0.2);
}

#[test]
fn every_async_identity_member_rejects_old_work_and_old_http_failure() {
    let (mut app, mut state, photo, _, events) = started();
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    let original = app.session.develop_of(photo).unwrap();
    let mut replacements = Vec::new();
    let mut old = identity.clone();
    old.local_round_id = LocalRoundId::new().unwrap();
    replacements.push(old);
    let mut old = identity.clone();
    old.submission_seq += 1;
    replacements.push(old);
    let mut old = identity.clone();
    old.connection_generation += 1;
    replacements.push(old);
    let mut old = identity.clone();
    old.scope.library_generation += 1;
    replacements.push(old);
    let mut old = identity.clone();
    old.scope.photo = app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    replacements.push(old);
    for old in replacements {
        state
            .prepared(
                &mut app,
                &egui::Context::default(),
                WorkerMessage::Prepared { identity: old.clone(), plan: None, settings: original.clone(), result: Err("late worker error".into()) },
            )
            .unwrap();
        events
            .try_send(network::Event::Response {
                submission: identity.submission_seq,
                round: String::new(),
                path: "/v1/rounds".into(),
                identity: old,
                result: Err("old uncertain HTTP failure".into()),
            })
            .unwrap();
        state.poll_inner(&mut app, &egui::Context::default());
        assert!(state.is_busy(photo), "old error must not terminate the new task");
        assert!(state.connection.is_some(), "old error must not close the new connection");
        assert_eq!(*app.session.develop_of(photo).unwrap(), *original);
    }
}

#[test]
fn stop_before_admission_retains_identity_then_closes_without_editing() {
    let (mut app, mut state, photo, requests, _) = started();
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    let original = app.session.develop_of(photo).unwrap();
    state.stop(photo).unwrap();
    assert_eq!(state.jobs.get(&photo).unwrap().identity, identity);
    assert!(requests.try_recv().is_err());
    state_event(&mut state, &mut app, photo, "queued");
    let request = requests.try_recv().unwrap();
    assert!(request.path.ends_with("/stop"));
    assert_eq!(request.identity, identity);
    assert!(!state.jobs.contains_key(&photo));
    assert_eq!(*app.session.develop_of(photo).unwrap(), *original);
}

#[test]
fn cached_conversation_cannot_be_written_to_a_different_library_scope() {
    let (mut app, mut state, photo, _, _) = started();
    let before = app.session.photo_companion(photo).unwrap().unwrap().clone();
    state.conversations.get_mut(&photo).unwrap().scope.as_mut().unwrap().library_generation += 1;
    state.conversations.get_mut(&photo).unwrap().messages.push(json!({"role":"user","text":"must not overwrite another library"}));
    assert!(state.save(&mut app, photo).unwrap_err().contains("未写入当前图库"));
    assert_eq!(app.session.photo_companion(photo).unwrap().unwrap(), &before);
}

#[test]
fn failed_stop_delivery_still_closes_its_connection_after_local_finish() {
    let (mut app, mut state, photo, requests, events) = started();
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    let original = app.session.develop_of(photo).unwrap();
    state_event(&mut state, &mut app, photo, "queued");
    state.stop(photo).unwrap();
    let stop = requests.try_recv().unwrap();
    assert!(stop.path.ends_with("/stop"));
    assert!(!state.jobs.contains_key(&photo));
    events
        .try_send(network::Event::Response {
            submission: 0,
            round: "backend-round".into(),
            path: stop.path,
            identity,
            result: Err("stop delivery failed".into()),
        })
        .unwrap();
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(state.connection.is_none(), "failed stop must retain the existing disconnect fallback");
    assert_eq!(*app.session.develop_of(photo).unwrap(), *original);
}
