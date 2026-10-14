//! Clear through the same external commands and widgets as the application.
use crate::{LightcraftApp, Services};
use lightcraft_engine::Session;
use serde_json::json;

#[test]
fn unconfirmed_clear_keeps_the_conversation_and_returns_its_scope() {
    let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
    let photo = app.session.active().unwrap();
    app.session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"Keep this old request"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
    let reply = app.run("ai.clear", json!({})).unwrap();
    assert_eq!(reply["confirmationRequired"], true);
    assert_eq!(reply["photo"], photo.0);
    assert_eq!(reply["libraryGeneration"], app.session.library_generation());
    assert_eq!(app.session.load_companion_conversation(photo).unwrap().unwrap()["messages"][0]["text"], "Keep this old request");
}

#[test]
fn confirmed_clear_from_another_library_refuses_before_touching_equal_photo_id() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "clear-A".into(), on_disk: false },
            true,
        )
        .unwrap();
    let mut app = LightcraftApp::new(session, Services::default());
    let photo = app.session.active().unwrap();
    app.session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"A history"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
    let confirmation = app.run("ai.clear", json!({})).unwrap();
    app.session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "clear-B".into(), on_disk: false },
            true,
        )
        .unwrap();
    assert_eq!(app.session.active(), Some(photo));
    app.session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"B history must remain"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
    let before = serde_json::to_value(&app.session.catalog).unwrap();
    let undo = app.session.undo.iter().map(|entry| entry.label.clone()).collect::<Vec<_>>();
    let result = app.run("ai.clear",json!({"confirm":true,"photo":confirmation["photo"],"library_generation":confirmation["libraryGeneration"],"expected_epoch":confirmation["expectedEpoch"]}));
    assert!(result.is_err(), "old-library confirmation is not authority to clear the new library");
    assert_eq!(app.session.load_companion_conversation(photo).unwrap().unwrap()["messages"][0]["text"], "B history must remain");
    assert_eq!(serde_json::to_value(&app.session.catalog).unwrap(), before);
    assert_eq!(app.session.undo.iter().map(|entry| entry.label.clone()).collect::<Vec<_>>(), undo);
}

#[test]
fn confirmed_command_commits_new_context_and_keeps_the_visible_unsent_draft_and_photo() {
    use crate::headless::Headless;
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    use std::time::Duration;
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "visible-clear".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let mut edited = (*session.develop_of(photo).unwrap()).clone();
    edited.light.exposure = 0.5;
    session.set_develop(photo, edited, "Manual effect").unwrap();
    session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"Old conversation must disappear"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
    let mut app = LightcraftApp::new(session, Services::default());
    app.ui.settings.gpu = false;
    let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    for _ in 0..5 {
        h.step();
    }
    let timeout = Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout)["ok"], true);
    assert_eq!(h.request("ui.text", json!({"text":"Keep this visible draft"}), timeout)["ok"], true);
    assert!(h.painted_text().iter().any(|text| text == "Keep this visible draft"));
    let confirmation = h.app.run("ai.clear", json!({})).unwrap();
    let before = serde_json::to_value(&h.app.session.catalog).unwrap();
    let undo = h.app.session.undo.iter().map(|entry| entry.label.clone()).collect::<Vec<_>>();
    let reply=h.app.run("ai.clear",json!({"confirm":true,"photo":confirmation["photo"],"library_generation":confirmation["libraryGeneration"],"expected_epoch":confirmation["expectedEpoch"]})).unwrap();
    assert_eq!(reply["cleared"], true);
    let header = h.app.session.companion_photo_header(photo).unwrap().unwrap();
    assert_eq!(
        header.conversation_epoch,
        confirmation["expectedEpoch"].as_u64().unwrap() + 1,
        "confirmed command must use the acknowledged new-epoch transaction"
    );
    assert!(h.app.session.companion_messages_page(photo, 0, 32).unwrap().records.is_empty());
    for _ in 0..5 {
        h.step();
    }
    assert!(h.painted_text().iter().any(|text| text == "Keep this visible draft"));
    assert!(!h.painted_text().iter().any(|text| text.contains("Old conversation must disappear")));
    assert_eq!(h.app.session.companion_editor(photo).unwrap().draft, "Keep this visible draft");
    assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), before);
    assert_eq!(h.app.session.undo.iter().map(|entry| entry.label.clone()).collect::<Vec<_>>(), undo);
}

