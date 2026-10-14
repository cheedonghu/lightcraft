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
    app.ai.next_connection = Some(connection);
    let mut h = crate::headless::Headless::new(app, [1100., 750.], 1.);
    h.app.run("ai.open", json!({})).unwrap();
    h.step();
    h.step();
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), Duration::from_secs(5))["ok"], true);
    h.app.run("ai.start", json!({"prompt":"new explicit request"})).unwrap();
    let status = native_history_tests::wait_idle(&mut h, &requests);
    assert!(status["text"].as_str().unwrap().contains("service limit"), "{status}");
    assert!(requests.try_recv().is_err());
    // The failed local attempt may be recorded; the entire original prefix stays intact.
    let mut cursor = 0;
    let mut retained = Vec::new();
    while cursor < before.message_count {
        let page = h.app.session.companion_messages_bounded(&before, cursor, 128, 32 * 1024 * 1024).unwrap();
        retained.extend(page.records);
        cursor = page.next_cursor.unwrap_or(before.message_count);
    }
    assert_eq!(retained, messages);
    assert_eq!(h.app.session.companion_messages_page(photo, 1024, 1).unwrap().records[0]["text"], "message 1024");
    assert!(h.app.session.companion_task_photo().is_none());
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
        fn retention_namespace(&self) -> Option<String> {
            self.files.retention_namespace()
        }
        fn conservative_retention(&self) -> bool {
            self.files.conservative_retention()
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
            h.size = egui::vec2(620.0, 520.0);
            for _ in 0..4 {
                h.step();
            }
            assert!(h.painted_text().iter().any(|s| s == language.tr("Older AI history")));
            let response =
                h.request("ui.screenshot", json!({"path":format!("{dir}/history-narrow-{}.png",language.code()),"headless":true}), timeout);
            assert_eq!(response["ok"], true, "{response}");
        }
    }
}

