use crate::{LightcraftApp, Services, headless::Headless};
use lightcraft_catalog::MemStore;
use lightcraft_engine::{Session, library::LibraryStores};
use serde_json::{Value, json};

fn headless(records: Vec<Value>) -> (Headless, lightcraft_catalog::PhotoId) {
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "memory".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"assistant","text":"A natural result."}],"round_records":records,"rounds":[]}),
        )
        .unwrap();
    session.persist().unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    (h, photo)
}
fn stage(name: &str, before: f64, after: f64) -> Value {
    let mut a = lightcraft_develop::DevelopSettings::default();
    a.light.exposure = before;
    let mut b = a.clone();
    b.light.exposure = after;
    json!({"stage":name,"plan_id":name,"reason":"Keep the highlights natural.","before_state":a,"proposed_state":b,"applied_state":b,"execution_outcome":"succeeded","locally_published":true,"delivery_state":"confirmed"})
}
#[test]
fn archived_round_has_one_result_card_with_absolute_initial_and_correction_values() {
    let (mut h, _) = headless(vec![
        json!({"local_round_id":"round-card","stages":[stage("initial",0.0,0.35),stage("correction",0.35,0.2)],"local_terminal":"completed"}),
        json!({"local_round_id":"pure-answer","stages":[],"local_terminal":"completed"}),
    ]);
    let status = h.app.run("ai.status", json!({})).unwrap();
    let cards = status["resultCards"].as_array().expect("caller-visible result cards");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["localRoundId"], "round-card");
    assert_eq!(cards[0]["stages"][0]["changes"][0]["before"], "0.00");
    assert_eq!(cards[0]["stages"][0]["changes"][0]["after"], "+0.35");
    assert_eq!(cards[0]["stages"][1]["changes"][0]["before"], "+0.35");
    assert_eq!(cards[0]["stages"][1]["changes"][0]["after"], "+0.20");
    for _ in 0..4 {
        h.step();
    }
    assert!(h.app.widgets.iter().any(|(id, _)| id == "ai:card:round-card"));
    assert!(h.app.widgets.iter().any(|(id, _)| id == "button:aiParameters:round-card"));
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "ai:card:pure-answer"));
}
#[test]
fn result_card_distinguishes_failed_target_from_retained_and_unconfirmed_actual_results() {
    let mut failed = stage("correction", 0.35, 0.2);
    failed.as_object_mut().unwrap().remove("applied_state");
    failed["locally_published"] = json!(false);
    failed["execution_outcome"] = json!("failed");
    failed["delivery_state"] = json!("not_sent");
    let mut published = stage("correction", 0.35, 0.2);

    published["delivery_state"] = json!("failed");
    let (mut h, _) = headless(vec![
        json!({"local_round_id":"failed-target","stages":[stage("initial",0.0,0.35),failed],"local_terminal":"failed"}),
        json!({"local_round_id":"published-correction","stages":[stage("initial",0.0,0.35),published],"local_terminal":"failed"}),
    ]);
    let result = h.app.run("ai.status", json!({})).unwrap();
    let c = &result["resultCards"][0]["stages"][1];
    assert_eq!(c["status"], "not_applied");
    assert_eq!(c["changes"][0]["target"], "+0.20");
    assert_eq!(c["changes"][0]["actual"], "+0.35");
    let c = &result["resultCards"][1]["stages"][1];
    assert_eq!(c["status"], "applied");
    assert_eq!(c["feedback"], "unconfirmed");
    assert_eq!(c["changes"][0]["actual"], "+0.20");
}
fn outgoing(h: &mut Headless, requests: &std::sync::mpsc::Receiver<super::network::Request>, suffix: &str) -> super::network::Request {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        h.step();
        if let Ok(r) = requests.try_recv() {
            assert!(r.path.ends_with(suffix), "{}", r.path);
            return r;
        }
        assert!(std::time::Instant::now() < deadline, "missing {suffix}");
        std::thread::yield_now();
    }
}
#[test]
fn actual_white_balance_result_keeps_execution_time_kelvin_values_after_reopen() {
    actual_wb(false);
}
#[test]
fn actual_rendered_white_balance_result_uses_relative_scale_after_reopen() {
    actual_wb(true);
}
fn actual_wb(relative: bool) {
    let catalog = MemStore::new();
    let files = MemStore::new();
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "memory".into(), on_disk: false },
            true,
        )
        .unwrap();
    let native = if relative {
        let path =
            format!("/tmp/lightcraft-ux-execution-20261013/16/rendered-{}.png", lightcraft_engine::companion::LocalRoundId::new().unwrap().as_str());
        let bytes = lightcraft_engine::export::encode_image(
            &lightcraft_raster::Rgba8::filled(32, 24, [96, 128, 160, 255]),
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
        )
        .unwrap();
        std::fs::write(&path, bytes).unwrap();
        let (loader, probe) = lightcraft_engine::files::fs_hooks();
        session.media.file_loader = Some(loader);
        session.media.file_probe = Some(probe);
        lightcraft_engine::import::import(&mut session, &[path.clone()], lightcraft_engine::import::ImportMode::Add).unwrap();
        let id = session.catalog.photos().find(|p| p.source == lightcraft_catalog::Source::File { path: path.clone() }).unwrap().id;
        session.execute("library.select", &json!({"ids":[id.0],"active":id.0})).unwrap();
        Some(path)
    } else {
        None
    };
    let photo = session.active().unwrap();
    let (connection, requests, events) = super::network::test_connection();
    let mut app = LightcraftApp::new(session, Services::default());
    app.ai.next_connection = Some(connection);
    let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    for _ in 0..4 {
        h.step();
    }
    let t = std::time::Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), t)["ok"], true);
    if let Some(path) = &native {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while h.app.session.media.availability.exists(path) != Some(true) {
            h.step();
            std::thread::yield_now();
            assert!(std::time::Instant::now() < deadline);
        }
    }
    h.app.run("ai.start", json!({"prompt":"Warm the photo."})).unwrap();
    let creation = outgoing(&mut h, &requests, "/v1/rounds");
    let phase = |name: &str| {
        events.send(super::network::Event::Server{kind:"round.state".into(),data:json!({"round_id":"wb-round","payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"library_generation":creation.identity.scope.library_generation,"photo_id":photo.0},"state":name}})}).unwrap()
    };
    phase("awaiting_input");
    let _input = outgoing(&mut h, &requests, "/input");
    phase("awaiting_initial_execution");
    let op = json!({"type":"set_white_balance","mode":"custom","temp":8125.0,"tint":-30.0});
    events.send(super::network::Event::Server{kind:"message.completed".into(),data:json!({"round_id":"wb-round","stage":"initial","payload":{"message_id":"wb","model_call_id":1,"tool_calls":[{"id":"wb-tool","name":"apply_edit_plan","arguments":{"operations":[op]}}]}})}).unwrap();
    events.send(super::network::Event::Server{kind:"edit.plan".into(),data:json!({"round_id":"wb-round","payload":{"plan_id":"wb-plan","tool_call_id":"wb-tool","capability_version":"lightcraft-v2","expected_revision":h.app.session.photo_revision(photo),"reason":"Warmer and less magenta.","operations":[op]}})}).unwrap();
    let execution = outgoing(&mut h, &requests, "/execution");
    assert_eq!(execution.body["outcome"], "succeeded");
    events
        .send(super::network::Event::Server {
            kind: "round.ended".into(),
            data: json!({"round_id":"wb-round","payload":{"terminal_state":"completed"}}),
        })
        .unwrap();
    for _ in 0..4 {
        h.step();
    }
    let values = h.app.run("ai.status", json!({})).unwrap();
    let changes = values["resultCards"][0]["stages"][0]["changes"].as_array().unwrap();
    let temp = changes.iter().find(|v| v["control"] == "wb.temp").expect("effective temperature");
    if relative {
        assert_eq!(temp["before"], "0");
        assert_eq!(temp["actual"], "+38");
        assert_eq!(temp["unit"], "Relative");
    } else {
        assert_eq!(temp["before"], "6500");
        assert_eq!(temp["actual"], "8125");
        assert_eq!(temp["unit"], "K");
    }
    assert!(changes.iter().any(|v| v["control"] == "wb.mode" && v["before"] == "As Shot" && v["actual"] == "Custom"));
    h.app.run("develop.set", json!({"control":"wb.temp","value":5000.0})).unwrap();
    h.app.session.close_library().unwrap();
    let mut session = Session::new();
    session
        .open_library_in(LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "memory".into(), on_disk: false }, false)
        .unwrap();
    let mut app = LightcraftApp::new(session, Services::default());
    app.run("library.select", json!({"ids":[photo.0],"active":photo.0})).unwrap();
    let after = app.run("ai.status", json!({})).unwrap();
    assert_eq!(after["resultCards"][0]["stages"][0]["changes"], values["resultCards"][0]["stages"][0]["changes"]);
    if let Some(path) = native {
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn card_history_navigation_keeps_current_photo_after_later_manual_edit() {
    let (mut h, photo) = headless(vec![
        json!({"local_round_id":"history-card","stages":[stage("initial",0.0,0.35)],"recent_complete_result":stage("initial",0.0,0.35)["applied_state"],"local_terminal":"completed"}),
    ]);
    h.app.run("develop.set", json!({"control":"light.exposure","value":0.8})).unwrap();
    let before = h.app.session.develop_of(photo).unwrap();
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["resultCards"][0]["currentPhotoHasLaterEdits"], true);
    for _ in 0..4 {
        h.step();
    }
    let result = h.request("ui.clickWidget", json!({"id":"button:aiHistory:history-card"}), std::time::Duration::from_secs(10));
    assert_eq!(result["ok"], true);
    assert_eq!(*h.app.session.develop_of(photo).unwrap(), *before);
    assert_eq!(serde_json::to_value(h.app.ui.right).unwrap(), "activity");
}
#[test]
fn published_crop_result_displays_actual_composition_parameters() {
    let mut s = stage("initial", 0.0, 0.0);
    let mut after: lightcraft_develop::DevelopSettings = serde_json::from_value(s["applied_state"].clone()).unwrap();
    after.crop.geometry.rect = lightcraft_geom::Rect::new(0.1, 0.2, 0.9, 0.8);
    after.crop.flip_h = true;
    s["applied_state"] = json!(after);
    s["proposed_state"] = json!(after);
    let (mut h, _) = headless(vec![json!({"local_round_id":"crop-card","stages":[s]})]);
    let status = h.app.run("ai.status", json!({})).unwrap();
    let changes = status["resultCards"][0]["stages"][0]["changes"].as_array().unwrap();
    assert!(changes.iter().any(|v| v["control"] == "crop.left" && v["before"] == "0.0" && v["actual"] == "10.0" && v["unit"] == "%"));
    assert!(changes.iter().any(|v| v["control"] == "crop.flip_h" && v["before"] == "Off" && v["actual"] == "On"));
}
#[test]
fn card_parameter_control_shows_readable_absolute_values_in_both_languages() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let (mut h, _) = headless(vec![
            json!({"local_round_id":"visible-card","stages":[stage("initial",0.0,0.35),stage("correction",0.35,0.2)],"local_terminal":"completed"}),
        ]);
        h.app.ui.language = language;
        for _ in 0..4 {
            h.step();
        }
        let expected = if language == crate::i18n::Locale::En { "Round result" } else { "本轮结果" };
        assert!(h.painted_text().iter().any(|text| text == expected));
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiParameters:visible-card"}), std::time::Duration::from_secs(10))["ok"], true);
        for _ in 0..3 {
            h.step();
        }
        let texts = h.painted_text();
        assert!(texts.iter().any(|text| text == "0.00 → +0.35"));
        assert!(texts.iter().any(|text| text == "+0.35 → +0.20"));
    }
}
