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
type RoundFixture = (
    Headless,
    lightcraft_catalog::PhotoId,
    String,
    lightcraft_develop::DevelopSettings,
    Receiver<super::network::Request>,
    std::sync::mpsc::SyncSender<super::network::Event>,
);
fn actual_round() -> RoundFixture {
    actual_round_with_session(Session::with_demo())
}
fn actual_round_with_session(mut session: Session) -> RoundFixture {
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
    (h, photo, local, start, requests, events)
}
#[test]
fn actual_round_card_reverts_its_whole_start_after_consent_is_revoked() {
    let (mut h, photo, round, start, _requests, _events) = actual_round();
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
    assert_eq!(
        status["resultCards"][0]["currentPhotoHasLaterEdits"], false,
        "the explicit round revert is reported as reverted, not as unrelated later editing"
    );
    assert!(status["text"].as_str().unwrap().contains("This round was reverted"));
    h.app.run("develop.set", json!({"values":{"light.exposure":0.6}})).unwrap();
    h.app.run("develop.set", json!({"values":{"light.exposure":-0.2}})).unwrap();
    let later = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(later["resultCards"][0]["reverted"], true);
    assert_eq!(later["resultCards"][0]["currentPhotoHasLaterEdits"], true);
    assert_eq!(later["resultCards"][0]["canUndo"], false);
}

#[test]
fn native_reopen_keeps_real_round_and_ordinary_history_without_a_runtime_grant() {
    native_reopen(false);
}
#[test]
fn native_reverted_round_fact_is_readable_after_reopen_without_undo_permission() {
    native_reopen(true);
}
fn native_reopen(reverted: bool) {
    let dir = std::env::temp_dir().join(format!("lc-ux17-native-{}", lightcraft_engine::companion::LocalRoundId::new().unwrap().as_str()));
    let mut session = Session::new().with_fs();
    session.open_library(&dir, true).unwrap();
    let photo = session.catalog.photos().next().unwrap().id;
    session.execute("library.select", &json!({"ids":[photo.0],"active":photo.0})).unwrap();
    let (mut h, photo, round, start, _requests, _events) = actual_round_with_session(session);
    if reverted {
        h.app.run("ai.undo", json!({"photo":photo.0,"local_round_id":round})).unwrap();
        assert_eq!(*h.app.session.develop_of(photo).unwrap(), start);
        // Exercise actual streaming selection using local opaque data, while
        // preserving the real execution/revert facts and model transcript.
        let mut stored = h.app.session.load_companion_conversation(photo).unwrap().unwrap();
        stored["round_records"][0]["diagnostic_padding"] = json!("opaque local diagnostic ".repeat(20_000));
        h.app.session.save_companion_conversation(photo, stored).unwrap();
    }
    let current = h.app.session.develop_of(photo).unwrap();
    h.app.session.close_library().unwrap();
    drop(h);
    let mut session = Session::new().with_fs();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match session.open_library(&dir, false) {
            Ok(_) => break,
            Err(error) => {
                assert!(Instant::now() < deadline, "{error}");
                std::thread::yield_now();
            }
        }
    }
    session.execute("library.select", &json!({"ids":[photo.0],"active":photo.0})).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historyLoading"] == false {
            break status;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    let cards = status["resultCards"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["localRoundId"], round);
    assert_eq!(cards[0]["canUndo"], false);
    assert_eq!(cards[0]["reverted"], reverted);
    if reverted {
        assert!(status["text"].as_str().unwrap().contains("This round's edits were reverted"));
    }
    assert_eq!(cards[0]["stages"].as_array().unwrap().len(), 2);
    assert_eq!(*h.app.session.develop_of(photo).unwrap(), *current);
    assert!(h.app.run("ai.undo", json!({"local_round_id":round})).is_err());
    let latest = h.app.session.catalog.photo(photo).unwrap().history.len() - if reverted { 3 } else { 2 };
    h.app.run("history.restore", json!({"index":latest})).unwrap();
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.4);
    h.app.session.close_library().unwrap();
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