#[test]
fn async_preflight_enqueue_failure_releases_the_local_lease_without_uncertain_admission() {
    let mut files = MemStore::new();
    files.background = true;
    let mut app = app(Box::new(files));
    let photo = app.session.active().unwrap();
    app.session
        .save_companion_tail(photo, None, &[json!({"role":"user","text":"old exact history"})], &[], &[], &json!({"schema_version":2}))
        .unwrap();
    let initial = app.session.develop_of(photo).unwrap();
    let (connection, requests, _) = network::test_connection();
    let mut state = State { consent: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
    state.paging.alive.store(true, Ordering::Relaxed);
    state.start(&mut app).unwrap();
    let identity = state.jobs[&photo].identity.clone();
    let cancel = state.jobs[&photo].cancel.clone();
    assert!(requests.try_recv().is_err());
    for _ in 0..8 {
        state
            .connection
            .as_ref()
            .unwrap()
            .requests
            .try_send(network::Request {
                path: "/fixture/full-queue".into(),
                body: Value::Null,
                submission: identity.submission_seq,
                round: String::new(),
                identity: identity.clone(),
                cancel: cancel.clone(),
            })
            .unwrap();
    }
    let message = state.rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let error = state.prepared(&mut app, &egui::Context::default(), message).unwrap_err();
    state.fail_one(&mut app, photo, &error);
    assert!(state.jobs.is_empty());
    assert!(app.session.companion_task_photo().is_none());
    assert!(cancel.load(Ordering::Relaxed));
    assert_eq!(app.session.develop_of(photo).unwrap(), initial);
    assert_eq!(state.conversations[&photo].records[0]["local_terminal"], "failed");
    for request in requests.try_iter() {
        assert_ne!(request.path, "/v1/rounds");
    }
}

#[test]
fn retired_history_worker_with_lost_completion_releases_the_local_lease() {
    let mut files = MemStore::new();
    files.background = true;
    let mut app = app(Box::new(files));
    let photo = app.session.active().unwrap();
    app.session
        .save_companion_tail(photo, None, &[json!({"role":"user","text":"old exact history"})], &[], &[], &json!({"schema_version":2}))
        .unwrap();
    let (connection, requests, _) = network::test_connection();
    let mut state = State { consent: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
    state.paging.alive.store(true, Ordering::Relaxed);
    state.start(&mut app).unwrap();
    // Model a disconnected/full completion channel after the worker has really retired.
    let _ = state.rx.recv_timeout(Duration::from_secs(10)).unwrap();
    state.jobs[&photo].cancel.store(true, Ordering::Relaxed);
    state.workers.insert(photo, Arc::new(AtomicBool::new(false)));
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(state.jobs.is_empty());
    assert!(app.session.companion_task_photo().is_none());
    assert!(requests.try_recv().is_err());
    assert_eq!(state.conversations[&photo].records[0]["local_terminal"], "failed");
}

#[test]
fn shared_tools_mixed_continuation_renders_old_fragment_before_newer_record_and_current_tail() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        for current_tail in [false, true] {
            let mut app = app(Box::new(MemStore::new()));
            app.ui.language = language;
            let photo = app.session.active().unwrap();
            let old = format!("{}OLD_MESSAGE_END", "A".repeat(270_000));
            let mut messages = vec![json!({"role":"assistant","text":old})];
            if !current_tail {
                messages.push(json!({"role":"user","text":"NEWER_MESSAGE"}));
            }
            app.session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
            let mut h = crate::headless::Headless::new(app, [1000.0, 680.0], 1.0);
            h.app.run("ai.open", json!({})).unwrap();
            for _ in 0..4 {
                h.step();
            }
            assert!(h.app.ai.conversations[&photo].view.as_ref().unwrap().messages.is_empty());
            if current_tail {
                h.app.ai.conversations.get_mut(&photo).unwrap().messages.push(json!({"role":"user","text":"NEWER_MESSAGE"}));
            }
            let timeout = Duration::from_secs(10);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryNewer"}), timeout)["ok"], true);
            for _ in 0..3 {
                h.step();
            }
            let text = h.app.run("ai.status", json!({})).unwrap()["text"].as_str().unwrap().to_owned();
            assert!(text.find("OLD_MESSAGE_END").unwrap() < text.find("NEWER_MESSAGE").unwrap(), "{current_tail}: {text}");
            h.request("ui.hoverWidget", json!({"id":"ai:messages"}), timeout);
            h.request("ui.scroll", json!({"dy":-100000.0}), timeout);
            for _ in 0..12 {
                h.step();
            }
            let painted = h.painted_text().join("\n");
            assert!(painted.find("OLD_MESSAGE_END").unwrap() < painted.find("NEWER_MESSAGE").unwrap());
            assert!(h.app.ai.conversations[&photo].view.as_ref().unwrap().bytes <= 256 * 1024);
            if let Ok(dir) = std::env::var("UX08_SCREENSHOT_DIR") {
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
                        json!({"path":format!("{dir}/fragment-{}-{}.png",language.code(),current_tail),"headless":true}),
                        timeout
                    )["ok"],
                    true
                );
            }
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryOlder"}), timeout)["ok"], true);
            for _ in 0..3 {
                h.step();
            }
            assert_eq!(h.app.ai.conversations[&photo].view.as_ref().unwrap().cursor.offset, 0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryNewer"}), timeout)["ok"], true);
            for _ in 0..3 {
                h.step();
            }
            let text = h.app.run("ai.status", json!({})).unwrap()["text"].as_str().unwrap().to_owned();
            assert!(text.find("OLD_MESSAGE_END").unwrap() < text.find("NEWER_MESSAGE").unwrap());
        }
    }
}

