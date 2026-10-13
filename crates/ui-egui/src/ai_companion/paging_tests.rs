use super::*;
use lightcraft_catalog::{MemStore, Store};
use lightcraft_engine::companion::storage::HistoryCursor;
use lightcraft_engine::library::LibraryStores;

fn app(files: Box<dyn Store>) -> LightcraftApp {
    let mut session = lightcraft_engine::Session::new();
    session.open_library_in(LibraryStores { catalog: Box::new(MemStore::new()), files, dir: "paging-fixture".into(), on_disk: false }, true).unwrap();
    LightcraftApp::new(session, crate::Services::default())
}
#[test]
fn ui_loads_only_a_byte_bounded_page_and_can_read_all_older_history() {
    let mut app = app(Box::new(MemStore::new()));
    let photo = app.session.active().unwrap();
    let messages: Vec<_> = (0..260).map(|n| json!({"role":"user","text":format!("message {n}"),"local_message_id":format!("m{n}")})).collect();
    app.session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
    let mut state = State::default();
    state.load(&mut app, photo).unwrap();
    let conversation = &state.conversations[&photo];
    assert!(conversation.messages.is_empty());
    assert!(conversation.records.is_empty());
    assert_eq!(conversation.view.as_ref().unwrap().messages[0]["text"], "message 228");
    assert!(conversation.view.as_ref().unwrap().bytes <= 256 * 1024);
    state.read_page(&mut app, photo, HistoryCursor::default()).unwrap();
    assert_eq!(state.conversations[&photo].view.as_ref().unwrap().messages[0]["text"], "message 0");
    assert_eq!(state.conversations[&photo].view.as_ref().unwrap().messages.len(), 32);
    let before = app.session.companion_photo_header(photo).unwrap().unwrap();
    state.messages_append_fixture(photo, "new actual user record");
    state.save(&mut app, photo).unwrap();
    assert_eq!(app.session.companion_photo_header(photo).unwrap().unwrap().message_count, before.message_count + 1);
    assert_eq!(app.session.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "message 0");
    assert_eq!(app.session.companion_messages_page(photo, 260, 1).unwrap().records[0]["text"], "new actual user record");
}
impl State {
    fn messages_append_fixture(&mut self, photo: PhotoId, text: &str) {
        self.conversations.get_mut(&photo).unwrap().messages.push(json!({"role":"user","text":text}));
    }
}
#[test]
fn service_overflow_refuses_submission_and_retains_all_local_records() {
    let mut app = app(Box::new(MemStore::new()));
    let photo = app.session.active().unwrap();
    let messages: Vec<_> = (0..1025).map(|n| json!({"role":"user","text":format!("message {n}")})).collect();
    app.session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
    let before = app.session.companion_photo_header(photo).unwrap().unwrap();
    let (connection, requests, _) = network::test_connection();
    let mut state = State { consent: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
    assert!(state.start(&mut app).is_err());
    assert!(requests.try_recv().is_err());
    assert!(state.jobs.is_empty());
    assert_eq!(app.session.companion_photo_header(photo).unwrap().unwrap(), before);
    assert_eq!(app.session.companion_messages_page(photo, 1024, 1).unwrap().records[0]["text"], "message 1024");
    assert!(app.session.companion_task_photo().is_none());
}
#[test]
fn oversized_unicode_history_can_be_paged_forward_and_back_without_retaining_the_record() {
    let mut app = app(Box::new(MemStore::new()));
    let photo = app.session.active().unwrap();
    app.session
        .save_companion_tail(
            photo,
            None,
            &[json!({"role":"assistant","text":"自然修改完整历史".repeat(300_000)})],
            &[],
            &[],
            &json!({"schema_version":2}),
        )
        .unwrap();
    let mut state = State::default();
    state.load(&mut app, photo).unwrap();
    let next = state.conversations[&photo].view.as_ref().unwrap().next.unwrap();
    assert!(state.conversations[&photo].view.as_ref().unwrap().messages.is_empty());
    assert!(state.conversations[&photo].view.as_ref().unwrap().fragments[0].contains("自然修改完整历史"));
    state.read_page(&mut app, photo, next).unwrap();
    assert!(state.conversations[&photo].view.as_ref().unwrap().bytes <= 256 * 1024);
    state.read_page(&mut app, photo, HistoryCursor { record: 0, offset: 1 }).unwrap();
    assert!(state.conversations[&photo].view.as_ref().unwrap().next.is_some());
}

#[test]
fn cancelling_blocked_native_style_history_preflight_never_enqueues_creation_or_late_publication() {
    use std::sync::Mutex;
    struct Gate {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        used: AtomicBool,
    }
    struct ReaderStore {
        files: MemStore,
        gate: Arc<Gate>,
        reader: bool,
    }
    impl ReaderStore {
        fn wait(&self, name: &str) {
            if self.reader && name.contains("/blocks/") && !self.gate.used.swap(true, Ordering::Relaxed) {
                self.gate.entered.send(()).unwrap();
                self.gate.release.lock().unwrap().recv().unwrap();
            }
        }
    }
    impl Store for ReaderStore {
        fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
            self.files.read(name)
        }
        fn read_limited(&mut self, name: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
            self.wait(name);
            self.files.read_limited(name, max)
        }
        fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
            self.wait(name);
            self.files.read_range(name, offset, length)
        }
        fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            self.files.write_atomic(name, bytes)
        }
        fn remove(&mut self, name: &str) -> std::io::Result<()> {
            self.files.remove(name)
        }
        fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            self.files.append(name, bytes)
        }
        fn truncate(&mut self, name: &str, length: u64) -> std::io::Result<()> {
            self.files.truncate(name, length)
        }
        fn describe(&self) -> String {
            "native-style reader barrier".into()
        }
        fn background_writer(&self) -> Option<Box<dyn Store>> {
            Some(Box::new(Self { files: self.files.clone(), gate: self.gate.clone(), reader: true }))
        }
    }
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let gate = Arc::new(Gate { entered: entered_tx, release: Mutex::new(release_rx), used: AtomicBool::new(false) });
    let mut app = app(Box::new(ReaderStore { files: MemStore::new(), gate, reader: false }));
    let photo = app.session.active().unwrap();
    app.session
        .save_companion_tail(photo, None, &[json!({"role":"user","text":"old exact history"})], &[], &[], &json!({"schema_version":2}))
        .unwrap();
    let initial = app.session.develop_of(photo).unwrap();
    let (connection, requests, _) = network::test_connection();
    let mut state = State { consent: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
    // Suppress the independent read-only panel request so this barrier names the model worker.
    state.paging.alive.store(true, Ordering::Relaxed);
    let response = state.start(&mut app).unwrap();
    assert_eq!(response["started"], true);
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(requests.try_recv().is_err());
    assert_eq!(app.session.companion_task_photo(), Some(photo));
    let identity = state.jobs[&photo].identity.clone();
    state.stop(photo).unwrap();
    state.save(&mut app, photo).unwrap();
    assert!(state.jobs.is_empty());
    assert!(app.session.companion_task_photo().is_none());
    release_tx.send(()).unwrap();
    // A manually delivered old completion is equally rejected, even after consent returns.
    state.consent = true;
    state
        .prepared(
            &mut app,
            &egui::Context::default(),
            WorkerMessage::HistoryPrepared { identity, result: Ok(vec![json!({"role":"user","text":"late"})]) },
        )
        .unwrap();
    assert!(requests.try_recv().is_err());
    assert_eq!(app.session.develop_of(photo).unwrap(), initial);
    assert!(state.conversations[&photo].records[0]["local_terminal"] == "cancelled");
}

