use super::*;
use crate::{Services, docking::Panel, headless::Headless, state::ViewMode};
use lightcraft_engine::Session;
use std::time::Duration;

fn headless(size: [f32; 2]) -> Headless {
    let app = LightcraftApp::new(Session::with_demo(), Services { png: None, ..Default::default() });
    Headless::new(app, size, 1.0)
}
fn rect(h: &Headless, name: &str) -> egui::Rect {
    h.app.widgets.iter().find(|(id, _)| id == name).unwrap_or_else(|| panic!("missing {name}")).1
}
fn frames(h: &mut Headless) {
    for _ in 0..4 {
        h.step();
    }
}
#[test]
fn ai_menu_opens_shared_tools_idempotently_without_changing_grid() {
    let mut h = headless([1400.0, 900.0]);
    h.app.ui.view = ViewMode::SquareGrid;
    h.app.run("panel.close", json!({})).unwrap();
    for _ in 0..2 {
        h.app.run("ai.open", json!({})).unwrap();
        frames(&mut h);
        assert_eq!(serde_json::to_value(h.app.ui.right).unwrap(), "ai");
        assert_eq!(h.app.ui.view, ViewMode::SquareGrid);
        assert!(h.app.ui.docking.as_ref().unwrap().exposed(Panel::Tools));
        let tools = rect(&h, "panel:right_panel");
        assert!(tools.contains_rect(rect(&h, "field:aiPrompt")));
    }
    h.app.run("panel.close", json!({})).unwrap();
    frames(&mut h);
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "field:aiPrompt"));
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    rect(&h, "field:aiPrompt");
}
#[test]
fn ai_menu_reveals_custom_collapsed_tools_with_no_selection() {
    let mut h = headless([1400.0, 900.0]);
    use craft_ui::docking::{Layout, Node, StackEntry};
    use craft_ui::layout::{SplitAxis, SplitSize};
    h.app.ui.docking = Some(crate::docking::Workspace {
        layout: Layout {
            root: Some(Node::Split {
                axis: SplitAxis::Horizontal,
                size: SplitSize::FixedSecond(350.0),
                first: Box::new(Node::Tabs { panels: vec![Panel::Canvas], active: 0 }),
                second: Box::new(Node::Stack {
                    entries: vec![
                        StackEntry { panel: Panel::Tools, open: false, height: Some(500.0) },
                        StackEntry { panel: Panel::Library, open: true, height: Some(100.0) },
                    ],
                }),
            }),
            floating: Vec::new(),
        },
        hidden: Vec::new(),
    });
    h.app.session.execute("library.select", &json!({"ids":[]})).unwrap();
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    assert!(h.app.ui.docking.as_ref().unwrap().exposed(Panel::Tools));
    rect(&h, "ai:selectionNotice");
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "field:aiPrompt"));
}
#[test]
fn multi_selection_explains_empty_state_before_any_composer() {
    let mut h = headless([1100.0, 750.0]);
    let ids: Vec<_> = h.app.session.catalog.photos().take(2).map(|p| p.id.0).collect();
    h.app.session.execute("library.select", &json!({"ids":ids})).unwrap();
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    rect(&h, "ai:selectionNotice");
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "field:aiPrompt"));
}
#[test]
fn long_messages_scroll_without_moving_the_stage_and_composer() {
    let mut h = headless([1000.0, 680.0]);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    let photo = h.app.session.active().unwrap();
    let input = rect(&h, "field:aiPrompt");
    let stage = rect(&h, "ai:stage");
    h.app.ai.conversations.entry(photo).or_default().messages =
        (0..200).map(|i| json!({"role":"assistant","text":format!("Message {i}: a longer explanation of this photograph.")})).collect();
    frames(&mut h);
    assert_eq!(rect(&h, "field:aiPrompt"), input);
    assert_eq!(rect(&h, "ai:stage"), stage);
    let messages = rect(&h, "ai:messages");
    assert!(messages.bottom() <= stage.top());
    assert!(rect(&h, "panel:right_panel").contains_rect(input));
    let before_scroll = h.paint();
    h.app.synthetic.push(egui::Event::PointerMoved(messages.center()));
    h.app.synthetic.push(egui::Event::MouseWheel {
        phase: egui::TouchPhase::Move,
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 180.0),
        modifiers: egui::Modifiers::NONE,
    });
    frames(&mut h);
    assert_eq!(rect(&h, "field:aiPrompt"), input);
    assert_eq!(rect(&h, "ai:stage"), stage);
    let after_scroll = h.paint();
    assert!(
        (messages.top() as usize..messages.bottom() as usize).any(|y| (messages.left() as usize..messages.right() as usize)
            .any(|x| before_scroll.pixels[y * before_scroll.size[0] + x] != after_scroll.pixels[y * after_scroll.size[0] + x])),
        "the real message viewport scrolls while footer stays fixed"
    );
}

