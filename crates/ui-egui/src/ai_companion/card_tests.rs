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
            json!({"schema_version":2,"messages":records.iter().map(|r|json!({"role":"assistant","text":"A natural result.","local_round_id":r["local_round_id"]})).collect::<Vec<_>>(),"round_records":records,"rounds":[]}),
        )
        .unwrap();
    session.persist().unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    ready(&mut h);
    (h, photo)
}
fn ready(h: &mut Headless) -> Value {
    for _ in 0..200 {
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historyLoading"] == false {
            return status;
        }
        h.step();
    }
    panic!("historical card projection did not finish");
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
    assert_eq!(result["resultCards"][0]["retainedResult"], "The initial complete result was retained.");
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
        let path = std::env::temp_dir()
            .join(format!("lc-rendered-wb-{}.png", lightcraft_engine::companion::LocalRoundId::new().unwrap().as_str()))
            .to_string_lossy()
            .into_owned();
        let bytes = lightcraft_engine::export::encode_image(
            &lightcraft_raster::Rgba8::filled(32, 24, [96, 128, 160, 255]),
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
        )
        .unwrap();
        std::fs::write(&path, bytes).unwrap();
        let (loader, probe) = lightcraft_engine::files::fs_hooks();
        session.media.file_loader = Some(loader);
        session.media.file_probe = Some(probe);
        lightcraft_engine::import::import(&mut session, std::slice::from_ref(&path), lightcraft_engine::import::ImportMode::Add).unwrap();
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
    let mut reopened = Headless::new(app, [1400.0, 1000.0], 1.0);
    let after = ready(&mut reopened);
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
#[test]
fn published_mask_result_displays_its_local_parameter_and_name() {
    let mut s = stage("initial", 0.0, 0.0);
    let mut before: lightcraft_develop::DevelopSettings = serde_json::from_value(s["before_state"].clone()).unwrap();
    before.masks.push(lightcraft_develop::Mask { id: 12, name: "Foreground".into(), ..Default::default() });
    let mut after = before.clone();
    after.masks[0].adjust.exposure = 0.5;
    s["before_state"] = json!(before);
    s["proposed_state"] = json!(after);
    s["applied_state"] = json!(after);
    let (mut h, _) = headless(vec![json!({"local_round_id":"mask-card","stages":[s]})]);
    let status = h.app.run("ai.status", json!({})).unwrap();
    let rows = status["resultCards"][0]["stages"][0]["changes"].as_array().unwrap();
    assert!(rows.iter().any(|v| v["control"] == "mask.12.exposure"
        && v["context"] == "Foreground"
        && v["before"] == "0.00"
        && v["actual"] == "+0.50"
        && v["unit"] == "EV"));
}
#[test]
fn cold_background_page_reads_old_card_from_large_round_record() {
    let mut files = MemStore::new();
    files.background = true;
    let mut session = Session::new();
    session
        .open_library_in(LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(files), dir: "memory".into(), on_disk: false }, true)
        .unwrap();
    let photo = session.active().unwrap();
    let display = |value: &str| json!({"values":[{"control":"light.exposure","label":"Exposure","value":value,"unit":"EV"}]});
    let mut first = stage("initial", 0.0, 0.35);
    first["display_before"] = display("0.00");
    first["display_target"] = display("+0.35");
    first["display_actual"] = display("+0.35");
    let mut records =
        vec![json!({"local_round_id":"old-large-card","request_text":"A".repeat(6*1024*1024),"stages":[first],"local_terminal":"completed"})];
    let mut messages = vec![json!({"role":"assistant","text":format!("OLD_ROUND_VISIBLE{}","A".repeat(270_000)),"local_round_id":"old-large-card"})];
    for i in 1..200 {
        let id = format!("later-{i}");
        records.push(json!({"local_round_id":id,"stages":[],"local_terminal":"completed"}));
        messages.push(json!({"role":"assistant","text":format!("Later {i}"),"local_round_id":id}));
    }
    session.save_companion_conversation(photo, json!({"schema_version":2,"messages":messages,"round_records":records,"rounds":[]})).unwrap();
    session.persist().unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    h.app.run("ai.history", json!({"record":0})).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let status = loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historyLoading"] == false && status["text"].as_str().unwrap().contains("OLD_ROUND_VISIBLE") {
            break status;
        }
        assert!(std::time::Instant::now() < deadline, "{status}");
        std::thread::yield_now();
    };
    assert_eq!(status["resultCards"][0]["localRoundId"], "old-large-card");
    assert_eq!(status["resultCards"][0]["stages"][0]["changes"][0]["actual"], "+0.35");
    for _ in 0..4 {
        h.step();
    }
    let mut readable = false;
    for index in 0..120 {
        let viewport = h.app.widgets.iter().find(|(id, _)| id == "ai:messages").unwrap().1;
        readable = h.view.painted_shapes().iter().any(|shape| matches!(shape, egui::epaint::Shape::Text(t) if t.galley.text() == "Round result" && viewport.contains_rect(t.visual_bounding_rect())));
        if readable {
            break;
        }
        if index % 10 == 0 {
            h.app.synthetic.push(egui::Event::PointerMoved(viewport.center()));
            h.app.synthetic.push(egui::Event::MouseWheel {
                phase: egui::TouchPhase::Move,
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -100_000.0),
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.step();
    }
    assert!(readable, "the historical card title must actually be visible after scrolling");
}
#[test]
fn published_non_slider_plan_exposes_its_actual_structural_parameters() {
    let mut s = stage("initial", 0.0, 0.0);
    let mut after: lightcraft_develop::DevelopSettings = serde_json::from_value(s["applied_state"].clone()).unwrap();
    after.treatment = lightcraft_develop::Treatment::Bw;
    after.curve.master = vec![lightcraft_geom::Point::new(0.0, 0.0), lightcraft_geom::Point::new(0.5, 0.6), lightcraft_geom::Point::new(1.0, 1.0)];
    after.point_colors.push(lightcraft_develop::PointColor::default());
    after.masks.push(lightcraft_develop::Mask {
        id: 12,
        name: "Sky".into(),
        components: vec![lightcraft_develop::MaskComponent {
            name: None,
            op: Default::default(),
            invert: false,
            shape: lightcraft_develop::MaskShape::Linear { start: lightcraft_geom::Point::new(0.1, 0.2), end: lightcraft_geom::Point::new(0.8, 0.9) },
        }],
        ..Default::default()
    });
    s["proposed_state"] = json!(after);
    s["applied_state"] = json!(after);
    let (mut h, _) = headless(vec![json!({"local_round_id":"structure-card","stages":[s]})]);
    let status = h.app.run("ai.status", json!({})).unwrap();
    let rows = status["resultCards"][0]["stages"][0]["changes"].as_array().unwrap();
    for (id, expected) in [
        ("treatment", "B&W"),
        ("curve.master", "(0.0, 0.0), (50.0, 60.0), (100.0, 100.0)"),
        ("pointColor.count", "1"),
        ("mask.count", "1"),
        ("mask.12.component.0.start", "(10.0, 20.0)"),
    ] {
        assert!(rows.iter().any(|v| v["control"] == id && v["actual"] == expected), "missing actual {id}: {rows:?}");
    }
}
#[test]
fn result_cards_are_readable_and_clipped_in_normal_narrow_and_short_tools() {
    use std::time::Duration;
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        for layout in ["normal", "narrow", "float120", "float240", "stack120", "stack240"] {
            let mut correction = stage("correction", 0.35, 0.2);
            correction["delivery_state"] = json!("not_sent");
            let (mut h, _) =
                headless(vec![json!({"local_round_id":"layout-card","stages":[stage("initial",0.0,0.35),correction],"local_terminal":"failed"})]);
            h.app.ui.language = language;
            let t = Duration::from_secs(10);
            let size = if layout == "normal" {
                [1400, 1000]
            } else if layout == "narrow" {
                [620, 520]
            } else {
                [1000, 680]
            };
            assert_eq!(h.request("ui.resize", json!({"width":size[0],"height":size[1]}), t)["ok"], true);
            for _ in 0..4 {
                h.step();
            }
            if layout.starts_with("float") || layout.starts_with("stack") {
                super::panel_tests::short_tools(&mut h, layout.starts_with("stack"), if layout.ends_with("120") { 120.0 } else { 240.0 });
            }
            let zoom = h.app.ui.zoom;
            let id = "button:aiParameters:layout-card";
            for _ in 0..80 {
                if h.app.widgets.iter().any(|(name, rect)| name == id && rect.height() >= 12.0) {
                    break;
                }
                let viewport = h.app.widgets.iter().find(|(id, _)| id == "ai:composer").unwrap().1;
                h.app.synthetic.push(egui::Event::PointerMoved(egui::pos2(viewport.right() - 2.0, viewport.center().y)));
                h.app.synthetic.push(egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -35.0),
                    modifiers: egui::Modifiers::NONE,
                });
                for _ in 0..4 {
                    h.step();
                }
            }
            assert_eq!(h.request("ui.clickWidget", json!({"id":id}), t)["ok"], true, "{layout}");
            for _ in 0..4 {
                h.step();
            }
            let tools = h.app.widgets.iter().find(|(id, _)| id == "panel:right_panel").unwrap().1;
            for (id, rect) in &h.app.widgets {
                if id.starts_with("ai:card:")
                    || id.starts_with("ai:stage:")
                    || id.starts_with("button:aiParameters:")
                    || id.starts_with("button:aiHistory:")
                {
                    assert!(tools.contains_rect(*rect), "{layout}: {id} {rect:?}");
                }
            }
            for literal in ["0.00 → +0.35", "+0.35 → +0.20"] {
                let mut visible = false;
                for index in 0..160 {
                    let viewport = h.app.widgets.iter().find(|(id, _)| id == "ai:messages").map(|(_, r)| *r);
                    visible = viewport.is_some_and(|v| h.view.painted_shapes().iter().any(|shape| matches!(shape,egui::epaint::Shape::Text(t) if t.opacity_factor>0.0 && t.galley.text()==literal && v.contains_rect(t.visual_bounding_rect()))));
                    if visible {
                        break;
                    }
                    let target = viewport.filter(|r| r.height() >= 18.0).map(|r| r.center()).unwrap_or_else(|| {
                        let r = h.app.widgets.iter().find(|(id, _)| id == "ai:composer").unwrap().1;
                        egui::pos2(r.right() - 2.0, r.center().y)
                    });
                    h.app.synthetic.push(egui::Event::PointerMoved(target));
                    h.app.synthetic.push(egui::Event::MouseWheel {
                        phase: egui::TouchPhase::Move,
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, if index < 80 { 12.0 } else { -12.0 }),
                        modifiers: egui::Modifiers::NONE,
                    });
                    for _ in 0..3 {
                        h.step();
                    }
                }
                assert!(visible, "{layout}/{}: actual parameter literal {literal} is not visibly readable", language.code());
            }
            assert_eq!(h.app.ui.zoom, zoom, "card clicks must not hit the canvas");
            if let Ok(dir) = std::env::var("UX16_SCREENSHOT_DIR") {
                h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
                h.app.services.png = Some(Box::new(|image| {
                    lightcraft_engine::export::encode_image(
                        image,
                        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                    )
                    .unwrap()
                }));
                assert_eq!(
                    h.request(
                        "ui.screenshot",
                        json!({"path":format!("{dir}/card-{layout}-{}.png",language.code()),"headless":true}),
                        Duration::from_secs(30)
                    )["ok"],
                    true
                );
            }
        }
    }
}
#[test]
fn moving_a_brush_stroke_shows_its_actual_points_even_when_count_is_unchanged() {
    let mut s = stage("initial", 0.0, 0.0);
    let mut before: lightcraft_develop::DevelopSettings = serde_json::from_value(s["before_state"].clone()).unwrap();
    let brush = |x| lightcraft_develop::MaskComponent {
        name: None,
        op: Default::default(),
        invert: false,
        shape: lightcraft_develop::MaskShape::Brush {
            strokes: vec![lightcraft_develop::BrushStroke { points: vec![lightcraft_geom::Point::new(x, 0.2)], ..Default::default() }],
        },
    };
    before.masks.push(lightcraft_develop::Mask { id: 7, name: "Background".into(), components: vec![brush(0.1)], ..Default::default() });
    let mut after = before.clone();
    after.masks[0].components = vec![brush(0.3)];
    s["before_state"] = json!(before);
    s["proposed_state"] = json!(after);
    s["applied_state"] = json!(after);
    let (mut h, _) = headless(vec![json!({"local_round_id":"brush-card","stages":[s]})]);
    let result = h.app.run("ai.status", json!({})).unwrap();
    let changes = result["resultCards"][0]["stages"][0]["changes"].as_array().unwrap();
    assert!(
        changes
            .iter()
            .any(|v| v["control"] == "mask.7.component.0.stroke.0.points" && v["before"] == "(10.0, 20.0)" && v["actual"] == "(30.0, 20.0)")
    );
}
#[test]
fn fallback_fragment_card_lookup_keeps_its_owner_across_bounded_scan_frames() {
    let records:Vec<_>=(0..100).map(|i|json!({"local_round_id":format!("fallback-{i}"),"stages":if i==90 {vec![stage("initial",0.0,0.35)]} else {Vec::new()},"local_terminal":"completed"})).collect();
    let (mut h, photo) = headless(records);
    // Replace the page's text through the same caller-used persisted conversation interface.
    let records = h.app.session.load_companion_conversation(photo).unwrap().unwrap()["round_records"].clone();
    h.app.session.save_companion_conversation(photo,json!({"schema_version":2,"messages":[{"role":"assistant","text":format!("OWNER_FRAGMENT{}","A".repeat(270_000)),"local_round_id":"fallback-90"}],"round_records":records,"rounds":[]})).unwrap();
    let session = std::mem::take(&mut h.app.session);
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    h.app.run("ai.history", json!({"record":0})).unwrap();
    for _ in 0..200 {
        h.step();
        if h.app.run("ai.status", json!({})).unwrap()["historyLoading"] == false {
            break;
        }
    }
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["resultCards"][0]["localRoundId"], "fallback-90");
}