#[test]
fn cold_native_history_command_and_rapid_requests_keep_the_latest_explicit_cursor() {
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
        fn read(&mut self, n: &str) -> std::io::Result<Option<Vec<u8>>> {
            self.files.read(n)
        }
        fn read_limited(&mut self, n: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
            self.wait(n);
            self.files.read_limited(n, max)
        }
        fn read_range(&mut self, n: &str, o: u64, l: usize) -> std::io::Result<Vec<u8>> {
            self.wait(n);
            self.files.read_range(n, o, l)
        }
        fn write_atomic(&mut self, n: &str, b: &[u8]) -> std::io::Result<()> {
            self.files.write_atomic(n, b)
        }
        fn remove(&mut self, n: &str) -> std::io::Result<()> {
            self.files.remove(n)
        }
        fn append(&mut self, n: &str, b: &[u8]) -> std::io::Result<()> {
            self.files.append(n, b)
        }
        fn truncate(&mut self, n: &str, l: u64) -> std::io::Result<()> {
            self.files.truncate(n, l)
        }
        fn describe(&self) -> String {
            "blocked native page".into()
        }
        fn retention_namespace(&self) -> Option<String> {
            self.files.retention_namespace()
        }
        fn conservative_retention(&self) -> bool {
            self.files.conservative_retention()
        }
        fn background_writer(&self) -> Option<Box<dyn Store>> {
            Some(Box::new(Self { files: self.files.clone(), gate: self.gate.clone(), reader: true }))
        }
    }
    for blocked_default in [false, true] {
        let (entered, entered_rx) = mpsc::sync_channel(1);
        let (release, release_rx) = mpsc::sync_channel(1);
        let gate = Arc::new(Gate { entered, release: Mutex::new(release_rx), used: AtomicBool::new(false) });
        let mut app = app(Box::new(ReaderStore { files: MemStore::new(), gate, reader: false }));
        let photo = app.session.active().unwrap();
        let messages: Vec<_> = (0..260).map(|n| json!({"role":"assistant","text":format!("history {n}")})).collect();
        app.session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
        if blocked_default {
            // Force the initial newest page to be inside native I/O before issuing
            // the actual explicit command; no history page has been loaded yet.
            app.run("ai.open", json!({})).unwrap();
            super::poll(&mut app, &egui::Context::default());
            entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        app.run("ai.history", json!({"photo":photo.0,"record":0,"offset":0})).unwrap();
        if !blocked_default {
            // On a completely cold command, cancellation may retire the default
            // worker before it enters I/O. Dispatch its queued explicit replacement.
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if entered_rx.try_recv().is_ok() {
                    break;
                }
                assert!(Instant::now() < deadline, "native page never entered I/O");
                super::poll(&mut app, &egui::Context::default());
                std::thread::yield_now();
            }
        }
        app.run("ai.open", json!({})).unwrap();
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.run("ai.status", json!({})).unwrap()["text"].as_str().unwrap().starts_with("history 0\n") && Instant::now() < deadline {
            super::poll(&mut app, &egui::Context::default());
            std::thread::yield_now();
        }
        assert!(app.run("ai.status", json!({})).unwrap()["text"].as_str().unwrap().starts_with("history 0\n"));
        // Queue several desired pages before accepting any already-completed result.
        app.run("ai.history", json!({"photo":photo.0,"record":32})).unwrap();
        app.run("ai.history", json!({"photo":photo.0,"record":64})).unwrap();
        app.run("ai.history", json!({"photo":photo.0,"record":96})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.run("ai.status", json!({})).unwrap()["text"].as_str().unwrap().starts_with("history 96\n") && Instant::now() < deadline {
            super::poll(&mut app, &egui::Context::default());
            let status = app.run("ai.status", json!({})).unwrap();
            let text = status["text"].as_str().unwrap();
            assert!(text.starts_with("history 0\n") || text.starts_with("history 96\n"), "obsolete navigation result became visible: {text}");
            std::thread::yield_now();
        }
        assert!(app.run("ai.status", json!({})).unwrap()["text"].as_str().unwrap().starts_with("history 96\n"));
    }
}