#[test]
fn delete_button_confirms_and_cancel_keeps_context_before_a_new_epoch() {
    use crate::headless::Headless;
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    use std::time::Duration;
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "dialog-clear".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"Retain until confirmed"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
    let mut app = LightcraftApp::new(session, Services::default());
    app.ui.settings.gpu = false;
    let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    for _ in 0..5 {
        h.step();
    }
    let timeout = Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClear"}), timeout)["ok"], true);
    assert!(
        h.painted_text().iter().any(|text| text.contains("Plans and comparisons in this conversation will also be deleted")),
        "actual delete button must show the preservation warning before clearing"
    );
    assert_eq!(h.app.session.load_companion_conversation(photo).unwrap().unwrap()["messages"][0]["text"], "Retain until confirmed");
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:dialogCancel"}), timeout)["ok"], true);
    assert_eq!(h.app.session.load_companion_conversation(photo).unwrap().unwrap()["messages"][0]["text"], "Retain until confirmed");
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClear"}), timeout)["ok"], true);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:dialogOk"}), timeout)["ok"], true);
    assert_eq!(h.app.session.companion_photo_header(photo).unwrap().unwrap().conversation_epoch, 1);
    assert!(h.app.session.companion_messages_page(photo, 0, 32).unwrap().records.is_empty());
}

#[test]
fn malformed_confirmation_never_falls_back_to_the_selected_photo() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "malformed-clear".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"Keep selection on invalid input"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
    let mut app = LightcraftApp::new(session, Services::default());
    let confirmation = app.run("ai.clear", json!({})).unwrap();
    let valid = json!({"confirm":true,"photo":confirmation["photo"],"library_generation":confirmation["libraryGeneration"],"expected_epoch":confirmation["expectedEpoch"]});
    let before = serde_json::to_value(&app.session.catalog).unwrap();
    for (field, value) in [
        ("photo", json!("invalid")),
        ("photo", serde_json::Value::Null),
        ("expected_epoch", json!("invalid")),
        ("library_generation", serde_json::Value::Null),
    ] {
        let mut input = valid.clone();
        input[field] = value;
        assert!(app.run("ai.clear", input.clone()).is_err(), "malformed confirmation must refuse: {input}");
        assert_eq!(app.session.load_companion_conversation(photo).unwrap().unwrap()["messages"][0]["text"], "Keep selection on invalid input");
    }
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("photo");
    assert!(app.run("ai.clear", missing).is_err(), "confirmed deletion requires captured photo");
    assert_eq!(serde_json::to_value(&app.session.catalog).unwrap(), before);
}