#[test]
fn fallback_giant_ignored_fields_keep_each_frame_bounded_and_eventually_show_the_bound_card() {
    use lightcraft_catalog::Store;
    use std::{
        io,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    struct Meter {
        files: MemStore,
        bytes: Arc<AtomicUsize>,
    }
    impl Store for Meter {
        fn read(&mut self, n: &str) -> io::Result<Option<Vec<u8>>> {
            let b = self.files.read(n)?;
            self.bytes.fetch_add(b.as_ref().map_or(0, Vec::len), Ordering::Relaxed);
            Ok(b)
        }
        fn read_limited(&mut self, n: &str, max: u64) -> io::Result<Option<Vec<u8>>> {
            let b = self.files.read_limited(n, max)?;
            self.bytes.fetch_add(b.as_ref().map_or(0, Vec::len), Ordering::Relaxed);
            Ok(b)
        }
        fn read_range(&mut self, n: &str, o: u64, l: usize) -> io::Result<Vec<u8>> {
            let b = self.files.read_range(n, o, l)?;
            self.bytes.fetch_add(b.len(), Ordering::Relaxed);
            Ok(b)
        }
        fn write_atomic(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
            self.files.write_atomic(n, b)
        }
        fn append(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
            self.files.append(n, b)
        }
        fn truncate(&mut self, n: &str, l: u64) -> io::Result<()> {
            self.files.truncate(n, l)
        }
        fn describe(&self) -> String {
            self.files.describe()
        }
        fn retention_namespace(&self) -> Option<String> {
            self.files.retention_namespace()
        }
    }
    let bytes = Arc::new(AtomicUsize::new(0));
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(Meter { files: MemStore::new(), bytes: bytes.clone() }),
                dir: "memory".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    session.save_companion_conversation(photo,json!({"schema_version":2,"messages":[{"role":"assistant","text":"A".repeat(6*1024*1024),"local_round_id":"giant-bound"}],"round_records":[{"local_round_id":"giant-bound","request":"x".repeat(24*1024*1024),"stages":[stage("initial",0.0,0.35)],"local_terminal":"completed"}],"rounds":[]})).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    bytes.store(0, Ordering::Relaxed);
    h.app.run("ai.history", json!({"record":0})).unwrap();
    assert!(bytes.swap(0, Ordering::Relaxed) <= 16 * 1024 * 1024, "history command parsed the complete giant ignored field");
    for _ in 0..200 {
        h.step();
        let read = bytes.swap(0, Ordering::Relaxed);
        assert!(read <= 16 * 1024 * 1024, "one frame read {read} bytes");
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historyLoading"] == false {
            assert_eq!(status["resultCards"][0]["localRoundId"], "giant-bound");
            assert_eq!(status["resultCards"][0]["stages"][0]["changes"][0]["actual"], "+0.35");
            return;
        }
    }
    panic!("bounded scan lost continuation");
}
#[test]
fn native_card_projection_failure_preserves_the_already_readable_message_page() {
    let mut files = MemStore::new();
    files.background = true;
    let mut session = Session::new();
    session
        .open_library_in(LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(files), dir: "memory".into(), on_disk: false }, true)
        .unwrap();
    let photo = session.active().unwrap();
    let mut oversized = stage("initial", 0.0, 0.35);
    oversized["reason"] = json!("R".repeat(2 * 1024 * 1024));
    session.save_companion_conversation(photo,json!({"schema_version":2,"messages":[{"role":"assistant","text":"KEEP_VALID_HISTORY_LITERAL","local_round_id":"oversized-card"}],"round_records":[{"local_round_id":"oversized-card","stages":[oversized],"local_terminal":"completed"}],"rounds":[]})).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    h.app.run("ai.history", json!({"record":0})).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historyLoading"] == false {
            let text = status["text"].as_str().unwrap();
            assert!(text.contains("KEEP_VALID_HISTORY_LITERAL"), "valid messages disappeared: {text}");
            assert!(text.contains("display"), "card projection failure must remain explicit: {text}");
            assert_eq!(status["resultCards"], json!([]));
            return;
        }
        assert!(std::time::Instant::now() < deadline, "reader failed to retire");
        std::thread::yield_now();
    }
}