#[test]
fn bounded_history_buttons_show_older_and_newer_pages_in_both_languages() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let mut app = app(Box::new(MemStore::new()));
        let photo = app.session.active().unwrap();
        let messages: Vec<_> = (0..260)
            .map(|n| json!({"role":"assistant","text":format!("Historic message {n}: keep all original records and read only the requested page.")}))
            .collect();
        app.session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
        app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        app.ui.language = language;
        app.ui.settings.gpu = false;
        let mut h = crate::headless::Headless::new(app, [1000.0, 680.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        for _ in 0..4 {
            h.step();
        }
        assert!(h.painted_text().iter().any(|s| s == language.tr("Older AI history")));
        assert_eq!(h.app.ai.conversations[&photo].view.as_ref().unwrap().cursor.record, 228);
        let timeout = Duration::from_secs(10);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryOlder"}), timeout)["ok"], true);
        for _ in 0..3 {
            h.step();
        }
        assert_eq!(h.app.ai.conversations[&photo].view.as_ref().unwrap().cursor.record, 196);
        assert!(h.app.ai.conversations[&photo].messages.is_empty());
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryNewer"}), timeout)["ok"], true);
        for _ in 0..3 {
            h.step();
        }
        assert_eq!(h.app.ai.conversations[&photo].view.as_ref().unwrap().cursor.record, 228);
        if let Ok(dir) = std::env::var("UX08_SCREENSHOT_DIR") {
            let response = h.request("ui.screenshot", json!({"path":format!("{dir}/history-{}.png",language.code()),"headless":true}), timeout);
            assert_eq!(response["ok"], true, "{response}");
        }
    }
}