#[test]
fn history_controls_remain_reachable_in_supported_short_floating_and_stacked_tools() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        for (stacked, height) in [(false, 120.0), (true, 120.0), (false, 240.0), (true, 240.0)] {
            let mut app = app(Box::new(MemStore::new()));
            app.ui.language = language;
            let photo = app.session.active().unwrap();
            let messages: Vec<_> = (0..260).map(|n| json!({"role":"assistant","text":format!("historic message {n}")})).collect();
            app.session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
            let mut h = crate::headless::Headless::new(app, [1000.0, 680.0], 1.0);
            super::panel_tests::short_tools(&mut h, stacked, height);
            let timeout = Duration::from_secs(10);
            super::panel_tests::reachable(&mut h, "button:aiHistoryOlder", -1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryOlder"}), timeout)["ok"], true);
            for _ in 0..3 {
                h.step();
            }
            assert_eq!(h.app.ai.conversations[&photo].view.as_ref().unwrap().cursor.record, 196);
            super::panel_tests::reachable(&mut h, "button:aiHistoryNewer", -1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryNewer"}), timeout)["ok"], true);
            for _ in 0..3 {
                h.step();
            }
            assert_eq!(h.app.ai.conversations[&photo].view.as_ref().unwrap().cursor.record, 228);
            super::panel_tests::short_snapshot(
                &mut h,
                &format!("history-short-{}-{}-{}", if stacked { "stack" } else { "float" }, height as usize, language.code()),
            );
        }
    }
}

#[test]
fn public_poll_preserves_initial_correction_feedback_and_stop_before_completed_result_consumption() {
    fn outgoing(h: &mut crate::headless::Headless, requests: &mpsc::Receiver<network::Request>, suffix: &str) -> network::Request {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            h.step();
            if let Ok(request) = requests.try_recv() {
                assert!(request.path.ends_with(suffix), "unexpected {}", request.path);
                assert_eq!(h.app.run("ai.status", json!({})).unwrap()["workerBusy"], false, "feedback must wait for its producer to retire");
                return request;
            }
            assert!(Instant::now() < deadline, "{suffix} was never emitted");
            std::thread::yield_now();
        }
    }
    fn phase(events: &mpsc::SyncSender<network::Event>, request: &network::Request, state: &str) {
        events.send(network::Event::Server{kind:"round.state".into(),data:json!({"round_id":format!("public-round-{}",request.identity.submission_seq),"payload":{"submission_seq":request.identity.submission_seq,"photo_ref":{"library_generation":request.identity.scope.library_generation,"photo_id":request.identity.scope.photo.0},"state":state}})}).unwrap();
    }
    fn plan(events: &mpsc::SyncSender<network::Event>, request: &network::Request, revision: u64, id: &str, exposure: f64) {
        events.send(network::Event::Server{kind:"message.completed".into(),data:json!({"round_id":format!("public-round-{}",request.identity.submission_seq),"stage":id,"payload":{"message_id":id,"model_call_id":id,"tool_calls":[{"id":id,"name":"apply_edit_plan","arguments":{"operations":[{"type":"set_controls","values":{"light.exposure":exposure}}]}}]}})}).unwrap();
        events.send(network::Event::Server{kind:"edit.plan".into(),data:json!({"round_id":format!("public-round-{}",request.identity.submission_seq),"payload":{"plan_id":id,"tool_call_id":id,"capability_version":"lightcraft-v2","expected_revision":revision,"operations":[{"type":"set_controls","values":{"light.exposure":exposure}}]}})}).unwrap();
    }
    let mut app = app(Box::new(MemStore::new()));
    let photo = app.session.active().unwrap();
    let (connection, requests, events) = network::test_connection();
    app.ai.connection = Some(connection);
    app.ai.generation = app.session.library_generation();
    let mut h = crate::headless::Headless::new(app, [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    for _ in 0..4 {
        h.step();
    }
    let timeout = Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiStart"}), timeout)["ok"], true);
    let creation = requests.try_recv().unwrap();
    assert_eq!(creation.path, "/v1/rounds");
    phase(&events, &creation, "awaiting_input");
    outgoing(&mut h, &requests, "/input");
    for (id, state, exposure) in [("initial", "awaiting_initial_execution", 0.4), ("correction", "awaiting_correction_execution", 0.2)] {
        phase(&events, &creation, state);
        plan(&events, &creation, h.app.session.photo_revision(photo), id, exposure);
        let feedback = outgoing(&mut h, &requests, "/execution");
        assert_eq!(feedback.body["outcome"], "succeeded");
        assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, exposure);
        events
            .send(network::Event::Response {
                submission: 0,
                round: feedback.round.clone(),
                path: feedback.path,
                identity: feedback.identity,
                result: Ok(json!({"accepted":true,"_client_execution_plan_id":id})),
            })
            .unwrap();
    }
    events
        .send(network::Event::Server {
            kind: "round.ended".into(),
            data: json!({"round_id":format!("public-round-{}",creation.identity.submission_seq),"payload":{"terminal_state":"completed"}}),
        })
        .unwrap();
    for _ in 0..3 {
        h.step();
    }
    h.app.run("ai.start", json!({"prompt":"keep the previous complete result"})).unwrap();
    let creation = requests.try_recv().unwrap();
    phase(&events, &creation, "awaiting_input");
    outgoing(&mut h, &requests, "/input");
    phase(&events, &creation, "awaiting_initial_execution");
    plan(&events, &creation, h.app.session.photo_revision(photo), "held", 0.6);
    super::poll(&mut h.app, &egui::Context::default());
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.2);
    // Status does not consume completion messages: let the real producer finish
    // while holding UI consumption, then cancel through the actual command.
    let deadline = Instant::now() + Duration::from_secs(30);
    while h.app.run("ai.status", json!({})).unwrap()["workerBusy"] == true {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    h.app.run("ai.stop", json!({})).unwrap();
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["busy"], false);
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.2);
    for request in requests.try_iter() {
        assert!(!request.path.ends_with("/execution"), "cancelled held completion cannot publish feedback");
    }
}

