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
        for id in ["ai:stage", "field:aiPrompt", "checkbox:aiConsent", "button:aiStart", "button:aiClear"] {
            assert!(tools.contains_rect(rect(&h, id)), "{id} in {language:?}: {:?} within {tools:?}", rect(&h, id));
        }
        assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiUndo"), "Undo belongs to an explicit result card, not the composer");
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
    for id in ["field:aiPrompt", "checkbox:aiConsent", "button:aiStart", "button:aiClear"] {
        assert_eq!(h.app.widgets.iter().filter(|(key, _)| key == id).count(), 1, "only the visible pass registers {id}");
        assert!(tools.contains_rect(rect(&h, id)));
    }
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiUndo"), "a draft alone never supplies an implicit round target");
}

pub(super) fn short_tools(h: &mut Headless, stacked: bool, height: f32) {
    if stacked {
        frames(h);
        // Stack heights are relative weights. Keep the companion body at the
        // requested height while the library consumes the remaining stack.
        let rest = (rect(h, "panel:right_panel").height() - 26.0 - height).max(1.0);
        h.app.ui.left_panel = true;
        use craft_ui::docking::{Layout, Node, StackEntry};
        use craft_ui::layout::{SplitAxis, SplitSize};
        h.app.ui.docking = Some(crate::docking::Workspace {
            layout: Layout {
                root: Some(Node::Split {
                    axis: SplitAxis::Horizontal,
                    size: SplitSize::FixedSecond(320.0),
                    first: Box::new(Node::Tabs { panels: vec![Panel::Canvas], active: 0 }),
                    second: Box::new(Node::Stack {
                        entries: vec![
                            StackEntry { panel: Panel::Tools, open: true, height: Some(height) },
                            StackEntry { panel: Panel::Library, open: true, height: Some(rest) },
                        ],
                    }),
                }),
                floating: Vec::new(),
            },
            hidden: Vec::new(),
        });
    } else {
        h.app.run("ui.dock", json!({"operation":"float","panel":"tools","rect":[450,100,320,height]})).unwrap();
    }
    h.app.run("ai.open", json!({})).unwrap();
    frames(h);
    assert!(rect(h, "panel:right_panel").height() <= height + 1.0, "the supported Tools pane actually stays short");
}

pub(super) fn reachable(h: &mut Headless, id: &str, direction: f32) -> egui::Rect {
    for _ in 0..30 {
        if let Some((_, target)) = h.app.widgets.iter().find(|(key, _)| key == id)
            && target.height() >= 12.0
        {
            assert!(rect(h, "panel:right_panel").contains_rect(*target), "{id} must be visibly inside Tools");
            return *target;
        }
        let composer = rect(h, "ai:composer");
        h.app.synthetic.push(egui::Event::PointerMoved(egui::pos2(composer.right() - 2.0, composer.center().y)));
        h.app.synthetic.push(egui::Event::MouseWheel {
            phase: egui::TouchPhase::Move,
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, direction * 35.0),
            modifiers: egui::Modifiers::NONE,
        });
        frames(h);
    }
    panic!("{id} never reachable in short Tools");
}

