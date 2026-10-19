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
fn native_reopen_qualified_round_restores_full_start_through_real_controls_in_both_languages_and_tools_sizes() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        for layout in ["normal", "normal1280", "narrow", "narrow240", "float120", "float240", "stack120", "stack240"] {
            native_reopen(false, language, layout);
        }
    }
}
#[test]
fn native_reverted_round_fact_is_readable_after_reopen_without_undo_permission() {
    native_reopen(true, crate::i18n::Locale::En, "normal");
}
fn native_reopen(reverted: bool, language: crate::i18n::Locale, layout: &str) {
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
    h.app.ui.language = language;
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
    assert_eq!(cards[0]["canUndo"], !reverted);
    assert_eq!(cards[0]["reverted"], reverted);
    if reverted {
        assert!(status["text"].as_str().unwrap().contains("This round's edits were reverted"));
    }
    assert_eq!(cards[0]["stages"].as_array().unwrap().len(), 2);
    assert_eq!(*h.app.session.develop_of(photo).unwrap(), *current);
    let latest = h.app.session.catalog.photo(photo).unwrap().history.len() - if reverted { 3 } else { 2 };
    if reverted {
        assert!(h.app.run("ai.undo", json!({"local_round_id":round})).is_err());
    } else {
        let size = if layout == "normal" {
            [1400, 1000]
        } else if layout == "narrow" {
            [620, 520]
        } else {
            [1000, 680]
        };
        let timeout = Duration::from_secs(10);
        assert_eq!(h.request("ui.resize", json!({"width":size[0],"height":size[1]}), timeout)["ok"], true);
        frames(&mut h);
        if layout.starts_with("float") || layout.starts_with("stack") {
            super::panel_tests::short_tools(&mut h, layout.starts_with("stack"), if layout.ends_with("120") { 120.0 } else { 240.0 });
        }
        show_label(&mut h, if language == crate::i18n::Locale::ZhHans { "撤销" } else { "Undo" });
        let id = format!("button:aiUndo:{round}");
        let target = h.app.widgets.iter().find(|(name, _)| name == &id).unwrap().1;
        let tools = h.app.widgets.iter().find(|(name, _)| name == "panel:right_panel").unwrap().1;
        let viewport = h.app.widgets.iter().find(|(name, _)| name == "ai:messages").unwrap().1;
        assert!(tools.contains_rect(target));
        assert!(viewport.contains_rect(target));
        screen(&mut h, &format!("cold-qualified-{layout}-{}", language.code()));
        let zoom = h.app.ui.zoom;
        assert_eq!(h.request("ui.clickWidget", json!({"id":id}), timeout)["ok"], true);
        assert_eq!(*h.app.session.develop_of(photo).unwrap(), start);
        assert_eq!(h.app.ui.zoom, zoom);
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(status["resultCards"][0]["reverted"], true);
        assert_eq!(status["resultCards"][0]["canUndo"], false);
        if layout == "normal" {
            show_label(&mut h, if language == crate::i18n::Locale::ZhHans { "已撤回" } else { "Reverted" });
            screen(&mut h, &format!("cold-reverted-{layout}-{}", language.code()));
        }
    }
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
        for layout in ["normal", "normal1280", "narrow", "narrow240", "float120", "float240", "stack120", "stack240"] {
            let (mut h, photo, round, start, _requests, _events) = actual_round();
            h.app.ui.language = language;
            let timeout = Duration::from_secs(10);
            let size = if layout == "normal" {
                [1400, 1000]
            } else if layout == "normal1280" {
                [1280, 800]
            } else if layout == "narrow" {
                [620, 520]
            } else if layout == "narrow240" {
                [620, 800]
            } else {
                [1000, 680]
            };
            assert_eq!(h.request("ui.resize", json!({"width":size[0],"height":size[1]}), timeout)["ok"], true);
            frames(&mut h);
            if layout.starts_with("float") || layout.starts_with("stack") {
                super::panel_tests::short_tools(&mut h, layout.starts_with("stack"), if layout.ends_with("120") { 120.0 } else { 240.0 });
            }
            let literal = if language == crate::i18n::Locale::ZhHans { "撤销" } else { "Undo" };
            show_label(&mut h, literal);
            let id = format!("button:aiUndo:{round}");
            let target = h.app.widgets.iter().find(|(name, _)| name == &id).unwrap().1;
            let tools = h.app.widgets.iter().find(|(name, _)| name == "panel:right_panel").unwrap().1;
            let viewport = h.app.widgets.iter().find(|(name, _)| name == "ai:messages").unwrap().1;
            assert!(tools.contains_rect(target));
            if layout == "narrow240" {
                assert!((tools.width() - 240.0).abs() < 8.0, "AI panel should be about 240px wide, got {}", tools.width());
                assert!(!h.painted_text().iter().any(|text| text.contains("本轮已完成")), "the card owns the completion state");
            }
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
                show_label(&mut h, if language == crate::i18n::Locale::ZhHans { "已撤回" } else { "Reverted" });
                screen(&mut h, &format!("reverted-{layout}-{}", language.code()));
            }
        }
    }
}