#[test]
fn narrow_tools_paints_complete_paging_labels_and_navigates_a_long_message() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let mut app = app(Box::new(MemStore::new()));
        app.ui.language = language;
        let photo = app.session.active().unwrap();
        app.session
            .save_companion_tail(
                photo,
                None,
                &[json!({"role":"assistant","text":format!("{}OLD_MESSAGE_END","A".repeat(270_000))}), json!({"role":"user","text":"NEWER_MESSAGE"})],
                &[],
                &[],
                &json!({"schema_version":2}),
            )
            .unwrap();
        let mut h = crate::headless::Headless::new(app, [620.0, 520.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        for _ in 0..4 {
            h.step();
        }
        let tools = h.app.widgets.iter().find(|(id, _)| id == "panel:right_panel").unwrap().1;
        for label in ["Older AI history", "Newer AI history"] {
            let shape = h
                .view
                .painted_shapes()
                .into_iter()
                .find_map(|s| match s {
                    egui::epaint::Shape::Text(text) if text.galley.job.text == language.tr(label) => Some(text),
                    _ => None,
                })
                .unwrap();
            assert!(tools.contains_rect(shape.visual_bounding_rect()), "{label} is clipped outside Tools");
        }
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryNewer"}), Duration::from_secs(10))["ok"], true);
        for _ in 0..3 {
            h.step();
        }
        let status = h.app.run("ai.status", json!({})).unwrap();
        let text = status["text"].as_str().unwrap();
        assert!(text.find("OLD_MESSAGE_END").unwrap() < text.find("NEWER_MESSAGE").unwrap());
    }
}

