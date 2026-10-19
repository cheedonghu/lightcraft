use super::*;
use crate::{Services, headless::Headless};
use lightcraft_engine::Session;

fn frames(h: &mut Headless) {
    for _ in 0..5 {
        h.step();
    }
}
fn click(h: &mut Headless, id: &str) {
    let reply = h.request("ui.clickWidget", json!({"id":id}), Duration::from_secs(10));
    assert_eq!(reply["ok"], true, "{id}: {reply}");
    frames(h);
}
fn has(h: &Headless, id: &str) -> bool {
    h.app.widgets.iter().any(|(key, _)| key == id)
}
pub(super) fn screenshots(h: &mut Headless, name: &str) {
    if let Ok(dir) = std::env::var("UX23_SCREENSHOT_DIR") {
        h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        h.app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        let shot = h.request("ui.screenshot", json!({"path":format!("{dir}/{name}.png"),"headless":true}), Duration::from_secs(10));
        assert_eq!(shot["ok"], true, "{shot}");
    }
}

#[test]
fn welcome_prompts_only_fill_drafts_empty_send_is_disabled_and_diagnostics_live_in_settings() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
        app.ui.language = language;
        let (connection, requests, _events) = network::test_connection();
        app.ai.connection = Some(connection);
        app.ai.generation = app.session.library_generation();
        let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        frames(&mut h);
        assert!(has(&h, "ai:welcome"));
        assert!(h.app.ai.prompt.is_empty(), "a new conversation must start with an empty draft");
        assert!(!h.painted_text().iter().any(|text| text == "Ready for your request" || text == "可以输入要求"));
        assert!(!has(&h, "button:aiCopyTrace") && !has(&h, "button:aiHealthRetry"));
        screenshots(&mut h, &format!("welcome-{}", language.code()));
        click(&mut h, "button:aiStart");
        assert!(requests.try_recv().is_err(), "empty arrow must not submit");
        click(&mut h, "button:aiReviewPrompt");
        assert_eq!(h.app.ai.prompt, if language == crate::i18n::Locale::ZhHans { "请点评这张照片。" } else { "Please review this photo." });
        assert!(requests.try_recv().is_err(), "quick prompt must only fill a draft");
        click(&mut h, "button:aiOptimizePrompt");
        assert_eq!(
            h.app.ai.prompt,
            if language == crate::i18n::Locale::ZhHans { "请自然优化这张照片。" } else { "Please naturally optimize this photo." }
        );
        let input = h.app.widgets.iter().find(|(id, _)| id == "field:aiPrompt").unwrap().1;
        click(&mut h, "button:aiSettings");
        assert!(has(&h, "button:aiHealthRetry"));
        assert_eq!(h.app.widgets.iter().find(|(id, _)| id == "field:aiPrompt").unwrap().1, input);
        screenshots(&mut h, &format!("settings-{}", language.code()));
        h.app.ai.settings_open = false;
        frames(&mut h);
        click(&mut h, "button:aiStart");
        assert_eq!(requests.try_recv().unwrap().path, "/v1/rounds");
        assert!(has(&h, "button:aiStop") && !has(&h, "button:aiStart") && !has(&h, "ai:welcome"));
    }
}

#[test]
fn live_process_expands_then_auto_collapses_preserving_final_answer_and_reopening_history() {
    let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
    app.ui.language = crate::i18n::Locale::ZhHans;
    let (connection, requests, events) = network::test_connection();
    app.ai.connection = Some(connection);
    app.ai.generation = app.session.library_generation();
    let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    h.app.run("ai.start", json!({"prompt":"点评这张照片"})).unwrap();
    let create = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    let photo = h.app.session.active().unwrap();
    let local = h.app.ai.jobs.get(&photo).unwrap().identity.local_round_id.as_str().to_owned();
    events.send(network::Event::Server { kind:"round.state".into(), data:json!({"round_id":"design-round","payload":{"submission_seq":create.submission,"photo_ref":create.body["photo_ref"],"state":"queued"}}) }).unwrap();
    for (id, text) in [("interim", "正在检查照片的光线与构图。"), ("final", "画面中的光线自然，建议保留明暗层次。\n- 主体清晰\n- 色彩协调")]
    {
        events
            .send(network::Event::Server {
                kind: "text.delta".into(),
                data: json!({"round_id":"design-round","payload":{"message_id":id,"delta":text}}),
            })
            .unwrap();
        events
            .send(network::Event::Server {
                kind: "message.completed".into(),
                data: json!({"round_id":"design-round","stage":"initial","payload":{"message_id":id,"tool_calls":[]}}),
            })
            .unwrap();
    }
    frames(&mut h);
    assert!(h.painted_text().join("\n").contains("正在检查照片"));
    screenshots(&mut h, "running-zh-hans");
    events
        .send(network::Event::Server {
            kind: "round.ended".into(),
            data: json!({"round_id":"design-round","trace_id":"ui-design-test","payload":{"terminal_state":"completed"}}),
        })
        .unwrap();
    frames(&mut h);
    let text = h.painted_text().join("\n");
    assert!(!text.contains("正在检查照片"), "completed process is collapsed: {text}");
    assert!(text.contains("建议保留明暗层次"), "final answer stays visible: {text}");
    assert!(!has(&h, "button:aiCopyTrace"));
    screenshots(&mut h, "completed-zh-hans");
    let before = h.app.ai.conversations.get(&photo).unwrap().messages.clone();
    click(&mut h, &format!("button:aiProcess:{local}"));
    assert!(h.painted_text().join("\n").contains("正在检查照片"));
    assert_eq!(h.app.ai.conversations.get(&photo).unwrap().messages, before);
    click(&mut h, &format!("button:aiProcess:{local}"));
    assert!(!h.painted_text().join("\n").contains("正在检查照片"));
}