fn frames(h: &mut Headless) {
    for _ in 0..4 {
        h.step();
    }
}
fn visible_label(h: &Headless, literal: &str) -> bool {
    h.app.widgets.iter().find(|(id,_)|id=="ai:messages").is_some_and(|(_,viewport)| h.view.painted_shapes().iter().any(|shape|matches!(shape,egui::epaint::Shape::Text(text) if text.opacity_factor>0.0 && text.galley.text()==literal && viewport.contains_rect(text.visual_bounding_rect()))))
}
fn show_label(h: &mut Headless, literal: &str) {
    for index in 0..180 {
        if visible_label(h, literal) {
            return;
        }
        let target = h
            .app
            .widgets
            .iter()
            .find(|(id, _)| id == "ai:messages")
            .filter(|(_, viewport)| viewport.height() >= 18.0)
            .map(|(_, viewport)| viewport.center())
            .unwrap_or_else(|| {
                let composer = h.app.widgets.iter().find(|(id, _)| id == "ai:composer").unwrap().1;
                egui::pos2(composer.right() - 2.0, composer.center().y)
            });
        h.app.synthetic.push(egui::Event::PointerMoved(target));
        h.app.synthetic.push(egui::Event::MouseWheel {
            phase: egui::TouchPhase::Move,
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, if index < 90 { -8.0 } else { 8.0 }),
            modifiers: egui::Modifiers::NONE,
        });
        frames(h);
    }
    panic!("complete actual message galley was never visible: {literal}");
}
fn screen(h: &mut Headless, name: &str) {
    if let Ok(dir) = std::env::var("UX17_SCREENSHOT_DIR") {
        h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        h.app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        assert_eq!(h.request("ui.screenshot", json!({"path":format!("{dir}/{name}.png"),"headless":true}), Duration::from_secs(30))["ok"], true);
    }
}
#[test]
fn real_round_revert_card_is_readable_and_clickable_in_both_languages_and_tools_sizes() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        for layout in ["normal", "narrow", "float120", "float240", "stack120", "stack240"] {
            let (mut h, photo, round, start, _requests, _events) = actual_round();
            h.app.ui.language = language;
            let timeout = Duration::from_secs(10);
            let size = if layout == "normal" {
                [1400, 1000]
            } else if layout == "narrow" {
                [620, 520]
            } else {
                [1000, 680]
            };
            assert_eq!(h.request("ui.resize", json!({"width":size[0],"height":size[1]}), timeout)["ok"], true);
            frames(&mut h);
            if layout.starts_with("float") || layout.starts_with("stack") {
                super::panel_tests::short_tools(&mut h, layout.starts_with("stack"), if layout.ends_with("120") { 120.0 } else { 240.0 });
            }
            let literal = if language == crate::i18n::Locale::ZhHans { "撤回本轮" } else { "Undo AI Round" };
            show_label(&mut h, literal);
            let id = format!("button:aiUndo:{round}");
            let target = h.app.widgets.iter().find(|(name, _)| name == &id).unwrap().1;
            let tools = h.app.widgets.iter().find(|(name, _)| name == "panel:right_panel").unwrap().1;
            let viewport = h.app.widgets.iter().find(|(name, _)| name == "ai:messages").unwrap().1;
            assert!(tools.contains_rect(target));
            assert!(viewport.contains_rect(target));
            let zoom = h.app.ui.zoom;
            screen(&mut h, &format!("card-{layout}-{}", language.code()));
            assert_eq!(h.request("ui.clickWidget", json!({"id":id}), timeout)["ok"], true);
            assert_eq!(*h.app.session.develop_of(photo).unwrap(), start);
            assert_eq!(h.app.ui.zoom, zoom, "card click never reaches the canvas");
            let status = h.app.run("ai.status", json!({})).unwrap();
            assert_eq!(status["resultCards"][0]["reverted"], true);
            assert_eq!(status["resultCards"][0]["canUndo"], false);
            if layout == "normal" {
                show_label(&mut h, if language == crate::i18n::Locale::ZhHans { "已撤回本轮。" } else { "This round was reverted." });
                screen(&mut h, &format!("reverted-{layout}-{}", language.code()));
            }
        }
    }
}