#[test]
fn compact_tools_history_is_visibly_readable_after_scrolling_and_older_newer_navigation() {
    fn visible_literal(h: &crate::headless::Headless, literal: &str) -> bool {
        let Some((_, viewport)) = h.app.widgets.iter().find(|(id, _)| id == "ai:messages") else { return false };
        let tools = h.app.widgets.iter().find(|(id, _)| id == "panel:right_panel").unwrap().1;
        if viewport.height() < 14.0 || !tools.contains_rect(*viewport) {
            return false;
        }
        h.view.painted_shapes().iter().any(|shape| match shape {
            egui::epaint::Shape::Text(text) if text.opacity_factor > 0.0 => {
                text.galley.rows.iter().any(|row| row.text() == literal && viewport.contains_rect(row.rect().translate(text.pos.to_vec2())))
            }
            _ => false,
        })
    }
    fn scroll_to_literal(h: &mut crate::headless::Headless, literal: &str) -> bool {
        for _ in 0..60 {
            if visible_literal(h, literal) {
                return true;
            }
            let composer = h.app.widgets.iter().find(|(id, _)| id == "ai:composer").unwrap().1;
            h.app.synthetic.push(egui::Event::PointerMoved(egui::pos2(composer.right() - 2.0, composer.center().y)));
            h.app.synthetic.push(egui::Event::MouseWheel {
                phase: egui::TouchPhase::Move,
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -35.0),
                modifiers: egui::Modifiers::NONE,
            });
            for _ in 0..5 {
                h.step();
            }
        }
        visible_literal(h, literal)
    }
    fn scroll_to_control(h: &mut crate::headless::Headless, id: &str) {
        // Readable nested history now participates in wheel scrolling too;
        // traverse both the message and outer compact content to its controls.
        for _ in 0..60 {
            if let Some((_, target)) = h.app.widgets.iter().find(|(key, _)| key == id)
                && target.height() >= 12.0
            {
                let tools = h.app.widgets.iter().find(|(key, _)| key == "panel:right_panel").unwrap().1;
                assert!(tools.contains_rect(*target));
                return;
            }
            let composer = h.app.widgets.iter().find(|(key, _)| key == "ai:composer").unwrap().1;
            h.app.synthetic.push(egui::Event::PointerMoved(egui::pos2(composer.right() - 2.0, composer.center().y)));
            h.app.synthetic.push(egui::Event::MouseWheel {
                phase: egui::TouchPhase::Move,
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 35.0),
                modifiers: egui::Modifiers::NONE,
            });
            for _ in 0..5 {
                h.step();
            }
        }
        panic!("{id} is not visibly reachable after scrolling compact history");
    }
    let mut failures = Vec::new();
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        for (stacked, height) in [(false, 120.0), (true, 120.0), (false, 240.0), (true, 240.0)] {
            let mut app = app(Box::new(MemStore::new()));
            app.ui.language = language;
            let photo = app.session.active().unwrap();
            let messages: Vec<_> = (0..260).map(|n| json!({"role":"assistant","text":format!("VISIBLE_HISTORY_{n}")})).collect();
            app.session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
            let mut h = crate::headless::Headless::new(app, [1000.0, 680.0], 1.0);
            super::panel_tests::short_tools(&mut h, stacked, height);
            if !scroll_to_literal(&mut h, "VISIBLE_HISTORY_259") {
                failures.push(format!("{} stacked={stacked} height={height}: newest text never visible", language.code()));
                continue;
            }
            let timeout = Duration::from_secs(10);
            scroll_to_control(&mut h, "button:aiHistoryOlder");
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryOlder"}), timeout)["ok"], true);
            for _ in 0..3 {
                h.step();
            }
            assert!(h.app.run("ai.status", json!({})).unwrap()["text"].as_str().unwrap().starts_with("VISIBLE_HISTORY_196\n"));
            if !scroll_to_literal(&mut h, "VISIBLE_HISTORY_227") {
                failures.push(format!("{} stacked={stacked} height={height}: older text never visible", language.code()));
                continue;
            }
            scroll_to_control(&mut h, "button:aiHistoryNewer");
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryNewer"}), timeout)["ok"], true);
            for _ in 0..3 {
                h.step();
            }
            assert!(h.app.run("ai.status", json!({})).unwrap()["text"].as_str().unwrap().starts_with("VISIBLE_HISTORY_228\n"));
            if !scroll_to_literal(&mut h, "VISIBLE_HISTORY_259") {
                failures.push(format!("{} stacked={stacked} height={height}: newer text never visible", language.code()));
                continue;
            }
            super::panel_tests::short_snapshot(
                &mut h,
                &format!("compact-readable-{}-{}-{}", language.code(), if stacked { "stack" } else { "float" }, height as usize),
            );
        }
    }
    assert!(failures.is_empty(), "Compact history is not readable: {failures:?}");
}