#[test]
fn cold_request_only_failure_keeps_its_explanation_visible_without_a_result_card() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "ui-request-only".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let message = json!({"role":"user","text":"Review this photo","local_round_id":"request-only","local_message_id":"u"});
    let notice = "This AI task could not finish. Start a new request from the current photo.\nThe photo remains at this round's starting settings.";
    let record = json!({"local_round_id":"request-only","photo_id":photo.0,"stages":[],"local_terminal":"failed","final_message_id":null,"terminal_notice":notice});
    session.save_companion_tail(photo, None, &[message], &[], &[record], &json!({"schema_version":2})).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !h.painted_text().join("\n").contains("starting settings") {
        h.step();
        assert!(Instant::now() < deadline, "saved request-only failure disappeared");
        std::thread::yield_now();
    }
    assert!(!has(&h, "button:aiProcess:request-only"), "an empty process needs no expander");
    assert!(!has(&h, "button:aiCopyTrace"));
    screenshots(&mut h, "failed-request-only");
}

#[test]
fn cold_paginated_process_uses_saved_final_identity_and_never_labels_a_fragment_as_conclusion() {
    for explicit_identity in [true, false] {
        use lightcraft_catalog::MemStore;
        use lightcraft_engine::library::LibraryStores;
        let mut session = Session::new();
        session
            .open_library_in(
                LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "ui-redesign".into(), on_disk: false },
                true,
            )
            .unwrap();
        let photo = session.active().unwrap();
        let messages = vec![
            json!({"role":"user","text":"My request","local_round_id":"r","local_message_id":"u"}),
            json!({"role":"assistant","text":"INTERMEDIATE_ONLY ".repeat(20000),"local_round_id":"r","local_message_id":"i"}),
            json!({"role":"assistant","text":"SAVED_FINAL_ANSWER","local_round_id":"r","local_message_id":"f"}),
        ];
        let record = json!({"local_round_id":"r","photo_id":photo.0,"stages":[],"local_terminal":"completed","final_message_id":"f","terminal_notice":"This round is complete."});
        let mut record = record;
        record["large_legacy_payload"] = json!("x".repeat(300_000));
        if !explicit_identity {
            record.as_object_mut().unwrap().remove("final_message_id");
        }
        session.save_companion_tail(photo, None, &messages, &[], &[record], &json!({"schema_version":2})).unwrap();
        let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        h.app.run("ai.history", json!({"record":0,"offset":0})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.app.ai.conversations.get(&photo).and_then(|c| c.view.as_ref()).is_none_or(|v| v.process.is_empty()) {
            h.step();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        // A current tail is not contiguous with this older, still-paginated round.
        h.app
            .ai
            .conversations
            .get_mut(&photo)
            .unwrap()
            .messages
            .push(json!({"role":"user","text":"Current tail request","local_round_id":"new-tail","local_message_id":"new-u"}));
        frames(&mut h);
        assert!(!h.painted_text().join("\n").contains("INTERMEDIATE_ONLY"));
        assert!(!h.painted_text().join("\n").contains("SAVED_FINAL_ANSWER"));
        click(&mut h, "button:aiProcess:r");
        assert!(h.painted_text().join("\n").contains("INTERMEDIATE_ONLY"));
        h.app.run("ai.history", json!({"record":2,"offset":0})).unwrap();
        while !h.painted_text().join("\n").contains("SAVED_FINAL_ANSWER") {
            h.step();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(h.app.session.companion_messages_page(photo, 2, 1).unwrap().records[0], messages[2]);
    }
}