struct RevertFault {
    memory: lightcraft_catalog::MemStore,
    fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl lightcraft_catalog::Store for RevertFault {
    fn read(&mut self, n: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.memory.read(n)
    }
    fn read_limited(&mut self, n: &str, l: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.memory.read_limited(n, l)
    }
    fn read_range(&mut self, n: &str, o: u64, l: usize) -> std::io::Result<Vec<u8>> {
        self.memory.read_range(n, o, l)
    }
    fn remove(&mut self, n: &str) -> std::io::Result<()> {
        self.memory.remove(n)
    }
    fn write_atomic(&mut self, n: &str, b: &[u8]) -> std::io::Result<()> {
        if n.ends_with("/revert.json") && self.fail.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(std::io::Error::other("injected Q30 guard fault"));
        }
        self.memory.write_atomic(n, b)
    }
    fn append(&mut self, n: &str, b: &[u8]) -> std::io::Result<()> {
        self.memory.append(n, b)
    }
    fn truncate(&mut self, n: &str, l: u64) -> std::io::Result<()> {
        self.memory.truncate(n, l)
    }
    fn describe(&self) -> String {
        self.memory.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
}
#[test]
fn rejected_guard_edit_has_no_false_later_edit_fact_on_the_real_card() {
    let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut session = Session::new();
    session
        .open_library_in(
            lightcraft_engine::library::LibraryStores {
                catalog: Box::new(lightcraft_catalog::MemStore::new()),
                files: Box::new(RevertFault { memory: lightcraft_catalog::MemStore::new(), fail: fail.clone() }),
                dir: "memory".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let (mut h, photo, _round, _start, _requests, _events) = actual_round_with_session(session);
    let before = h.app.session.develop_of(photo).unwrap();
    fail.store(true, std::sync::atomic::Ordering::Relaxed);
    let error = h.app.run("develop.set", json!({"control":"light.exposure","value":1.4})).unwrap_err();
    assert!(error.contains("This edit was not applied"));
    assert_eq!(h.app.session.develop_of(photo).unwrap(), before);
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["resultCards"][0]["canUndo"], false);
    assert_eq!(status["resultCards"][0]["currentPhotoHasLaterEdits"], false, "relinquished protection is not an applied edit");
}

#[test]
fn save_retry_control_recovers_storage_without_replaying_edit() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut session = Session::new();
        session
            .open_library_in(
                lightcraft_engine::library::LibraryStores {
                    catalog: Box::new(lightcraft_catalog::MemStore::new()),
                    files: Box::new(RevertFault { memory: lightcraft_catalog::MemStore::new(), fail: fail.clone() }),
                    dir: "memory".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let (mut h, photo, _round, _start, requests, _events) = actual_round_with_session(session);
        h.app.ui.language = language;
        frames(&mut h);
        let before = h.app.session.develop_of(photo).unwrap();
        let history = serde_json::to_value(&h.app.session.catalog.photo(photo).unwrap().history).unwrap();
        let labels: Vec<_> = h.app.session.undo.iter().map(|e| e.label.clone()).collect();
        fail.store(true, std::sync::atomic::Ordering::Relaxed);
        let error = h.app.run("develop.set", json!({"control":"light.exposure","value":1.4})).unwrap_err().to_string();
        let cause = if language == crate::i18n::Locale::ZhHans {
            "无法保存撤回保护记录，本次编辑未执行。请检查存储后重试。"
        } else {
            "The revert protection record could not be saved. This edit was not applied; check storage and retry."
        };
        assert!(error.contains(if language == crate::i18n::Locale::ZhHans { "本次编辑未执行" } else { "This edit was not applied" }));
        super::panel_tests::short_tools(&mut h, true, 120.0);
        show_composer_label(&mut h, cause);
        screen(&mut h, &format!("save-cause-short-{}", language.code()));
        let literal = if language == crate::i18n::Locale::ZhHans { "重试保存" } else { "Retry Saving" };
        show_composer_label(&mut h, literal);
        screen(&mut h, &format!("save-failed-short-{}", language.code()));
        fail.store(false, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiRetrySave"}), Duration::from_secs(10))["ok"], true);
        assert_eq!(h.app.session.develop_of(photo).unwrap(), before);
        assert_eq!(serde_json::to_value(&h.app.session.catalog.photo(photo).unwrap().history).unwrap(), history);
        assert_eq!(h.app.session.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
        assert_eq!(h.app.run("ai.status", json!({})).unwrap()["saveState"], "saved");
        assert_eq!(h.app.session.develop_of(photo).unwrap(), before);
        assert!(requests.try_recv().is_err(), "saving must not reconnect or submit a backend action");
        show_composer_label(
            &mut h,
            if language == crate::i18n::Locale::ZhHans {
                "已保存。准备好后请明确重试编辑。"
            } else {
                "Saved. Retry the edit explicitly when ready."
            },
        );
        screen(&mut h, &format!("save-recovered-short-{}", language.code()));
        h.app.session.end_interaction().unwrap();
        h.app.session.begin_interaction("Explicit retry").unwrap();
        h.app.run("develop.set", json!({"control":"light.exposure","value":1.4})).unwrap();
        h.app.session.end_interaction().unwrap();
        assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 1.4);
    }
}

#[test]
fn readonly_task_retry_acknowledges_the_displayed_photo_without_editing_selected_photo() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut session = Session::new();
        session
            .open_library_in(
                lightcraft_engine::library::LibraryStores {
                    catalog: Box::new(lightcraft_catalog::MemStore::new()),
                    files: Box::new(RevertFault { memory: lightcraft_catalog::MemStore::new(), fail: fail.clone() }),
                    dir: "memory".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let (mut h, a, _round, _start, requests, _events) = actual_round_with_session(session);
        h.app.ui.language = language;
        fail.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(h.app.run("develop.set", json!({"control":"light.exposure","value":1.4})).is_err());
        frames(&mut h);
        fail.store(false, std::sync::atomic::Ordering::Relaxed);
        h.app.run("photo.delete", json!({"ids":[a.0]})).unwrap();
        let b = h.app.session.active().unwrap();
        assert_ne!(a, b);
        h.app.run("ai.task", json!({})).unwrap();
        frames(&mut h);
        assert_eq!(h.app.run("ai.status", json!({})).unwrap()["readonlyPhoto"], a.0);
        let before = serde_json::to_value(&h.app.session.catalog).unwrap();
        let labels: Vec<_> = h.app.session.undo.iter().map(|e| e.label.clone()).collect();
        super::panel_tests::short_tools(&mut h, true, 120.0);
        h.app.run("ai.task", json!({})).unwrap();
        frames(&mut h);
        show_composer_label(&mut h, if language == crate::i18n::Locale::ZhHans { "重试保存" } else { "Retry Saving" });
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiRetrySave"}), Duration::from_secs(10))["ok"], true);
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(status["readonlyPhoto"], a.0);
        assert!(status["text"].as_str().unwrap().contains(if language == crate::i18n::Locale::ZhHans {
            "已保存。准备好后请明确重试编辑。"
        } else {
            "Saved. Retry the edit explicitly when ready."
        }));
        assert_eq!(h.app.session.active(), Some(b));
        assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), before);
        assert_eq!(h.app.session.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn stale_or_closed_library_save_retry_refuses_before_saving_another_library() {
    let (mut h, a, _round, _start, requests, _events) = actual_round();
    let old_generation = h.app.session.library_generation();
    let files = lightcraft_catalog::MemStore::new();
    h.app
        .session
        .open_library_in(
            lightcraft_engine::library::LibraryStores {
                catalog: Box::new(lightcraft_catalog::MemStore::new()),
                files: Box::new(files.clone()),
                dir: "another-library".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let b = h.app.session.active().unwrap();
    let generation = h.app.session.library_generation();
    assert_ne!(old_generation, generation);
    let draft = lightcraft_engine::companion::EditorState { draft: "B's unsaved local draft".into(), ..Default::default() };
    h.app.session.set_companion_editor(b, draft.clone()).unwrap();
    let dirty = h.app.session.companion_dirty_editors();
    let before = serde_json::to_value(&h.app.session.catalog).unwrap();
    assert!(
        h.app.run("ai.retrySave", json!({"photo":a.0,"library_generation":old_generation})).is_err(),
        "captured A scope must not flush B's dirty data"
    );
    assert_eq!(h.app.session.companion_dirty_editors(), dirty);
    assert_eq!(h.app.session.companion_editor(b).unwrap(), draft);
    assert!(files.get(&format!("companion/v2/photos/{}/editor.json", b.0)).is_none());
    assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), before);
    assert_eq!(h.app.run("ai.retrySave", json!({"photo":b.0,"library_generation":generation})).unwrap()["saved"], true);
    assert!(h.app.session.companion_dirty_editors().is_empty());
    assert_eq!(h.app.session.companion_editor(b).unwrap(), draft);
    assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), before);
    h.app.session.close_library().unwrap();
    h.app.session = Session::new();
    assert!(h.app.run("ai.retrySave", json!({})).is_err(), "closed library has no durable save receipt");
    assert!(requests.try_recv().is_err());
}

#[test]
fn save_retry_command_never_releases_or_replays_a_rejected_held_gesture() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut session = Session::new();
        session
            .open_library_in(
                lightcraft_engine::library::LibraryStores {
                    catalog: Box::new(lightcraft_catalog::MemStore::new()),
                    files: Box::new(RevertFault { memory: lightcraft_catalog::MemStore::new(), fail: fail.clone() }),
                    dir: "memory".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let (mut h, photo, _round, _start, requests, _events) = actual_round_with_session(session);
        h.app.ui.language = language;
        frames(&mut h);
        let before = serde_json::to_value(&h.app.session.catalog).unwrap();
        let labels: Vec<_> = h.app.session.undo.iter().map(|e| e.label.clone()).collect();
        h.app.session.begin_interaction("Exposure").unwrap();
        fail.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(h.app.run("develop.set", json!({"control":"light.exposure","value":1.4})).is_err());
        fail.store(false, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(h.app.run("ai.retrySave", json!({"photo":photo.0})).unwrap()["saved"], true);
        let error = h.app.run("develop.set", json!({"control":"light.exposure","value":1.4})).unwrap_err();
        assert!(error.contains(if language == crate::i18n::Locale::ZhHans { "本次手势未执行" } else { "This gesture was not applied" }));
        assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), before);
        assert_eq!(h.app.session.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
        assert!(requests.try_recv().is_err());
        h.app.session.end_interaction().unwrap();
        h.app.session.begin_interaction("Explicit retry").unwrap();
        h.app.run("develop.set", json!({"control":"light.exposure","value":1.4})).unwrap();
        h.app.session.end_interaction().unwrap();
        assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 1.4);
    }
}

#[test]
fn save_retry_does_not_claim_saved_while_the_certificate_still_waits_for_interaction_end() {
    let (catalog, files) = (lightcraft_catalog::MemStore::new(), lightcraft_catalog::MemStore::new());
    let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut session = Session::new();
    session
        .open_library_in(
            lightcraft_engine::library::LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(RevertFault { memory: files.clone(), fail: fail.clone() }),
                dir: "memory".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let (mut h, a, round, start, requests, _events) = actual_round_with_session(session);
    h.app.session.persist().unwrap();
    let b = h.app.session.catalog.photos().find(|p| p.id != a).unwrap().id;
    let rating = (h.app.session.catalog.photo(b).unwrap().rating + 1) % 6;
    fail.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = h.app.session.execute("photo.rate", &json!({"ids":[b.0],"rating":rating}));
    assert!(h.app.session.persist().is_err());
    assert!(h.app.session.companion_photo_unsaved(a));
    assert!(!h.app.session.companion_history_unsaved(a));
    let before = serde_json::to_value(&h.app.session.catalog).unwrap();
    let labels: Vec<_> = h.app.session.undo.iter().map(|e| e.label.clone()).collect();
    h.app.session.begin_interaction("Exposure").unwrap();
    fail.store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(h.app.run("ai.retrySave", json!({"photo":a.0})).is_err(), "a deferred certificate is still unsaved");
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["saveState"], "unsaved");
    assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), before);
    assert_eq!(h.app.session.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
    h.app.session.end_interaction().unwrap();
    assert_eq!(h.app.run("ai.retrySave", json!({"photo":a.0})).unwrap()["saved"], true);
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["saveState"], "saved");
    assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), before);
    assert!(requests.try_recv().is_err());
    h.app.session.close_library().unwrap();
    drop(h);
    let mut reopened = Session::new();
    reopened
        .open_library_in(
            lightcraft_engine::library::LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "memory".into(), on_disk: false },
            false,
        )
        .unwrap();
    let round: lightcraft_engine::companion::LocalRoundId = serde_json::from_value(json!(round)).unwrap();
    let permit = reopened.verify_round_revert(a, &round).unwrap();
    reopened.commit_verified_round_revert(permit).unwrap();
    assert_eq!(*reopened.develop_of(a).unwrap(), start);
    assert_eq!(reopened.catalog.photo(b).unwrap().rating, rating);
}

