use super::*;
use lightcraft_catalog::MemStore;
use lightcraft_engine::library::LibraryStores;

fn app(catalog: MemStore, files: MemStore) -> LightcraftApp {
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "memory".into(), on_disk: false }, true)
        .unwrap();
    LightcraftApp::new(session, crate::Services::default())
}
fn phase(state: &mut State, app: &mut LightcraftApp, photo: PhotoId, phase: &str) {
    let job = &state.jobs[&photo];
    state.server(app, &egui::Context::default(), "round.state", &json!({"round_id":"round","payload":{"submission_seq":job.identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":job.identity.scope.library_generation},"state":phase}})).unwrap();
}
fn render(state: &mut State, app: &mut LightcraftApp) {
    let msg = state.rx.recv_timeout(Duration::from_secs(30)).unwrap();
    state.prepared(app, &egui::Context::default(), msg).unwrap();
}
#[test]
fn real_initial_correction_save_reopen_retains_associations_previews_and_actual_results() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut app = app(catalog.clone(), files.clone());
    let photo = app.session.active().unwrap();
    let (connection, requests, events) = network::test_connection();
    let mut state = State { consent: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
    state.start(&mut app).unwrap();
    requests.try_recv().unwrap();
    let local = state.jobs[&photo].identity.local_round_id.clone();
    phase(&mut state, &mut app, photo, "awaiting_input");
    render(&mut state, &mut app);
    requests.try_recv().unwrap();
    let mut late_request = None;
    for (id, phase_name, exposure) in [("i", "awaiting_initial_execution", 0.4), ("c", "awaiting_correction_execution", 0.2)] {
        phase(&mut state, &mut app, photo, phase_name);
        let revision = app.session.photo_revision(photo);
        let stage = if id == "i" { "initial" } else { "review" };
        let tool = format!("round/{stage}/plan");
        state.server(&mut app, &egui::Context::default(), "message.completed", &json!({"round_id":"round","stage":stage,"payload":{"message_id":"round/1","model_call_id":1,"tool_calls":[{"id":tool,"name":"apply_edit_plan","arguments":{"operations":[]}}]}})).unwrap();
        state.server(&mut app, &egui::Context::default(), "edit.plan", &json!({"round_id":"round","payload":{"plan_id":id,"tool_call_id":tool,"capability_version":"lightcraft-v2","expected_revision":revision,"reason":"light","operations":[{"type":"set_controls","values":{"light.exposure":exposure}}]}})).unwrap();
        render(&mut state, &mut app);
        let request = requests.try_recv().unwrap();
        if id == "c" {
            late_request = Some(request);
        } else {
            events
                .try_send(network::Event::Response {
                    submission: 0,
                    round: "round".into(),
                    path: request.path,
                    identity: request.identity,
                    result: Ok(json!({"accepted":true,"_client_execution_plan_id":id})),
                })
                .unwrap();
            state.poll_inner(&mut app, &egui::Context::default());
        }
    }
    state
        .server(
            &mut app,
            &egui::Context::default(),
            "round.ended",
            &json!({"round_id":"round","trace_id":"trace","payload":{"terminal_state":"completed"}}),
        )
        .unwrap();
    let late = late_request.unwrap();
    assert!(state.jobs.is_empty());
    for response in [json!({"accepted":false,"_client_execution_plan_id":"c"}), json!({"_client_execution_plan_id":"c"})] {
        events
            .try_send(network::Event::Response {
                submission: 0,
                round: "round".into(),
                path: late.path.clone(),
                identity: late.identity.clone(),
                result: Ok(response),
            })
            .unwrap();
        state.poll_inner(&mut app, &egui::Context::default());
        assert_eq!(state.conversations[&photo].records[0]["backend_confirmed_plan"], "i");
    }
    app.session.execute("develop.set", &json!({"ids":[photo.0],"values":{"light.exposure":0.6}})).unwrap();
    let revision = app.session.photo_revision(photo);
    events
        .try_send(network::Event::Response {
            submission: 0,
            round: "round".into(),
            path: late.path,
            identity: late.identity,
            result: Ok(json!({"accepted":true,"_client_execution_plan_id":"c"})),
        })
        .unwrap();
    state.poll_inner(&mut app, &egui::Context::default());
    assert_eq!(state.conversations[&photo].records[0]["backend_confirmed_plan"], "c");
    assert_eq!(app.session.photo_revision(photo), revision, "late receipt records facts only");
    assert!(!state.conversations[&photo].checkpoint.as_ref().unwrap().eligible, "a receipt must never renew a manually invalidated checkpoint");
    app.session.close_library().unwrap();
    let mut reopened = self::app(catalog, files.clone());
    let mut state = State::default();
    state.load(&mut reopened, photo).unwrap();
    assert!(state.jobs.is_empty());
    assert!(state.connection.is_none());
    let record = reopened.session.companion_round(photo, local.as_str()).unwrap().unwrap();
    assert_eq!(record["local_terminal"], "completed");
    assert_eq!(record["trace_id"], "trace");
    assert_eq!(record["stages"][0]["applied_state"]["light"]["exposure"], 0.4);
    assert_eq!(record["stages"][1]["before_state"]["light"]["exposure"], 0.4);
    assert_eq!(record["recent_complete_result"]["light"]["exposure"], 0.2);
    assert_eq!(record["backend_confirmed_plan"], "c");
    assert_eq!(record["stages"][0]["model_call_id"], 1);
    assert_eq!(record["stages"][1]["model_call_id"], 1);
    let assistant_ids: Vec<_> =
        state.conversations[&photo].messages.iter().filter(|m| m["role"] == "assistant").map(|m| m["local_message_id"].clone()).collect();
    assert_eq!(assistant_ids.len(), 2);
    assert_ne!(assistant_ids[0], assistant_ids[1]);
    let history = state.history(&mut reopened, photo).unwrap();
    assert_eq!(history.iter().flat_map(|m| m["images"].as_array().into_iter().flatten()).count(), 3);
    assert!(history.iter().all(|m| m.get("local_message_id").is_none()));
    let root: Value = serde_json::from_slice(&files.get("companion/state.json").unwrap()).unwrap();
    assert_eq!(root["version"], 2);
    // A single damaged attachment retains readable messages, but blocks full model input.
    let image = &state.conversations[&photo].messages[0]["images"][0];
    let key = image["attachment"].as_str().unwrap();
    files.set(key, b"damaged JPEG".to_vec());
    let mut reading = State::default();
    reading.load(&mut reopened, photo).unwrap();
    assert_eq!(reading.conversations[&photo].messages.len(), state.conversations[&photo].messages.len());
    assert!(reading.history(&mut reopened, photo).is_err());
}
#[test]
fn pure_explanation_round_is_saved_and_reopens_without_a_result_or_resumed_job() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut app = app(catalog.clone(), files.clone());
    let photo = app.session.active().unwrap();
    let (connection, requests, _) = network::test_connection();
    let mut state = State { consent: true, connection: Some(connection), ..Default::default() };
    state.start(&mut app).unwrap();
    requests.try_recv().unwrap();
    phase(&mut state, &mut app, photo, "awaiting_input");
    render(&mut state, &mut app);
    requests.try_recv().unwrap();
    phase(&mut state, &mut app, photo, "analyzing_initial");
    state
        .server(
            &mut app,
            &egui::Context::default(),
            "text.delta",
            &json!({"round_id":"round","payload":{"message_id":"m","delta":"Explanation only."}}),
        )
        .unwrap();
    state
        .server(
            &mut app,
            &egui::Context::default(),
            "message.completed",
            &json!({"round_id":"round","payload":{"message_id":"m","model_call_id":"call-1","stage":"initial"}}),
        )
        .unwrap();
    state.server(&mut app, &egui::Context::default(), "round.ended", &json!({"round_id":"round","payload":{"terminal_state":"completed"}})).unwrap();
    app.session.close_library().unwrap();
    let mut app = self::app(catalog, files);
    let mut state = State::default();
    state.load(&mut app, photo).unwrap();
    assert_eq!(state.conversations[&photo].messages[1]["text"], "Explanation only.");
    assert_eq!(state.conversations[&photo].messages[1]["model_call_id"], "call-1");
    assert!(state.conversations[&photo].checkpoint.is_none());
    assert!(state.jobs.is_empty());
}

#[test]
fn detailed_preview_reduces_dimensions_at_quality75_and_keeps_full_frame() {
    let image = lightcraft_raster::Rgba8::from_fn(640, 640, |x, y| {
        let n = ((x as u32).wrapping_mul(747796405) ^ (y as u32).wrapping_mul(2891336453)).wrapping_mul(277803737);
        [(n & 255) as u8, ((n >> 8) & 255) as u8, ((n >> 16) & 255) as u8, 255]
    });
    let options =
        lightcraft_engine::export::ExportOptions { quality: 75, metadata: lightcraft_engine::export::MetadataPolicy::None, ..Default::default() };
    let original = lightcraft_engine::export::encode_image(&image, &options).unwrap();
    assert!(original.len() > 128 * 1024, "fixture must exercise dimension reduction");
    let (data, width, height) = encode_preview(&image).unwrap();
    assert!(data.len() <= 128 * 1024);
    assert!(width < 640);
    assert_eq!(width, height);
    let decoded = lightcraft_codecs::decode(&data, lightcraft_codecs::DecodeOptions::default()).unwrap();
    assert_eq!((decoded.width, decoded.height), (width as u32, height as u32));
    assert_eq!(decoded.orientation, 1);
    assert_eq!(decoded.format, lightcraft_codecs::Format::Jpeg);
}

#[test]
fn failed_render_saves_true_failure_and_late_receipt_cannot_claim_a_successful_plan() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut app = app(catalog.clone(), files.clone());
    let photo = app.session.active().unwrap();
    let original = app.session.develop_of(photo).unwrap();
    let revision = app.session.photo_revision(photo);
    let (connection, requests, events) = network::test_connection();
    let mut state = State { consent: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
    state.start(&mut app).unwrap();
    requests.try_recv().unwrap();
    phase(&mut state, &mut app, photo, "awaiting_input");
    render(&mut state, &mut app);
    requests.try_recv().unwrap();
    phase(&mut state, &mut app, photo, "awaiting_initial_execution");
    let plan = json!({"plan_id":"failed-render","capability_version":"lightcraft-v2","expected_revision":revision,"operations":[{"type":"set_controls","values":{"light.exposure":0.7}}]});
    state.server(&mut app, &egui::Context::default(), "edit.plan", &json!({"round_id":"round","payload":plan})).unwrap();
    let identity = state.jobs[&photo].identity.clone();
    state
        .prepared(
            &mut app,
            &egui::Context::default(),
            WorkerMessage::Prepared {
                identity: identity.clone(),
                plan: Some(plan),
                settings: original.clone(),
                result: Err("fixture render failure".into()),
            },
        )
        .unwrap();
    let request = requests.try_recv().unwrap();
    assert_eq!(request.body["outcome"], "failed");
    assert_eq!(app.session.photo_revision(photo), revision);
    events
        .try_send(network::Event::Response {
            submission: 0,
            round: "round".into(),
            path: request.path,
            identity: request.identity,
            result: Ok(json!({"accepted":true,"_client_execution_plan_id":"failed-render"})),
        })
        .unwrap();
    state.poll_inner(&mut app, &egui::Context::default());
    app.session.close_library().unwrap();
    let mut reopened = self::app(catalog, files);
    let record = reopened.session.companion_round(photo, identity.local_round_id.as_str()).unwrap().unwrap();
    assert_eq!(record["stages"][0]["execution_outcome"], "failed");
    assert_eq!(record["stages"][0]["render_outcome"], "failed");
    assert!(record.get("backend_confirmed_plan").is_none());
    assert!(record.get("recent_complete_result").is_none());
    assert_eq!(*reopened.session.develop_of(photo).unwrap(), *original);
    let mut reading = State::default();
    reading.load(&mut reopened, photo).unwrap();
    assert!(reading.conversations[&photo].messages.iter().any(|m| m["text"].as_str().is_some_and(|t| t.contains("render_failed"))));
}

#[test]
fn invalid_plan_failure_reopens_as_failed_without_claiming_user_cancellation() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut app = app(catalog.clone(), files.clone());
    let photo = app.session.active().unwrap();
    let original = app.session.develop_of(photo).unwrap();
    let (connection, requests, events) = network::test_connection();
    let mut state = State { consent: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
    state.start(&mut app).unwrap();
    requests.try_recv().unwrap();
    let identity = state.jobs[&photo].identity.clone();
    phase(&mut state, &mut app, photo, "awaiting_input");
    render(&mut state, &mut app);
    requests.try_recv().unwrap();
    phase(&mut state, &mut app, photo, "awaiting_initial_execution");
    let revision = app.session.photo_revision(photo);
    events.try_send(network::Event::Server {kind:"edit.plan".into(),data:json!({"round_id":"round","payload":{"plan_id":"invalid","expected_revision":revision,"capability_version":"lightcraft-v2","operations":[{"type":"set_controls","values":{"not.a.control":1.0}}]}})}).unwrap();
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(state.jobs.is_empty());
    assert!(requests.try_recv().unwrap().path.ends_with("/stop"));
    app.session.close_library().unwrap();
    let mut reopened = self::app(catalog, files);
    let record = reopened.session.companion_round(photo, identity.local_round_id.as_str()).unwrap().unwrap();
    assert_eq!(record["local_terminal"], "failed");
    assert_eq!(record["stages"][0]["execution_outcome"], "validation_failed");
    assert_eq!(*reopened.session.develop_of(photo).unwrap(), *original);
}

#[test]
fn post_failure_and_explicit_stop_preserve_published_result_but_have_distinct_terminal_facts() {
    for user_stop in [false, true] {
        let (catalog, files) = (MemStore::new(), MemStore::new());
        let mut app = app(catalog.clone(), files.clone());
        let photo = app.session.active().unwrap();
        let (connection, requests, events) = network::test_connection();
        let mut state = State { consent: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
        state.start(&mut app).unwrap();
        requests.try_recv().unwrap();
        let identity = state.jobs[&photo].identity.clone();
        phase(&mut state, &mut app, photo, "awaiting_input");
        render(&mut state, &mut app);
        requests.try_recv().unwrap();
        phase(&mut state, &mut app, photo, "awaiting_initial_execution");
        let revision = app.session.photo_revision(photo);
        state.server(&mut app,&egui::Context::default(),"edit.plan",&json!({"round_id":"round","payload":{"plan_id":"initial","expected_revision":revision,"capability_version":"lightcraft-v2","operations":[{"type":"set_controls","values":{"light.exposure":0.3}}]}})).unwrap();
        render(&mut state, &mut app);
        let execution = requests.try_recv().unwrap();
        if user_stop {
            state.stop(photo).unwrap();
            state.save(&mut app, photo).unwrap();
        } else {
            events
                .try_send(network::Event::Response {
                    submission: 0,
                    round: "round".into(),
                    path: execution.path,
                    identity: execution.identity,
                    result: Err("fixture POST failed".into()),
                })
                .unwrap();
            state.poll_inner(&mut app, &egui::Context::default());
        }
        assert!(state.jobs.is_empty());
        app.session.close_library().unwrap();
        let mut reopened = self::app(catalog, files);
        let record = reopened.session.companion_round(photo, identity.local_round_id.as_str()).unwrap().unwrap();
        assert_eq!(record["local_terminal"], if user_stop { "cancelled" } else { "failed" });
        assert_eq!(record["stages"][0]["execution_outcome"], "succeeded");
        assert_eq!(record["recent_complete_result"]["light"]["exposure"], 0.3);
        assert_eq!(reopened.session.develop_of(photo).unwrap().light.exposure, 0.3);
    }
}

#[test]
fn real_stop_or_failure_recovers_the_owning_request_and_preserves_another_photo_draft() {
    use crate::headless::Headless;
    for stop in [true, false] {
        let mut h = Headless::new(self::app(MemStore::new(), MemStore::new()), [1000.0, 680.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        for _ in 0..4 {
            h.step();
        }
        let a = h.app.session.active().unwrap();
        let b = h.app.session.catalog.photos().find(|p| p.id != a).unwrap().id;
        let timeout = Duration::from_secs(10);
        let (connection, requests, events) = network::test_connection();
        h.app.ai.next_connection = Some(connection);
        h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
        h.request("ui.text", json!({"text":"A submitted immutable request"}), timeout);
        h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiStart"}), timeout)["ok"], true);
        let creation = requests.try_recv().unwrap();
        assert_eq!(h.app.session.companion_editor(a).unwrap().draft, "");
        events.try_send(network::Event::Server { kind:"round.state".into(), data:json!({"round_id":"draft-round","payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"photo_id":a.0,"library_generation":creation.identity.scope.library_generation},"state":"queued"}}) }).unwrap();
        for _ in 0..4 {
            h.step();
        }
        h.app.run("library.select", json!({"ids":[b.0],"active":b.0})).unwrap();
        for _ in 0..4 {
            h.step();
        }
        h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
        h.request("ui.text", json!({"text":"B newer unsent request"}), timeout);
        if stop {
            h.app.run("ai.stop", json!({"photo":a.0})).unwrap();
        } else {
            events
                .try_send(network::Event::Server {
                    kind: "round.ended".into(),
                    data: json!({"round_id":"draft-round","payload":{"terminal_state":"failed","reason_code":"fixture_failure"}}),
                })
                .unwrap();
        }
        for _ in 0..5 {
            h.step();
        }
        assert_eq!(h.app.session.companion_editor(a).unwrap().draft, "A submitted immutable request");
        assert_eq!(h.app.session.companion_editor(b).unwrap().draft, "B newer unsent request");
        assert_eq!(h.app.run("ai.status", json!({"photo":a.0})).unwrap()["busy"], false);
        assert!(requests.try_iter().all(|request| request.path != "/v1/rounds"), "restoration never submits");
        h.app.run("library.select", json!({"ids":[a.0],"active":a.0})).unwrap();
        for _ in 0..4 {
            h.step();
        }
        h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
        h.request("ui.key", json!({"key":"A","cmd":true}), timeout);
        h.request("ui.text", json!({"text":"A later authored draft"}), timeout);
        events
            .try_send(network::Event::Server {
                kind: "round.ended".into(),
                data: json!({"round_id":"draft-round","payload":{"terminal_state":"failed","reason_code":"late_failure"}}),
            })
            .unwrap();
        for _ in 0..4 {
            h.step();
        }
        h.app.session.persist().unwrap();
        assert_eq!(h.app.session.companion_editor(a).unwrap().draft, "A later authored draft");
        assert_eq!(h.app.session.companion_editor(b).unwrap().draft, "B newer unsent request");
    }
}