fn sampled_mask(op: lightcraft_develop::MaskOp, invert: bool) -> lightcraft_develop::DevelopSettings {
    let mut before = lightcraft_develop::DevelopSettings::default();
    before.masks.push(lightcraft_develop::Mask {
        id: 7,
        name: "Sampled colors".into(),
        components: vec![lightcraft_develop::MaskComponent {
            name: None,
            op,
            invert,
            shape: lightcraft_develop::MaskShape::ColorRange { samples: vec![[0.5, 0.1, 0.1]], refine: 50.0 },
        }],
        ..Default::default()
    });
    before
}
fn update_sampled_mask(before: &lightcraft_develop::DevelopSettings, sample_a: f64) -> lightcraft_develop::DevelopSettings {
    let context = lightcraft_engine::companion::CandidateContext::new(
        std::sync::Arc::new(lightcraft_raster::Rgb32f::filled(60, 40, [0.2; 3])),
        Default::default(),
        before,
        "current",
    );
    lightcraft_engine::companion::candidate(before, &json!({"operations":[{"type":"update_manual_mask","ref":"mask-7","frame_id":"current","coordinate_space":"native_normalized","shape":{"kind":"colorRange","samples":[[0.5,sample_a,0.1]],"refine":50.0}}]}), &context).unwrap()
}
#[test]
fn supported_color_range_sample_update_displays_the_actual_oklab_values() {
    let before = sampled_mask(lightcraft_develop::MaskOp::Add, false);
    let after = update_sampled_mask(&before, -0.1);
    assert_ne!(before, after, "supported candidate actually changes the sampled color");
    let s = json!({"stage":"initial","before_state":before,"proposed_state":after,"applied_state":after,"locally_published":true,"delivery_state":"confirmed"});
    let (mut h, _) = headless(vec![json!({"local_round_id":"sample-card","stages":[s]})]);
    let result = h.app.run("ai.status", json!({})).unwrap();
    let rows = result["resultCards"][0]["stages"][0]["changes"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["control"] == "mask.7.component.0.samples"
        && r["before"] == "(0.50, 0.10, 0.10)"
        && r["actual"] == "(0.50, -0.10, 0.10)"
        && r["unit"] == "OkLab"));
    for locale in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        h.app.ui.language = locale;
        h.step();
        if locale == crate::i18n::Locale::En {
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiParameters:sample-card"}), std::time::Duration::from_secs(10))["ok"], true);
        }
        for _ in 0..4 {
            h.step();
        }
        assert!(h.painted_text().iter().any(|t| t == "(0.50, 0.10, 0.10) → (0.50, -0.10, 0.10)"));
        screenshot(&mut h, &format!("sample-{}", locale.code()));
    }
}