#[cfg(unix)]
#[test]
fn cold_alias_guard_failure_exposes_local_save_recovery_on_the_selected_photo() {
    let root = std::env::temp_dir().join(format!("lc-ux18-recovery-alias-{}", lightcraft_engine::companion::LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir_all(&root).unwrap();
    let original = root.join("original.png");
    let alias = root.join("alias.png");
    let image = lightcraft_raster::Rgba8::from_fn(8, 8, |x, y| [(x * 20) as u8, (y * 20) as u8, 60, 255]);
    let bytes = lightcraft_engine::export::encode_image(
        &image,
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    std::fs::write(&original, &bytes).unwrap();
    std::os::unix::fs::symlink(&original, &alias).unwrap();
    let (catalog, files) = (lightcraft_catalog::MemStore::new(), lightcraft_catalog::MemStore::new());
    let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut session = Session::new().with_fs();
    session
        .open_library_in(
            lightcraft_engine::library::LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(RevertFault { memory: files.clone(), fail: fail.clone() }),
                dir: "memory".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let a = session.active().unwrap();
    let b = session.catalog.photos().find(|p| p.id != a).unwrap().id;
    for (id, path, name) in [(a, &alias, "alias.png"), (b, &original, "original.png")] {
        session
            .commit(
                "Native source",
                lightcraft_catalog::Op::SetFile {
                    id,
                    file_name: name.into(),
                    source: lightcraft_catalog::Source::File { path: path.to_string_lossy().into() },
                },
            )
            .unwrap();
    }
    let round = lightcraft_engine::companion::LocalRoundId::new().unwrap();
    let start = session.develop_of(a).unwrap();
    let lease = session.acquire_companion_lease(a, round.clone()).unwrap();
    let mut edited = (*start).clone();
    edited.light.exposure = 0.75;
    let op = session.develop_op(a, edited, "AI edit plan").unwrap();
    session.commit_companion(&lease, &round, session.photo_revision(a), op).unwrap();
    drop(lease);
    session.save_companion_conversation(a,json!({"schema_version":2,"messages":[],"rounds":[],"round_records":[{"local_round_id":round.as_str(),"original":start,"recent_complete_result":session.develop_of(a).unwrap(),"complete_state":"complete"}]})).unwrap();
    session.close_library().unwrap();
    drop(session);
    let mut session = Session::new().with_fs();
    session
        .open_library_in(
            lightcraft_engine::library::LibraryStores {
                catalog: Box::new(catalog),
                files: Box::new(RevertFault { memory: files, fail: fail.clone() }),
                dir: "memory".into(),
                on_disk: false,
            },
            false,
        )
        .unwrap();
    session.execute("library.select", &json!({"ids":[b.0],"active":b.0})).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    let before_a = h.app.session.develop_of(a).unwrap();
    let before_b = h.app.session.develop_of(b).unwrap();
    let history = serde_json::to_value(&h.app.session.catalog.photo(a).unwrap().history).unwrap();
    let labels: Vec<_> = h.app.session.undo.iter().map(|e| e.label.clone()).collect();
    let plans = h.app.session.plan_rename(&[b], "moved", 1);
    fail.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(h.app.session.apply_rename(&plans).is_err());
    frames(&mut h);
    show_composer_label(&mut h, "Retry Saving");
    fail.store(false, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiRetrySave"}), Duration::from_secs(10))["ok"], true);
    assert_eq!(h.app.session.active(), Some(b));
    assert_eq!(h.app.session.develop_of(a).unwrap(), before_a);
    assert_eq!(h.app.session.develop_of(b).unwrap(), before_b);
    assert_eq!(serde_json::to_value(&h.app.session.catalog.photo(a).unwrap().history).unwrap(), history);
    assert_eq!(h.app.session.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
    assert_eq!(std::fs::read(&original).unwrap(), bytes);
    assert_eq!(std::fs::read(&alias).unwrap(), bytes);
    assert!(!root.join("moved.png").exists(), "local save never replays file movement");
    assert!(h.app.session.verify_round_revert(a, &round).is_err());
    h.app.session.apply_rename(&plans).unwrap();
    assert!(root.join("moved.png").exists());
    drop(h);
    std::fs::remove_dir_all(root).unwrap();
}

fn show_composer_label(h: &mut Headless, literal: &str) {
    for index in 0..180 {
        let tools = h.app.widgets.iter().find(|(id, _)| id == "panel:right_panel").unwrap().1;
        let composer = h.app.widgets.iter().find(|(id, _)| id == "ai:composer").unwrap().1;
        if h.view.painted_shapes().iter().any(|shape|matches!(shape,egui::epaint::Shape::Text(text) if text.opacity_factor>0.0 && text.galley.text()==literal && tools.contains_rect(text.visual_bounding_rect()) && composer.contains_rect(text.visual_bounding_rect()))) { return }
        h.app.synthetic.push(egui::Event::PointerMoved(egui::pos2(composer.right() - 2.0, composer.center().y)));
        h.app.synthetic.push(egui::Event::MouseWheel {
            phase: egui::TouchPhase::Move,
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, if index < 90 { -8.0 } else { 8.0 }),
            modifiers: egui::Modifiers::NONE,
        });
        frames(h);
    }
    panic!(
        "complete composer galley was never visible: {literal}; status={}; painted={:?}",
        h.app.run("ai.status", json!({})).unwrap(),
        h.painted_text()
    );
}