#[test]
fn real_consent_start_revoke_and_strip_collapse_work_in_both_languages() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let mut h = headless([1000.0, 680.0]);
        h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        h.app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        h.app.ui.language = language;
        h.app.ui.settings.gpu = false;
        let (connection, requests, _events) = network::test_connection();
        h.app.ai.connection = Some(connection);
        h.app.ai.generation = h.app.session.library_generation();
        h.app.run("ai.open", json!({})).unwrap();
        frames(&mut h);
        let timeout = Duration::from_secs(10);
        assert!(h.painted_text().iter().any(|text| text == language.tr("AI Assistant")));
        assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
        assert!(h.app.ai.consent);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiStart"}), timeout)["ok"], true);
        assert_eq!(requests.try_recv().unwrap().path, "/v1/rounds");
        assert!(requests.try_recv().is_err(), "measurement cannot submit another request");
        let photo = h.app.session.active().unwrap();
        let cancel = h.app.ai.jobs.get(&photo).unwrap().cancel.clone();
        assert!(h.app.ui.docking.as_ref().unwrap().exposed(Panel::Tools));
        rect(&h, "button:aiStop");
        let checkbox = rect(&h, "checkbox:aiConsent");
        assert!(rect(&h, "panel:right_panel").contains_rect(checkbox));
        if let Ok(dir) = std::env::var("UX06_SCREENSHOT_DIR") {
            let shot = h.request("ui.screenshot", json!({"path":format!("{dir}/active-{}.png",language.code()),"headless":true}), timeout);
            assert_eq!(shot["ok"], true, "{shot}");
            let inspect = h.request("ui.inspect", json!({}), timeout);
            assert_eq!(inspect["ok"], true);
            std::fs::write(format!("{dir}/inspect-{}.json", language.code()), serde_json::to_vec_pretty(&inspect).unwrap()).unwrap();
        }
        assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
        assert!(!h.app.ai.consent);
        assert!(cancel.load(Ordering::Relaxed));
        h.app.ui.presets = true;
        frames(&mut h);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiCollapse"}), timeout)["ok"], true);
        assert!(h.app.ui.presets, "collapsing AI only closes Tools");
        frames(&mut h);
        assert!(!h.app.widgets.iter().any(|(id, _)| id == "field:aiPrompt"));
        assert_eq!(h.request("ui.clickWidget", json!({"id":"icon:ai"}), timeout)["ok"], true);
        rect(&h, "field:aiPrompt");
    }
}

#[test]
fn narrow_shared_panel_keeps_all_composer_controls_inside_tools() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let mut h = headless([880.0, 600.0]);
        h.app.ui.language = language;
        h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        h.app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        h.app.run("ai.open", json!({})).unwrap();
        frames(&mut h);
        let tools = rect(&h, "panel:right_panel");
        for id in ["ai:stage", "field:aiPrompt", "checkbox:aiConsent", "button:aiStart", "button:aiUndo", "button:aiClear"] {
            assert!(tools.contains_rect(rect(&h, id)), "{id} in {language:?}: {:?} within {tools:?}", rect(&h, id));
        }
        if let Ok(dir) = std::env::var("UX06_SCREENSHOT_DIR") {
            let shot =
                h.request("ui.screenshot", json!({"path":format!("{dir}/narrow-{}.png",language.code()),"headless":true}), Duration::from_secs(20));
            assert_eq!(shot["ok"], true, "{shot}");
        }
    }
}