fn screenshot(h: &mut Headless, name: &str) {
    if let Ok(dir) = std::env::var("UX16_SCREENSHOT_DIR") {
        h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        h.app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        assert_eq!(
            h.request("ui.screenshot", json!({"path":format!("{dir}/{name}.png"),"headless":true}), std::time::Duration::from_secs(30))["ok"],
            true
        );
    }
}
#[test]
fn supported_manual_mask_update_displays_changed_combination_and_inversion() {
    let before = sampled_mask(lightcraft_develop::MaskOp::Subtract, true);
    let after = update_sampled_mask(&before, 0.1);
    assert_eq!(after.masks[0].components[0].op, lightcraft_develop::MaskOp::Add);
    assert!(!after.masks[0].components[0].invert);
    let s = json!({"stage":"initial","before_state":before,"proposed_state":after,"applied_state":after,"locally_published":true,"delivery_state":"confirmed"});
    let (mut h, _) = headless(vec![json!({"local_round_id":"combine-card","stages":[s]})]);
    let result = h.app.run("ai.status", json!({})).unwrap();
    let rows = result["resultCards"][0]["stages"][0]["changes"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["control"] == "mask.7.component.0.op" && r["before"] == "Subtract" && r["actual"] == "Add"));
    assert!(rows.iter().any(|r| r["control"] == "mask.7.component.0.component_invert" && r["before"] == "On" && r["actual"] == "Off"));
    for locale in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        h.app.ui.language = locale;
        h.step();
        if locale == crate::i18n::Locale::En {
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiParameters:combine-card"}), std::time::Duration::from_secs(10))["ok"], true);
        }
        for _ in 0..4 {
            h.step();
        }
        let expected =
            if locale == crate::i18n::Locale::En { ["Subtract → Add", "On → Off"] } else { ["减去 → 添加", "开启 → 关闭"] };
        for literal in expected {
            assert!(h.painted_text().iter().any(|t| t == literal), "{literal}");
        }
        screenshot(&mut h, &format!("combine-{}", locale.code()));
    }
}
#[test]
fn zero_chunk_payload_returns_an_error_and_keeps_valid_history_readable() {
    use lightcraft_engine::companion::storage::{DisplayRecord, DisplayScan, HistoryCursor};
    for background in [false, true] {
        let mut files = MemStore::new();
        files.background = background;
        let mut session = Session::new();
        session
            .open_library_in(
                LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(files.clone()), dir: "memory".into(), on_disk: false },
                true,
            )
            .unwrap();
        let photo = session.active().unwrap();
        session.save_companion_conversation(photo,json!({"schema_version":2,"messages":[{"role":"assistant","text":"VALID_LAYOUT_MESSAGE","local_round_id":"bad-layout"}],"round_records":[{"local_round_id":"bad-layout","stages":[stage("initial",0.0,0.35)]}],"rounds":[]})).unwrap();
        let mut header = session.companion_photo_header(photo).unwrap().unwrap();
        let root = header.record_root.as_mut().unwrap();
        let mut index: Value = serde_json::from_slice(&files.get(&root.key).unwrap()).unwrap();
        index["entries"][0]["chunk_payload"] = json!(0);
        let bytes = serde_json::to_vec(&index).unwrap();
        root.digest = lightcraft_preview::hash_bytes(&bytes).to_string();
        files.set(&root.key, bytes);
        // A checksum-consistent malformed input, published only in the isolated filesystem adapter.
        let head_key = format!("companion/v2/photos/{}/HEAD.json", photo.0);
        let mut head: Value = serde_json::from_slice(&files.get(&head_key).unwrap()).unwrap();
        let manifest = serde_json::to_vec(&header).unwrap();
        files.set(head["manifest"].as_str().unwrap(), manifest.clone());
        head["manifest_digest"] = json!(lightcraft_preview::hash_bytes(&manifest).to_string());
        files.set(&head_key, serde_json::to_vec(&head).unwrap());
        let mut scan = DisplayScan::new(header.clone(), DisplayRecord::Round(0));
        let error = session.companion_display_step(&mut scan).expect_err("invalid layout must error without panicking");
        assert!(error.to_string().contains("invalid chunk layout"));
        assert!(session.companion_rounds_display_at(&header, 0, 32).is_err());
        if let Some(mut reader) = session.companion_reader(&header).unwrap() {
            assert!(reader.rounds_display(0, 32).is_err());
        }
        let page = session.companion_history_slice_at(&header, HistoryCursor::default(), 256 * 1024).unwrap();
        assert_eq!(page.records[0]["text"], "VALID_LAYOUT_MESSAGE");
        let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        let status = ready(&mut h);
        assert!(status["text"].as_str().unwrap().contains("VALID_LAYOUT_MESSAGE"));
        assert!(status["text"].as_str().unwrap().contains("invalid chunk layout"));
        assert_eq!(status["resultCards"], json!([]));
    }
}
#[test]
fn oversized_legacy_parameters_are_explicitly_unavailable_without_losing_the_result() {
    for background in [false, true] {
        let mut before = lightcraft_develop::DevelopSettings::default();
        before.masks.push(lightcraft_develop::Mask {
            id: 7,
            components: vec![lightcraft_develop::MaskComponent {
                name: None,
                op: Default::default(),
                invert: false,
                shape: lightcraft_develop::MaskShape::Brush {
                    strokes: vec![lightcraft_develop::BrushStroke {
                        points: (0..12000).map(|_| lightcraft_geom::Point::new(0.1, 0.2)).collect(),
                        ..Default::default()
                    }],
                },
            }],
            ..Default::default()
        });
        let mut after = before.clone();
        after.light.exposure = 0.35;
        let mut files = MemStore::new();
        files.background = background;
        let mut session = Session::new();
        session
            .open_library_in(LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(files), dir: "memory".into(), on_disk: false }, true)
            .unwrap();
        let photo = session.active().unwrap();
        session.save_companion_conversation(photo,json!({"schema_version":2,"messages":[{"role":"assistant","text":"VALID_LEGACY_MESSAGE","local_round_id":"legacy-detail"}],"round_records":[{"local_round_id":"legacy-detail","local_terminal":"completed","recent_complete_result":after,"stages":[{"stage":"initial","reason":"Brighter","before_state":before,"proposed_state":after,"applied_state":after,"locally_published":true,"delivery_state":"confirmed"}]}],"rounds":[]})).unwrap();
        let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        let status = ready(&mut h);
        assert!(status["text"].as_str().unwrap().contains("VALID_LEGACY_MESSAGE"));
        let stage = &status["resultCards"][0]["stages"][0];
        assert_eq!(stage["status"], "applied");
        assert_eq!(stage["parameterDetailsUnavailable"], true, "missing detail must not pretend to be no parameter changes");
        for locale in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
            h.app.ui.language = locale;
            for _ in 0..4 {
                h.step();
            }
            let expected =
                if locale == crate::i18n::Locale::En { "Historical parameter details are unavailable." } else { "历史参数详情不可用。" };
            assert!(h.painted_text().iter().any(|t| t == expected), "{expected}");
            screenshot(&mut h, &format!("legacy-detail-{background}-{}", locale.code()));
        }
    }
}
#[test]
fn failed_proposal_retained_categorical_value_uses_the_selected_language() {
    let before = lightcraft_develop::DevelopSettings::default();
    let mut proposed = before.clone();
    proposed.crop.flip_h = true;
    let s = json!({"stage":"initial","before_state":before,"proposed_state":proposed,"locally_published":false,"execution_outcome":"failed","delivery_state":"not_sent"});
    let (mut h, _) = headless(vec![json!({"local_round_id":"failed-category","stages":[s],"local_terminal":"failed"})]);
    for locale in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        h.app.ui.language = locale;
        for _ in 0..4 {
            h.step();
        }
        if locale == crate::i18n::Locale::En {
            assert_eq!(
                h.request("ui.clickWidget", json!({"id":"button:aiParameters:failed-category"}), std::time::Duration::from_secs(10))["ok"],
                true
            );
        }
        for _ in 0..4 {
            h.step();
        }
        let expected = if locale == crate::i18n::Locale::En { "Off" } else { "关闭" };
        assert!(h.painted_text().iter().any(|t| t == expected), "retained actual: {expected}");
        if locale == crate::i18n::Locale::ZhHans {
            assert!(!h.painted_text().iter().any(|t| t == "Off"));
        }
        screenshot(&mut h, &format!("failed-category-{}", locale.code()));
    }
}

