//! Caller-facing runtime revert tests: widgets, real publication and transport exchange.
use crate::{LightcraftApp, Services, headless::Headless};
use lightcraft_engine::Session;
use serde_json::json;
use std::{
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

fn outgoing(h: &mut Headless, requests: &Receiver<super::network::Request>, suffix: &str) -> super::network::Request {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        h.step();
        if let Ok(request) = requests.try_recv() {
            assert!(request.path.ends_with(suffix), "{}", request.path);
            return request;
        }
        assert!(Instant::now() < deadline, "missing {suffix}");
        std::thread::yield_now();
    }
}
fn actual_round() -> (Headless, lightcraft_catalog::PhotoId, String, lightcraft_develop::DevelopSettings) {
    let mut session = Session::with_demo();
    let photo = session.active().unwrap();
    let mut start = (*session.develop_of(photo).unwrap()).clone();
    start.light.exposure = -0.2;
    start.effects.clarity = 17.0;
    session.set_develop(photo, start.clone(), "Manual edit").unwrap();
    let (connection, requests, events) = super::network::test_connection();
    let mut app = LightcraftApp::new(session, Services::default());
    app.ai.next_connection = Some(connection);
    let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    for _ in 0..4 {
        h.step();
    }
    let timeout = Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
    h.app.run("ai.start", json!({"prompt":"Naturally optimize this photo."})).unwrap();
    let creation = outgoing(&mut h, &requests, "/v1/rounds");
    let local = creation.identity.local_round_id.as_str().to_owned();
    events.send(super::network::Event::Server { kind:"round.state".into(),data:json!({"round_id":"revert-round","payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"library_generation":creation.identity.scope.library_generation,"photo_id":photo.0},"state":"awaiting_input"}})}).unwrap();
    outgoing(&mut h, &requests, "/input");
    for (stage, phase, exposure) in [("initial", "awaiting_initial_execution", 0.4), ("correction", "awaiting_correction_execution", 0.2)] {
        events.send(super::network::Event::Server { kind:"round.state".into(),data:json!({"round_id":"revert-round","payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"library_generation":creation.identity.scope.library_generation,"photo_id":photo.0},"state":phase}})}).unwrap();
        let op = json!({"type":"set_controls","values":{"light.exposure":exposure,"effects.clarity":30.0}});
        events.send(super::network::Event::Server { kind:"message.completed".into(),data:json!({"round_id":"revert-round","stage":stage,"payload":{"message_id":stage,"model_call_id":if stage=="initial" {1}else{2},"tool_calls":[{"id":stage,"name":"apply_edit_plan","arguments":{"operations":[op]}}]}})}).unwrap();
        events.send(super::network::Event::Server { kind:"edit.plan".into(),data:json!({"round_id":"revert-round","payload":{"plan_id":stage,"tool_call_id":stage,"capability_version":"lightcraft-v2","expected_revision":h.app.session.photo_revision(photo),"reason":"Keep the highlights natural.","operations":[op]}})}).unwrap();
        assert_eq!(outgoing(&mut h, &requests, "/execution").body["outcome"], "succeeded");
    }
    events
        .send(super::network::Event::Server {
            kind: "round.ended".into(),
            data: json!({"round_id":"revert-round","payload":{"terminal_state":"completed"}}),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["busy"] == false && status["workerBusy"] == false {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    for _ in 0..4 {
        h.step();
    }
    (h, photo, local, start)
}
#[test]
fn actual_round_card_reverts_its_whole_start_after_consent_is_revoked() {
    let (mut h, photo, round, start) = actual_round();
    let timeout = Duration::from_secs(10);
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.2);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
    for _ in 0..4 {
        h.step();
    }
    assert!(h.app.widgets.iter().any(|(id, _)| id == &format!("button:aiUndo:{round}")), "the actual card owns the explicit action");
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiUndo"), "no implicit latest-round footer action");
    assert_eq!(h.request("ui.clickWidget", json!({"id":format!("button:aiUndo:{round}")}), timeout)["ok"], true);
    assert_eq!(*h.app.session.develop_of(photo).unwrap(), start);
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["resultCards"][0]["reverted"], true);
    assert_eq!(status["resultCards"][0]["canUndo"], false);
    assert!(status["text"].as_str().unwrap().contains("This round was reverted"));
}