struct ClearFiles {
    memory: lightcraft_catalog::MemStore,
    fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
    visible: bool,
    cleanup: bool,
}
impl lightcraft_catalog::Store for ClearFiles {
    fn read(&mut self, key: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.memory.read(key)
    }
    fn read_limited(&mut self, key: &str, limit: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.memory.read_limited(key, limit)
    }
    fn read_range(&mut self, key: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.memory.read_range(key, offset, length)
    }
    fn write_atomic(&mut self, key: &str, bytes: &[u8]) -> std::io::Result<()> {
        if !self.cleanup && key.ends_with("/HEAD.json") && self.fail.load(std::sync::atomic::Ordering::Relaxed) {
            if self.visible {
                self.memory.write_atomic(key, bytes)?;
            }
            return Err(std::io::Error::other("UX13 confirmed clear HEAD unavailable"));
        }
        self.memory.write_atomic(key, bytes)
    }
    fn append(&mut self, key: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.memory.append(key, bytes)
    }
    fn truncate(&mut self, key: &str, length: u64) -> std::io::Result<()> {
        self.memory.truncate(key, length)
    }
    fn remove(&mut self, key: &str) -> std::io::Result<()> {
        if self.cleanup && key.ends_with("/pending.json") && self.fail.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(std::io::Error::other("UX13 acknowledged clear cleanup unavailable"));
        }
        self.memory.remove(key)
    }
    fn describe(&self) -> String {
        self.memory.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
}
#[test]
fn actual_save_retry_acknowledges_failed_clear_without_losing_the_visible_draft() {
    use crate::headless::Headless;
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    for visible in [false, true] {
        let fail = Arc::new(AtomicBool::new(false));
        let mut session = Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(MemStore::new()),
                    files: Box::new(ClearFiles { memory: MemStore::new(), fail: fail.clone(), visible, cleanup: false }),
                    dir: "clear-ui-retry".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let photo = session.active().unwrap();
        session.save_companion_conversation(photo,json!({"schema_version":2,"messages":[{"role":"user","text":"Old context retained until acknowledgement"}],"rounds":[],"round_records":[]})).unwrap();
        let mut app = LightcraftApp::new(session, Services::default());
        app.ui.settings.gpu = false;
        let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        for _ in 0..5 {
            h.step();
        }
        let timeout = Duration::from_secs(10);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout)["ok"], true);
        assert_eq!(h.request("ui.text", json!({"text":"Keep retry draft"}), timeout)["ok"], true);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClear"}), timeout)["ok"], true);
        fail.store(true, Ordering::Relaxed);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClearConfirm"}), timeout)["ok"], true);
        for _ in 0..5 {
            h.step();
        }
        let failed = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(failed["saveState"], "unsaved");
        assert!(failed["text"].as_str().unwrap().contains("Old context retained until acknowledgement"));
        let transaction = h.app.session.companion_pending(photo).unwrap().unwrap().transaction_id;
        fail.store(false, Ordering::Relaxed);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiRetrySave"}), timeout)["ok"], true);
        for _ in 0..5 {
            h.step();
        }
        let saved = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(saved["saveState"], "saved", "retry must finish the already-confirmed deletion: {saved}");
        let header = h.app.session.companion_photo_header(photo).unwrap().unwrap();
        assert_eq!(header.transaction_id, transaction);
        assert_eq!(header.conversation_epoch, 1);
        assert!(h.app.session.companion_messages_page(photo, 0, 32).unwrap().records.is_empty());
        assert_eq!(h.app.session.companion_editor(photo).unwrap().draft, "Keep retry draft");
        assert!(!saved["text"].as_str().unwrap().contains("Old context retained until acknowledgement"));
    }
}