#[test]
fn queued_stage_is_truthful_and_checkbox_revocation_saves_its_terminal_record() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let (mut app, state, photo, requests, events) = super::identity_tests::started();
        let identity = state.jobs.get(&photo).unwrap().identity.clone();
        app.ai = state;
        app.ui.language = language;
        app.run("ai.open", json!({})).unwrap();
        let mut h = Headless::new(app, [1000.0, 680.0], 1.0);
        events
            .try_send(network::Event::Server {
                kind: "round.state".into(),
                data: json!({"round_id":"backend-round", "payload":{
            "submission_seq":identity.submission_seq,
            "photo_ref":{"photo_id":photo.0,"library_generation":identity.scope.library_generation}, "state":"queued"}}),
            })
            .unwrap();
        frames(&mut h);
        let expected = if language == crate::i18n::Locale::En { "Waiting in the queue…" } else { "排队等待处理…" };
        assert!(h.painted_text().iter().any(|text| text == expected), "phase must reflect actual queued state");
        let response = h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), Duration::from_secs(10));
        assert_eq!(response["ok"], true);
        assert!(!h.app.ai.consent);
        assert!(requests.try_recv().unwrap().path.ends_with("/stop"));
        let archive = h.app.session.photo_companion(photo).unwrap().unwrap();
        assert_eq!(archive["round_records"][0]["local_terminal"], "cancelled", "checkbox cancellation follows the normal durable stop path");
    }
}

#[test]
fn cancellation_save_waits_for_a_completed_feedback_frame() {
    let (mut app, mut state, photo, requests, _) = super::identity_tests::started();
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    let ctx = egui::Context::default();
    state
        .server(
            &mut app,
            &ctx,
            "round.state",
            &json!({"round_id":"backend-round", "payload":{
        "submission_seq":identity.submission_seq,
        "photo_ref":{"photo_id":photo.0,"library_generation":identity.scope.library_generation},"state":"queued"}}),
        )
        .unwrap();
    let before = app.session.photo_companion(photo).unwrap().unwrap().clone();
    state.defer_local_saves_for_feedback(&ctx);
    state.set_consent(false).unwrap();
    assert!(!state.consent);
    assert!(state.jobs.is_empty());
    assert!(requests.try_recv().unwrap().path.ends_with("/stop"));
    state.poll_inner(&mut app, &ctx);
    assert_eq!(app.session.photo_companion(photo).unwrap().unwrap(), &before, "a same-frame poll/discard cannot write before feedback paint");
    assert!(state.pending_local_saves.contains(&photo));
    let mut frame = ctx.run_ui(egui::RawInput::default(), |_| {});
    frame.textures_delta.clear();
    state.poll_inner(&mut app, &ctx);
    assert_eq!(app.session.photo_companion(photo).unwrap().unwrap()["round_records"][0]["local_terminal"], "cancelled");
    assert!(state.pending_local_saves.is_empty());
}

#[test]
fn multiline_draft_scrolls_inside_a_fixed_composer_without_sizing_consuming_input() {
    let mut h = headless([880.0, 600.0]);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    let input = rect(&h, "field:aiPrompt");
    let stage = rect(&h, "ai:stage");
    let timeout = Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout)["ok"], true);
    let draft = "Please improve this photograph naturally.\n".repeat(50);
    assert_eq!(h.request("ui.text", json!({"text":draft}), timeout)["ok"], true);
    assert_eq!(h.app.ai.prompt, draft);
    frames(&mut h);
    assert_eq!(rect(&h, "field:aiPrompt"), input);
    assert_eq!(rect(&h, "ai:stage"), stage);
    let tools = rect(&h, "panel:right_panel");
    for id in ["field:aiPrompt", "checkbox:aiConsent", "button:aiStart", "button:aiUndo", "button:aiClear"] {
        assert_eq!(h.app.widgets.iter().filter(|(key, _)| key == id).count(), 1, "only the visible pass registers {id}");
        assert!(tools.contains_rect(rect(&h, id)));
    }
}