#[test]
fn supported_short_floating_and_stacked_tools_keep_active_controls_on_their_own_surface() {
    for (stacked, height) in [(false, 200.0), (true, 200.0), (false, 120.0), (true, 120.0)] {
        for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
            let mut h = headless([1000.0, 680.0]);
            h.app.ui.language = language;
            let (connection, requests, _events) = network::test_connection();
            h.app.ai.connection = Some(connection);
            h.app.ai.generation = h.app.session.library_generation();
            h.app.run("ai.open", json!({})).unwrap();
            frames(&mut h);
            let timeout = Duration::from_secs(10);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiStart"}), timeout)["ok"], true);
            assert_eq!(requests.try_recv().unwrap().path, "/v1/rounds");
            let photo = h.app.session.active().unwrap();
            let cancel = h.app.ai.jobs.get(&photo).unwrap().cancel.clone();
            short_tools(&mut h, stacked, height);
            let zoom = h.app.ui.zoom;
            let tools = rect(&h, "panel:right_panel");
            for (id, target) in h.app.widgets.iter().filter(|(id, _)| {
                id != "button:aiTaskEntry" && (id.starts_with("button:ai") || id.starts_with("checkbox:ai") || id == "field:aiPrompt")
            }) {
                assert!(tools.contains_rect(*target), "{id} clips outside {tools:?}: {target:?}");
            }
            let global_entry = rect(&h, "button:aiTaskEntry");
            assert!(!global_entry.intersects(tools), "global task entry remains outside short Tools");
            reachable(&mut h, "checkbox:aiConsent", -1.0);
            short_snapshot(&mut h, &format!("short-active-{}-{}-{}", if stacked { "stack" } else { "float" }, height as usize, language.code()));
            assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
            assert!(!h.app.ai.consent);
            assert!(cancel.load(Ordering::Relaxed));
            assert_eq!(h.app.ui.zoom, zoom, "AI consent cannot click/zoom the photo behind its pane");
            reachable(&mut h, "checkbox:aiConsent", 1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
            assert!(h.app.ai.consent);
            assert!(cancel.load(Ordering::Relaxed), "re-consent never revives the stopped job");
            reachable(&mut h, "button:aiStop", 1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiStop"}), timeout)["ok"], true);
            assert_eq!(h.app.ui.zoom, zoom);
        }
    }
}

#[test]
fn supported_short_tools_allow_idle_draft_clear_and_start_through_visible_widgets() {
    for (stacked, height) in [(false, 240.0), (true, 240.0), (false, 120.0), (true, 120.0)] {
        for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
            let mut h = headless([1000.0, 680.0]);
            h.app.ui.language = language;
            let photo = h.app.session.active().unwrap();
            h.app
                .session
                .set_photo_companion(photo, Some(json!({"schema_version":1,"messages":[{"role":"user","text":"kept legacy text"}],"rounds":[]})))
                .unwrap();
            let (connection, requests, _events) = network::test_connection();
            h.app.ai.connection = Some(connection);
            h.app.ai.generation = h.app.session.library_generation();
            short_tools(&mut h, stacked, height);
            let timeout = Duration::from_secs(10);
            reachable(&mut h, "field:aiPrompt", -1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout)["ok"], true);
            assert_eq!(h.request("ui.text", json!({"text":"Please keep natural colors."}), timeout)["ok"], true);
            assert_eq!(h.app.ai.prompt, "Please keep natural colors.");
            let zoom = h.app.ui.zoom;
            reachable(&mut h, "button:aiClear", -1.0);
            short_snapshot(&mut h, &format!("short-idle-{}-{}-{}", if stacked { "stack" } else { "float" }, height as usize, language.code()));
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClear"}), timeout)["ok"], true);
            assert!(h.app.session.photo_companion(photo).unwrap().is_none());
            assert_eq!(h.app.ai.prompt, "Please keep natural colors.");
            reachable(&mut h, "checkbox:aiConsent", 1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
            reachable(&mut h, "button:aiStart", -1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiStart"}), timeout)["ok"], true);
            assert_eq!(requests.try_recv().unwrap().path, "/v1/rounds");
            assert_eq!(h.app.session.companion_task_photo(), Some(photo));
            assert_eq!(h.app.ui.zoom, zoom);
        }
    }
}

pub(super) fn short_snapshot(h: &mut Headless, name: &str) {
    if let Ok(dir) = std::env::var("UX06_SCREENSHOT_DIR") {
        h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        h.app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        assert_eq!(h.request("ui.screenshot", json!({"path":format!("{dir}/{name}.png"),"headless":true}), Duration::from_secs(20))["ok"], true);
    }
}

#[test]
fn drafts_follow_the_actual_selected_photo_and_survive_tool_switches() {
    let mut h = headless([1000.0, 680.0]);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    let a = h.app.session.active().unwrap().0;
    let b = h.app.session.catalog.photos().find(|p| p.id.0 != a).unwrap().id.0;
    let ids = [a, b];
    let timeout = Duration::from_secs(10);
    h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
    h.request("ui.text", json!({"text":"A unsent draft"}), timeout);
    assert!(h.painted_text().iter().any(|text| text == "A unsent draft"));
    h.app.run("library.select", json!({"ids":[ids[1]],"active":ids[1]})).unwrap();
    frames(&mut h);
    assert!(!h.painted_text().iter().any(|text| text == "A unsent draft"), "B must not display A's unsent draft");
    h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
    h.request("ui.text", json!({"text":"B draft 草稿"}), timeout);
    h.app.run("library.select", json!({"ids":[ids[0]],"active":ids[0]})).unwrap();
    frames(&mut h);
    assert!(h.painted_text().iter().any(|text| text == "A unsent draft"));
    h.app.run("panel.edit", json!({})).unwrap();
    frames(&mut h);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    assert!(h.painted_text().iter().any(|text| text == "A unsent draft"));
    for id in ids {
        assert!(h.app.session.load_companion_conversation(PhotoId(id)).unwrap().is_none(), "unsent drafts are never messages");
    }
}

#[test]
fn real_reading_offset_is_photo_scoped_and_user_scroll_stops_tail_following() {
    let mut session = Session::new();
    session
        .open_library_in(
            lightcraft_engine::library::LibraryStores {
                catalog: Box::new(lightcraft_catalog::MemStore::new()),
                files: Box::new(lightcraft_catalog::MemStore::new()),
                dir: "memory".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let a = session.active().unwrap();
    let b = session.catalog.photos().find(|p| p.id != a).unwrap().id;
    let messages: Vec<_> = (0..100)
        .map(
            |i| json!({"role":"assistant","text":format!("Paragraph {i}: real reading position must stay put when the assistant appends new text.")}),
        )
        .collect();
    session.save_companion_conversation(a, json!({"schema_version":2,"messages":messages,"rounds":[]})).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    let timeout = Duration::from_secs(10);
    let (connection, requests, events) = network::test_connection();
    h.app.ai.next_connection = Some(connection);
    h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout);
    h.request("ui.clickWidget", json!({"id":"button:aiStart"}), timeout);
    let creation = native_history_tests::outgoing(&mut h, &requests, "/v1/rounds");
    events.try_send(network::Event::Server { kind:"round.state".into(), data:json!({"round_id":"reading-round","payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"photo_id":a.0,"library_generation":creation.identity.scope.library_generation},"state":"queued"}}) }).unwrap();
    frames(&mut h);
    let messages = rect(&h, "ai:messages");
    h.request("ui.move", json!({"x":messages.center().x,"y":messages.center().y}), timeout);
    h.request("ui.scroll", json!({"dy":180.0}), timeout);
    for _ in 0..20 {
        h.step();
    }
    let before = h.app.session.companion_editor(a).unwrap().scroll_offset;
    assert!(before > 0.0);
    assert!(!h.app.session.companion_editor(a).unwrap().follow_tail);
    events
        .try_send(network::Event::Server {
            kind: "text.delta".into(),
            data: json!({"round_id":"reading-round","payload":{"message_id":"read-message","delta":"new tail text"}}),
        })
        .unwrap();
    frames(&mut h);
    assert!((h.app.session.companion_editor(a).unwrap().scroll_offset - before).abs() < 2.0);
    h.app.run("panel.edit", json!({})).unwrap();
    frames(&mut h);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    assert!((h.app.session.companion_editor(a).unwrap().scroll_offset - before).abs() < 2.0);
    h.app.run("library.select", json!({"ids":[b.0],"active":b.0})).unwrap();
    frames(&mut h);
    assert_eq!(h.app.session.companion_editor(b).unwrap().scroll_offset, 0.0);
    h.app.run("library.select", json!({"ids":[a.0],"active":a.0})).unwrap();
    frames(&mut h);
    assert!((h.app.session.companion_editor(a).unwrap().scroll_offset - before).abs() < 2.0);
}

#[test]
fn actual_drafts_without_jobs_save_and_reopen_in_both_languages() {
    for locale in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let catalog = lightcraft_catalog::MemStore::new();
        let files = lightcraft_catalog::MemStore::new();
        let mut session = Session::new();
        session
            .open_library_in(
                lightcraft_engine::library::LibraryStores {
                    catalog: Box::new(catalog.clone()),
                    files: Box::new(files.clone()),
                    dir: "memory".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
        h.app.ui.language = locale;
        h.app.run("ai.open", json!({})).unwrap();
        frames(&mut h);
        let a = h.app.session.active().unwrap();
        let b = h.app.session.catalog.photos().find(|p| p.id != a).unwrap().id;
        let timeout = Duration::from_secs(10);
        h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
        h.request("ui.text", json!({"text":"A unsent draft 草稿"}), timeout);
        h.app.run("library.select", json!({"ids":[b.0],"active":b.0})).unwrap();
        frames(&mut h);
        h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
        h.request("ui.text", json!({"text":"B 草稿 unsent"}), timeout);
        h.app.run("panel.edit", json!({})).unwrap();
        frames(&mut h);
        h.app.session.persist().unwrap();
        assert_eq!(h.app.run("ai.status", json!({})).unwrap()["busy"], false);
        for id in [a, b] {
            assert!(h.app.session.load_companion_conversation(id).unwrap().is_none());
        }
        let mut reopened = Session::new();
        reopened
            .open_library_in(
                lightcraft_engine::library::LibraryStores {
                    catalog: Box::new(catalog),
                    files: Box::new(files),
                    dir: "memory".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let mut h = Headless::new(LightcraftApp::new(reopened, Services::default()), [1000.0, 680.0], 1.0);
        h.app.ui.language = locale;
        h.app.run("library.select", json!({"ids":[a.0],"active":a.0})).unwrap();
        h.app.run("ai.open", json!({})).unwrap();
        frames(&mut h);
        assert!(h.painted_text().iter().any(|text| text == "A unsent draft 草稿"));
        h.app.run("library.select", json!({"ids":[b.0],"active":b.0})).unwrap();
        frames(&mut h);
        assert!(h.painted_text().iter().any(|text| text == "B 草稿 unsent"));
        if let Ok(dir) = std::env::var("UX12_SCREENSHOT_DIR") {
            h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
            h.app.services.png = Some(Box::new(|image| {
                lightcraft_engine::export::encode_image(
                    image,
                    &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                )
                .unwrap()
            }));
            let path = std::path::Path::new(&dir).join(if locale == crate::i18n::Locale::En {
                "drafts-reopened-en.png"
            } else {
                "drafts-reopened-zh-hans.png"
            });
            assert_eq!(h.request("ui.screenshot", json!({"path":path,"headless":true}), Duration::from_secs(20))["ok"], true);
        }
    }
}

#[test]
fn rejected_other_photo_submission_does_not_replace_the_visible_authored_draft() {
    let mut h = headless([1000.0, 680.0]);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    let a = h.app.session.active().unwrap();
    let b = h.app.session.catalog.photos().find(|p| p.id != a).unwrap().id;
    let timeout = Duration::from_secs(10);
    h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
    h.request("ui.text", json!({"text":"A authored draft"}), timeout);
    h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout);
    assert!(h.app.run("ai.start", json!({"photo":b.0,"prompt":"wrong B request"})).is_err());
    frames(&mut h);
    assert_eq!(h.app.session.companion_editor(a).unwrap().draft, "A authored draft");
    assert!(h.painted_text().iter().any(|text| text == "A authored draft"));
}

#[test]
fn native_library_switch_saves_jobless_drafts_and_equal_photo_ids_never_share_them() {
    let dir = std::env::temp_dir().join(format!("lc-ux12-native-{}-{}", std::process::id(), LocalRoundId::new().unwrap().as_str()));
    let old = dir.join("A");
    let next = dir.join("B");
    let mut other = Session::new().with_fs();
    other.open_library(&next, true).unwrap();
    other.close_library().unwrap();
    drop(other);
    let mut session = Session::new().with_fs();
    session.open_library(&old, true).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    let a = h.app.session.active().unwrap();
    let timeout = Duration::from_secs(10);
    h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
    h.request("ui.text", json!({"text":"library A unsent draft"}), timeout);
    h.app.run("app.openLibrary", json!({"path":next.to_string_lossy()})).unwrap();
    frames(&mut h);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    let b = h.app.session.active().unwrap();
    assert_eq!(a, b);
    assert_eq!(h.app.session.companion_editor(b).unwrap().draft, "");
    h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
    h.request("ui.text", json!({"text":"library B independent draft"}), timeout);
    h.app.run("app.openLibrary", json!({"path":old.to_string_lossy()})).unwrap();
    frames(&mut h);
    assert_eq!(h.app.session.companion_editor(a).unwrap().draft, "library A unsent draft");
    assert!(h.painted_text().iter().any(|text| text == "library A unsent draft"));
    h.app.run("app.openLibrary", json!({"path":next.to_string_lossy()})).unwrap();
    frames(&mut h);
    assert_eq!(h.app.session.companion_editor(b).unwrap().draft, "library B independent draft");
    assert!(h.painted_text().iter().any(|text| text == "library B independent draft"));
    h.app.session.close_library().unwrap();
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn protected_outer_archive_keeps_visible_authored_drafts_and_reports_them_unsaved() {
    let files = lightcraft_catalog::MemStore::new();
    files.set("companion/state.json", b"protected unknown root".to_vec());
    let mut session = Session::new();
    session
        .open_library_in(
            lightcraft_engine::library::LibraryStores {
                catalog: Box::new(lightcraft_catalog::MemStore::new()),
                files: Box::new(files),
                dir: "memory".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    let a = h.app.session.active().unwrap();
    let b = h.app.session.catalog.photos().find(|p| p.id != a).unwrap().id;
    let timeout = Duration::from_secs(10);
    h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
    h.request("ui.text", json!({"text":"visible draft must survive"}), timeout);
    assert!(h.app.session.persist().is_err(), "the authored draft must remain dirty under protection");
    assert!(h.app.session.unsaved().is_some());
    h.app.run("library.select", json!({"ids":[b.0],"active":b.0})).unwrap();
    frames(&mut h);
    h.app.run("library.select", json!({"ids":[a.0],"active":a.0})).unwrap();
    frames(&mut h);
    assert!(h.painted_text().iter().any(|text| text == "visible draft must survive"));
    assert_eq!(h.app.session.companion_editor(a).unwrap().draft, "visible draft must survive");
}

fn wait_painted(h: &mut Headless, literal: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        if h.painted_text().iter().any(|text| text.contains(literal)) {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "missing painted {literal}");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn wait_position_page(h: &mut Headless) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historyLoading"] == false
            && status["text"].as_str().unwrap().contains("position row 196")
            && h.painted_text().iter().any(|text| text.contains("position row "))
        {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "saved page not displayed: {status}; {:?}", h.painted_text());
        std::thread::yield_now();
    }
}

fn position_anchor(h: &Headless) -> Option<(String, f32)> {
    let viewport = h.app.widgets.iter().find(|(id, _)| id == "ai:messages")?.1;
    h.view.painted_shapes().iter().find_map(|shape| match shape {
        egui::epaint::Shape::Text(text)
            if text.galley.text().contains("position row ") && viewport.contains(text.visual_bounding_rect().center()) =>
        {
            Some((text.galley.text().to_owned(), text.visual_bounding_rect().top() - viewport.top()))
        }
        _ => None,
    })
}
fn wait_position_anchor(h: &mut Headless, anchor: &(String, f32)) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        if let Some(current) = position_anchor(h)
            && current.0 == anchor.0
            && (current.1 - anchor.1).abs() < 2.0
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "same visible saved text and position not restored: expected {anchor:?}, actual {:?}",
            position_anchor(h)
        );
        std::thread::yield_now();
    }
}

#[test]
fn native_saved_older_page_and_scroll_return_after_photo_tool_switch_and_reopen() {
    let dir = std::env::temp_dir().join(format!("lc-ux12-paged-{}", LocalRoundId::new().unwrap().as_str()));
    let mut session = Session::new().with_fs();
    session.open_library(&dir, true).unwrap();
    let a = session.active().unwrap();
    let b = session.catalog.photos().find(|p| p.id != a).unwrap().id;
    for (photo, label) in [(a, "position"), (b, "other")] {
        let messages: Vec<_> = (0..260)
            .map(|n| json!({"role":"assistant","text":format!("{label} row {n}: enough readable text for the actual scroll viewport.")}))
            .collect();
        session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
    }
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    wait_painted(&mut h, "position row 259");
    let timeout = Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryOlder"}), timeout)["ok"], true);
    wait_painted(&mut h, "position row 196");
    h.request("ui.hoverWidget", json!({"id":"ai:messages"}), timeout);
    h.request("ui.scroll", json!({"dy":-180.0}), timeout);
    for _ in 0..20 {
        h.step();
    }
    wait_position_page(&mut h);
    let saved = h.app.session.companion_editor(a).unwrap();
    assert_eq!(saved.reading_cursor.unwrap().record, 196);
    assert_eq!(saved.reading_count, Some(260));
    assert_eq!(saved.reading_epoch, Some(h.app.session.companion_photo_header(a).unwrap().unwrap().conversation_epoch));
    let anchor = position_anchor(&h).expect("fixture must capture an actually visible same-page text anchor");
    assert!(saved.scroll_offset > 0.0);
    assert!(!saved.follow_tail);
    h.app.run("panel.edit", json!({})).unwrap();
    frames(&mut h);
    h.app.run("ai.open", json!({})).unwrap();
    wait_position_anchor(&mut h, &anchor);
    h.app.run("library.select", json!({"ids":[b.0],"active":b.0})).unwrap();
    wait_painted(&mut h, "other row 259");
    h.app.run("library.select", json!({"ids":[a.0],"active":a.0})).unwrap();
    wait_position_anchor(&mut h, &anchor);
    h.app.session.persist().unwrap();
    h.app.session.close_library().unwrap();
    drop(h);
    let mut session = Session::new().with_fs();
    session.open_library(&dir, false).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
    h.app.run("library.select", json!({"ids":[a.0],"active":a.0})).unwrap();
    h.app.run("ai.open", json!({})).unwrap();
    wait_position_anchor(&mut h, &anchor);
    frames(&mut h);
    let restored = h.app.session.companion_editor(a).unwrap();
    assert_eq!(restored.reading_cursor, saved.reading_cursor);
    assert_eq!(restored.reading_epoch, saved.reading_epoch);
    assert_eq!(restored.reading_count, saved.reading_count);
    assert!((restored.scroll_offset - saved.scroll_offset).abs() < 2.0);
    assert!(!restored.follow_tail);
    h.app.session.close_library().unwrap();
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_pending_older_intent_survives_switching_away_before_the_reader_retires() {
    use lightcraft_catalog::{FsStore, MemStore, Store};
    use std::sync::Mutex;
    struct Gate {
        armed: AtomicBool,
        used: AtomicBool,
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    struct Files {
        inner: FsStore,
        dir: std::path::PathBuf,
        gate: Arc<Gate>,
        reader: bool,
    }
    impl Files {
        fn wait(&self, key: &str) {
            if self.reader && key.contains("/blocks/") && self.gate.armed.load(Ordering::Relaxed) && !self.gate.used.swap(true, Ordering::Relaxed) {
                self.gate.entered.send(()).unwrap();
                self.gate.release.lock().unwrap().recv().unwrap();
            }
        }
    }
    impl Store for Files {
        fn read(&mut self, k: &str) -> std::io::Result<Option<Vec<u8>>> {
            self.inner.read(k)
        }
        fn read_limited(&mut self, k: &str, n: u64) -> std::io::Result<Option<Vec<u8>>> {
            self.wait(k);
            self.inner.read_limited(k, n)
        }
        fn read_range(&mut self, k: &str, o: u64, n: usize) -> std::io::Result<Vec<u8>> {
            self.wait(k);
            self.inner.read_range(k, o, n)
        }
        fn write_atomic(&mut self, k: &str, b: &[u8]) -> std::io::Result<()> {
            self.inner.write_atomic(k, b)
        }
        fn append(&mut self, k: &str, b: &[u8]) -> std::io::Result<()> {
            self.inner.append(k, b)
        }
        fn truncate(&mut self, k: &str, n: u64) -> std::io::Result<()> {
            self.inner.truncate(k, n)
        }
        fn remove(&mut self, k: &str) -> std::io::Result<()> {
            self.inner.remove(k)
        }
        fn describe(&self) -> String {
            "native page read barrier".into()
        }
        fn retention_namespace(&self) -> Option<String> {
            self.inner.retention_namespace()
        }
        fn conservative_retention(&self) -> bool {
            self.inner.conservative_retention()
        }
        fn background_writer(&self) -> Option<Box<dyn Store>> {
            Some(Box::new(Self { inner: FsStore::open(&self.dir).unwrap(), dir: self.dir.clone(), gate: self.gate.clone(), reader: true }))
        }
    }
    let dir = std::env::temp_dir().join(format!("lc-ux12-intent-{}", LocalRoundId::new().unwrap().as_str()));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let gate = Arc::new(Gate { armed: AtomicBool::new(false), used: AtomicBool::new(false), entered: entered_tx, release: Mutex::new(release_rx) });
    let mut session = Session::new();
    session
        .open_library_in(
            lightcraft_engine::library::LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(Files { inner: FsStore::open(&dir).unwrap(), dir: dir.clone(), gate: gate.clone(), reader: false }),
                dir: dir.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let a = session.active().unwrap();
    let b = session.catalog.photos().find(|p| p.id != a).unwrap().id;
    for (photo, label) in [(a, "intent A"), (b, "intent B")] {
        let messages: Vec<_> = (0..260).map(|n| json!({"role":"assistant","text":format!("{label} row {n}")})).collect();
        session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
    }
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    wait_painted(&mut h, "intent A row 259");
    gate.armed.store(true, Ordering::Relaxed);
    h.app.run("ai.history", json!({"record":196})).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    h.app.run("library.select", json!({"ids":[b.0],"active":b.0})).unwrap();
    frames(&mut h);
    release_tx.send(()).unwrap();
    wait_painted(&mut h, "intent B row 259");
    h.app.run("library.select", json!({"ids":[a.0],"active":a.0})).unwrap();
    wait_painted(&mut h, "intent A row 196");
    assert!(!h.painted_text().iter().any(|text| text.contains("intent A row 228")));
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_final_fragment_is_restored_instead_of_the_first_fragment_after_reopen() {
    let dir = std::env::temp_dir().join(format!("lc-ux12-fragment-{}", LocalRoundId::new().unwrap().as_str()));
    let mut session = Session::new().with_fs();
    session.open_library(&dir, true).unwrap();
    let photo = session.active().unwrap();
    let text = format!("FIRST FRAGMENT MARKER\n{}\nSECOND FRAGMENT MARKER\n{}", "first paragraph. ".repeat(17_000), "last paragraph. ".repeat(800));
    session.save_companion_tail(photo, None, &[json!({"role":"assistant","text":text})], &[], &[], &json!({"schema_version":2})).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    wait_painted(&mut h, "FIRST FRAGMENT MARKER");
    frames(&mut h);
    let timeout = Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryNewer"}), timeout)["ok"], true);
    wait_painted(&mut h, "SECOND FRAGMENT MARKER");
    frames(&mut h);
    h.app.session.persist().unwrap();
    h.app.session.close_library().unwrap();
    drop(h);
    let mut session = Session::new().with_fs();
    session.open_library(&dir, false).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    wait_painted(&mut h, "SECOND FRAGMENT MARKER");
    assert!(!h.painted_text().iter().any(|text| text.contains("FIRST FRAGMENT MARKER")));
    h.app.session.close_library().unwrap();
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn first_stop_feedback_does_not_read_a_cold_other_photo_editor_or_show_the_task_draft_there() {
    use lightcraft_catalog::{MemStore, Store};
    use std::sync::atomic::AtomicUsize;
    struct Files {
        inner: MemStore,
        armed: Arc<AtomicBool>,
        reads: Arc<AtomicUsize>,
    }
    impl Files {
        fn count(&self) {
            if self.armed.load(Ordering::Relaxed) {
                self.reads.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    impl Store for Files {
        fn read(&mut self, k: &str) -> std::io::Result<Option<Vec<u8>>> {
            self.count();
            self.inner.read(k)
        }
        fn read_limited(&mut self, k: &str, n: u64) -> std::io::Result<Option<Vec<u8>>> {
            self.count();
            self.inner.read_limited(k, n)
        }
        fn read_range(&mut self, k: &str, o: u64, n: usize) -> std::io::Result<Vec<u8>> {
            self.count();
            self.inner.read_range(k, o, n)
        }
        fn write_atomic(&mut self, k: &str, b: &[u8]) -> std::io::Result<()> {
            self.inner.write_atomic(k, b)
        }
        fn append(&mut self, k: &str, b: &[u8]) -> std::io::Result<()> {
            self.inner.append(k, b)
        }
        fn truncate(&mut self, k: &str, n: u64) -> std::io::Result<()> {
            self.inner.truncate(k, n)
        }
        fn remove(&mut self, k: &str) -> std::io::Result<()> {
            self.inner.remove(k)
        }
        fn describe(&self) -> String {
            "observable editor reads".into()
        }
    }
    let armed = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicUsize::new(0));
    let mut session = Session::new();
    session
        .open_library_in(
            lightcraft_engine::library::LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(Files { inner: MemStore::new(), armed: armed.clone(), reads: reads.clone() }),
                dir: "memory".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let a = session.active().unwrap();
    let b = session.catalog.photos().find(|p| p.id != a).unwrap().id;
    session.set_companion_editor(b, lightcraft_engine::companion::EditorState { draft: "B saved own draft".into(), ..Default::default() }).unwrap();
    session.persist().unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    let (connection, requests, events) = network::test_connection();
    h.app.ai.next_connection = Some(connection);
    let timeout = Duration::from_secs(10);
    h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout);
    h.request("ui.text", json!({"text":"A immutable stopped request"}), timeout);
    h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout);
    h.request("ui.clickWidget", json!({"id":"button:aiStart"}), timeout);
    let creation = requests.try_recv().unwrap();
    events.try_send(network::Event::Server{kind:"round.state".into(),data:json!({"round_id":"feedback-round","payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"photo_id":a.0,"library_generation":creation.identity.scope.library_generation},"state":"queued"}})}).unwrap();
    frames(&mut h);
    h.app.run("library.select", json!({"ids":[b.0],"active":b.0})).unwrap();
    h.app.run("ai.stop", json!({"photo":a.0})).unwrap();
    armed.store(true, Ordering::Relaxed);
    h.step();
    assert_eq!(reads.load(Ordering::Relaxed), 0, "first feedback must not hydrate a cold editor/history");
    assert!(!h.painted_text().iter().any(|text| text == "A immutable stopped request"));
    frames(&mut h);
    assert!(h.painted_text().iter().any(|text| text == "B saved own draft"));
}

#[test]
fn opening_history_does_not_replace_an_unreadable_draft_before_the_user_authors_a_replacement() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::companion::EditorState;
    let files = MemStore::new();
    let mut session = Session::new();
    session
        .open_library_in(
            lightcraft_engine::library::LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(files.clone()),
                dir: "memory".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    session.set_companion_editor(photo, EditorState { draft: "prior authored draft".into(), ..Default::default() }).unwrap();
    session
        .save_companion_tail(
            photo,
            None,
            &[json!({"role":"assistant","text":"readable independent history"})],
            &[],
            &[],
            &json!({"schema_version":2}),
        )
        .unwrap();
    session.persist().unwrap();
    let payload = format!("companion/v2/photos/{}/draft-{}.txt", photo.0, lightcraft_preview::hash_bytes(b"prior authored draft"));
    files.set(&payload, b"damaged".to_vec());
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    wait_painted(&mut h, "readable independent history");
    frames(&mut h);
    let _ = h.app.session.persist();
    assert!(
        h.app.session.companion_editor(photo).is_err(),
        "reading history must preserve the unreadable draft instead of saving an empty replacement"
    );
    let timeout = Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout)["ok"], true);
    assert_eq!(h.request("ui.text", json!({"text":"new explicitly authored draft"}), timeout)["ok"], true);
    h.app.session.persist().unwrap();
    assert_eq!(h.app.session.companion_editor(photo).unwrap().draft, "new explicitly authored draft");
}