#[test]
fn actual_fallback_view_retains_its_graph_across_inactive_scans_until_real_retirement() {
    retained_view(false);
}
#[test]
fn actual_native_completion_view_retains_its_graph_until_real_retirement() {
    retained_view(true);
}
fn retained_view(background: bool) {
    use lightcraft_catalog::Store;
    use std::{
        io,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    struct Files {
        memory: MemStore,
        calls: Arc<AtomicUsize>,
        largest: Arc<AtomicUsize>,
        reader: bool,
        native_ranges: Arc<AtomicUsize>,
    }
    impl Store for Files {
        fn read(&mut self, n: &str) -> io::Result<Option<Vec<u8>>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.largest.store(usize::MAX, Ordering::Relaxed);
            self.memory.read(n)
        }
        fn read_limited(&mut self, n: &str, max: u64) -> io::Result<Option<Vec<u8>>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.largest.fetch_max(max as usize, Ordering::Relaxed);
            self.memory.read_limited(n, max)
        }
        fn read_range(&mut self, n: &str, o: u64, len: usize) -> io::Result<Vec<u8>> {
            if self.reader {
                self.native_ranges.fetch_add(1, Ordering::Relaxed);
            }
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.largest.fetch_max(len, Ordering::Relaxed);
            self.memory.read_range(n, o, len)
        }
        fn write_atomic(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.memory.write_atomic(n, b)
        }
        fn append(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
            self.memory.append(n, b)
        }
        fn remove(&mut self, n: &str) -> io::Result<()> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.memory.remove(n)
        }
        fn truncate(&mut self, n: &str, len: u64) -> io::Result<()> {
            self.memory.truncate(n, len)
        }
        fn describe(&self) -> String {
            self.memory.describe()
        }
        fn retention_namespace(&self) -> Option<String> {
            self.memory.retention_namespace()
        }
        fn background_writer(&self) -> Option<Box<dyn Store>> {
            self.memory.background.then(|| {
                Box::new(Self {
                    memory: self.memory.clone(),
                    calls: self.calls.clone(),
                    largest: self.largest.clone(),
                    reader: true,
                    native_ranges: self.native_ranges.clone(),
                }) as Box<dyn Store>
            })
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let largest = Arc::new(AtomicUsize::new(0));
    let native_ranges = Arc::new(AtomicUsize::new(0));
    let mut backing = MemStore::new();
    backing.background = background;
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(Files {
                    memory: backing.clone(),
                    calls: calls.clone(),
                    largest: largest.clone(),
                    reader: false,
                    native_ranges: native_ranges.clone(),
                }),
                dir: "view-retention".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let other = session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    session
        .set_companion_editor(other, lightcraft_engine::companion::EditorState { draft: "independent editor draft".into(), ..Default::default() })
        .unwrap();
    let rounds: Vec<_> = (0..5).map(|i| json!({"local_round_id":format!("retained-{i}"),"request":"ignored".repeat(100_000),"stages":if i==4 {vec![stage("initial",0.0,0.35)]} else {Vec::new()},"local_terminal":"completed"})).collect();
    session.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"role":"assistant","text":"A".repeat(600_000),"local_round_id":"retained-4"}],"round_records":rounds,"rounds":[]})).unwrap();
    let old = session.companion_photo_header(photo).unwrap().unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    h.app.run("ai.history", json!({"record":0})).unwrap();
    if background {
        let status = ready(&mut h);
        assert_eq!(status["resultCards"][0]["localRoundId"], "retained-4");
        assert!(native_ranges.load(Ordering::Relaxed) > 0, "actual background handle read the captured graph");
    } else {
        h.step();
        assert_eq!(h.app.run("ai.status", json!({})).unwrap()["historyLoading"], true);
    }
    h.app.run("library.select", json!({"ids":[other.0],"active":other.0})).unwrap();
    h.step(); // real inactive HistoryView owns its unfinished scanner
    h.app
        .session
        .save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"role":"assistant","text":"current saved view"}],"rounds":[]}))
        .unwrap();
    let current = h.app.session.companion_photo_header(photo).unwrap().unwrap();
    h.app.session.pin_companion_prefix(photo, None).unwrap();
    let mut idle = false;
    for _ in 0..1000 {
        calls.store(0, Ordering::Relaxed);
        largest.store(0, Ordering::Relaxed);
        h.app.session.persist().unwrap();
        assert!(calls.load(Ordering::Relaxed) <= 100);
        assert!(largest.load(Ordering::Relaxed) <= 32 * 1024);
        if calls.load(Ordering::Relaxed) == 0 {
            idle = true;
            break;
        }
    }
    assert!(idle, "protected automatic cycle reached observed zero-I/O idle");
    assert!(
        h.app.session.companion_messages_bounded(&old, 0, 1, 1024 * 1024).is_ok(),
        "actual paused fallback view must protect its captured graph after cache release"
    );
    h.app.run("library.select", json!({"ids":[photo.0],"active":photo.0})).unwrap();
    let status = ready(&mut h);
    assert_eq!(status["resultCards"][0]["localRoundId"], "retained-4");
    assert_eq!(status["resultCards"][0]["stages"][0]["changes"][0]["actual"], "+0.35");
    assert_eq!(h.app.session.companion_editor(other).unwrap().draft, "independent editor draft");
    // A real library-generation change retires the actual view and scanner.
    h.app
        .session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "other-library".into(), on_disk: false },
            true,
        )
        .unwrap();
    h.step();
    // Reopening A resumes its transferred work, without touching its HEAD.
    h.app
        .session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(Files {
                    memory: backing,
                    calls: calls.clone(),
                    largest: largest.clone(),
                    reader: false,
                    native_ranges: native_ranges.clone(),
                }),
                dir: "view-retention".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let mut reclaimed = false;
    for _ in 0..1000 {
        calls.store(0, Ordering::Relaxed);
        largest.store(0, Ordering::Relaxed);
        h.app.session.persist().unwrap();
        assert!(calls.load(Ordering::Relaxed) <= 100);
        assert!(largest.load(Ordering::Relaxed) <= 32 * 1024);
        assert_eq!(h.app.session.companion_messages_bounded(&current, 0, 1, 1024).unwrap().records[0]["text"], "current saved view");
        if h.app.session.companion_messages_bounded(&old, 0, 1, 1024 * 1024).is_err() {
            reclaimed = true;
            break;
        }
    }
    assert!(reclaimed, "actual view retirement must permit eventual automatic reclamation");
    for _ in 0..1000 {
        calls.store(0, Ordering::Relaxed);
        h.app.session.persist().unwrap();
        if calls.load(Ordering::Relaxed) == 0 {
            break;
        }
    }
    assert!(!h.app.session.companion_cleanup_pending());
    assert!(h.app.session.unsaved().is_none());
    assert_eq!(h.app.session.companion_editor(other).unwrap().draft, "independent editor draft");
}