#[test]
fn confirmation_and_failed_deletion_are_localized_and_reachable_in_small_tools() {
    use crate::{
        headless::Headless,
        i18n::{Locale, tr},
    };
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    for language in [Locale::En, Locale::ZhHans] {
        for layout in ["normal", "narrow", "float120", "stack240"] {
            let fail = Arc::new(AtomicBool::new(false));
            let mut session = Session::new();
            session
                .open_library_in(
                    LibraryStores {
                        catalog: Box::new(MemStore::new()),
                        files: Box::new(ClearFiles { memory: MemStore::new(), fail: fail.clone(), visible: false, cleanup: false }),
                        dir: "clear-localized".into(),
                        on_disk: false,
                    },
                    true,
                )
                .unwrap();
            let photo = session.active().unwrap();
            session
                .save_companion_conversation(
                    photo,
                    json!({"schema_version":2,"messages":[{"role":"user","text":"Original conversation"}],"rounds":[],"round_records":[]}),
                )
                .unwrap();
            let mut app = LightcraftApp::new(session, Services::default());
            app.ui.language = language;
            app.ui.settings.gpu = false;
            app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
            app.services.png = Some(Box::new(|image| {
                lightcraft_engine::export::encode_image(
                    image,
                    &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                )
                .unwrap()
            }));
            let mut h = Headless::new(app, if layout == "narrow" { [880.0, 600.0] } else { [1400.0, 1000.0] }, 1.0);
            h.app.run("ai.open", json!({})).unwrap();
            for _ in 0..5 {
                h.step();
            }
            if layout.starts_with("float") || layout.starts_with("stack") {
                super::panel_tests::short_tools(&mut h, layout.starts_with("stack"), if layout.ends_with("120") { 120.0 } else { 240.0 });
            }
            super::panel_tests::reachable(&mut h, "button:aiClear", -1.0);
            let timeout = Duration::from_secs(10);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClear"}), timeout)["ok"], true);
            let warning =
                tr("Plans and comparisons in this conversation will also be deleted. Photo effects, edit history and the unsent draft are kept.");
            assert!(h.painted_text().iter().any(|text| text == warning), "complete localized modal warning {layout}/{}", language.code());
            if let Ok(dir) = std::env::var("UX13_SCREENSHOT_DIR") {
                assert_eq!(
                    h.request(
                        "ui.screenshot",
                        json!({"path":format!("{dir}/confirm-{layout}-{}.png",language.code()),"headless":true}),
                        Duration::from_secs(30)
                    )["ok"],
                    true
                );
            }
            fail.store(true, Ordering::Relaxed);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClearConfirm"}), timeout)["ok"], true);
            for _ in 0..5 {
                h.step();
            }
            let status = h.app.run("ai.status", json!({})).unwrap();
            let cause = tr("Conversation deletion has not been saved. Check storage, then retry saving.");
            assert!(status["text"].as_str().unwrap().contains(cause), "localized deletion failure {layout}/{}: {status}", language.code());
            assert_eq!(status["saveState"], "unsaved");
            let explicit_retry = tr("Conversation deletion requires an explicit save retry in that photo's AI conversation.");
            assert!(
                h.painted_text().iter().any(|text| text == explicit_retry),
                "global toast must describe actual explicit recovery: {layout}/{}",
                language.code()
            );
            super::panel_tests::reachable(&mut h, "button:aiRetrySave", 1.0);
            if layout == "normal" {
                assert!(h.painted_text().iter().any(|text| text == cause), "normal pane must paint the failure cause");
            }
            if let Ok(dir) = std::env::var("UX13_SCREENSHOT_DIR") {
                assert_eq!(
                    h.request(
                        "ui.screenshot",
                        json!({"path":format!("{dir}/failure-{layout}-{}.png",language.code()),"headless":true}),
                        Duration::from_secs(30)
                    )["ok"],
                    true
                );
            }
        }
    }
}

#[test]
fn acknowledged_clear_shows_independent_cleanup_debt_without_unsaved_history() {
    use crate::{
        headless::Headless,
        i18n::{Locale, tr},
    };
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    for language in [Locale::En, Locale::ZhHans] {
        let fail = Arc::new(AtomicBool::new(false));
        let mut session = Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(MemStore::new()),
                    files: Box::new(ClearFiles { memory: MemStore::new(), fail: fail.clone(), visible: false, cleanup: true }),
                    dir: "clear-cleanup-ui".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let photo = session.active().unwrap();
        session
            .save_companion_conversation(
                photo,
                json!({"schema_version":2,"messages":[{"role":"user","text":"Delete only after acknowledgement"}],"rounds":[],"round_records":[]}),
            )
            .unwrap();
        let mut app = LightcraftApp::new(session, Services::default());
        app.ui.language = language;
        app.ui.settings.gpu = false;
        let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        for _ in 0..5 {
            h.step();
        }
        let timeout = Duration::from_secs(10);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClear"}), timeout)["ok"], true);
        fail.store(true, Ordering::Relaxed);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClearConfirm"}), timeout)["ok"], true);
        h.app.session.persist().unwrap();
        for _ in 0..5 {
            h.step();
        }
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(status["saveState"], "saved");
        assert!(status["cleanupError"].is_string());
        assert!(!status["text"].as_str().unwrap().contains("Delete only after acknowledgement"));
        let notice = tr("File cleanup is still pending. Current conversation data is kept.");
        assert!(h.painted_text().iter().any(|text| text == notice), "independent localized cleanup feedback: {}", language.code());
    }
}
