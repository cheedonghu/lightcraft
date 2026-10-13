//! Actual widgets, backend events and Session persistence through varying Store
//! adapters. No injected render result or private worker-alive manipulation.
use super::*;
use lightcraft_catalog::{MemStore, Store};
use lightcraft_engine::library::LibraryStores;

struct Storage {
    memory: MemStore,
    failed: Arc<AtomicBool>,
    catalog: bool,
}
impl Store for Storage {
    fn read(&mut self, k: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.memory.read(k)
    }
    fn read_limited(&mut self, k: &str, n: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.memory.read_limited(k, n)
    }
    fn read_range(&mut self, k: &str, o: u64, n: usize) -> std::io::Result<Vec<u8>> {
        self.memory.read_range(k, o, n)
    }
    fn write_atomic(&mut self, k: &str, b: &[u8]) -> std::io::Result<()> {
        if !self.catalog && k.ends_with("/HEAD.json") && self.failed.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("UX09 fixture disk unavailable"));
        }
        self.memory.write_atomic(k, b)
    }
    fn append(&mut self, k: &str, b: &[u8]) -> std::io::Result<()> {
        self.memory.append(k, b)?;
        if self.catalog && String::from_utf8_lossy(b).contains("\"op\":\"setDevelop\"") {
            // Arm the failing file adapter after the real core edit was logged.
            self.failed.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
    fn remove(&mut self, k: &str) -> std::io::Result<()> {
        self.memory.remove(k)
    }
    fn truncate(&mut self, k: &str, n: u64) -> std::io::Result<()> {
        self.memory.truncate(k, n)
    }
    fn describe(&self) -> String {
        "UX09 procedural storage failure".into()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
}
fn outgoing(h: &mut crate::headless::Headless, requests: &mpsc::Receiver<network::Request>, suffix: &str) -> network::Request {
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
#[test]
fn public_ai_result_stays_visible_and_unsaved_recovery_only_writes_storage() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let failed = Arc::new(AtomicBool::new(false));
        let catalog = MemStore::new();
        let files = MemStore::new();
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(Storage { memory: catalog.clone(), failed: failed.clone(), catalog: true }),
                    files: Box::new(Storage { memory: files.clone(), failed: failed.clone(), catalog: false }),
                    dir: "UX09-public-recovery".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let photo = session.active().unwrap();
        let mut app = LightcraftApp::new(session, crate::Services::default());
        app.ui.language = language;
        app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
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
        let creation = outgoing(&mut h, &requests, "/v1/rounds");
        let round = "UX09-public-round";
        let phase = |state: &str| {
            events.send(network::Event::Server{kind:"round.state".into(),data:json!({"round_id":round,"payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":creation.identity.scope.library_generation},"state":state}})}).unwrap()
        };
        phase("awaiting_input");
        outgoing(&mut h, &requests, "/input");
        phase("awaiting_initial_execution");
        events.send(network::Event::Server{kind:"message.completed".into(),data:json!({"round_id":round,"stage":"initial","payload":{"message_id":"UX09-message","model_call_id":"UX09-call","tool_calls":[{"id":"UX09-tool","name":"apply_edit_plan","arguments":{"operations":[{"type":"set_controls","values":{"light.exposure":0.4}}]}}]}})}).unwrap();
        events.send(network::Event::Server{kind:"edit.plan".into(),data:json!({"round_id":round,"payload":{"plan_id":"UX09-plan","tool_call_id":"UX09-tool","capability_version":"lightcraft-v2","expected_revision":h.app.session.photo_revision(photo),"operations":[{"type":"set_controls","values":{"light.exposure":0.4}}]}})}).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while h.app.session.develop_of(photo).unwrap().light.exposure != 0.4 || h.app.session.unsaved().is_none() {
            h.step();
            assert!(Instant::now() < deadline, "actual edited result and unsaved storage never appeared");
            std::thread::yield_now();
        }
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(status["saveState"], "unsaved");
        let expected = if language == crate::i18n::Locale::En {
            "Photo changes are kept. AI history is not saved; saving will retry."
        } else {
            "照片修改已保留。AI 对话记录尚未保存，将重试保存。"
        };
        assert!(status["text"].as_str().unwrap().contains(expected), "storage failure must give a truthful localized notice: {status}");
        for _ in 0..3 {
            h.step();
        }
        assert!(h.app.widgets.iter().any(|(id, _)| id == "indicator:unsaved"));
        if let Ok(dir) = std::env::var("UX09_SCREENSHOT_DIR") {
            let shot = h.request("ui.screenshot", json!({"path":format!("{dir}/actual-unsaved-{}.png",language.code()),"headless":true}), timeout);
            assert_eq!(shot["ok"], true, "{shot}");
        }
        while requests.try_recv().is_ok() {}
        let revision = h.app.session.photo_revision(photo);
        failed.store(false, Ordering::Relaxed);
        let recovery_deadline = Instant::now() + Duration::from_secs(6);
        while h.app.session.unsaved().is_some() {
            h.step();
            assert!(Instant::now() < recovery_deadline, "ordinary client frames must retry repaired storage");
            std::thread::sleep(Duration::from_millis(2));
        }
        h.step();
        let saved_status = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(saved_status["saveState"], "saved");
        let saved_notice = if language == crate::i18n::Locale::En {
            "AI history saved. Photo changes are kept."
        } else {
            "AI 对话记录已保存，照片修改已保留。"
        };
        assert!(
            saved_status["text"].as_str().unwrap().contains(saved_notice),
            "recovered history must not keep claiming it is unsaved: {saved_status}"
        );
        assert_eq!(h.app.session.photo_revision(photo), revision);
        assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.4);
        assert!(requests.try_recv().is_err(), "storage retry does not send any request or reexecute the edit");
        let saved = h.app.session.load_companion_conversation(photo).unwrap().unwrap();
        assert_eq!(saved["round_records"][0]["stages"][0]["execution_outcome"], "succeeded");
        assert_eq!(saved["round_records"][0]["recent_complete_result"]["light"]["exposure"], 0.4);
        if let Ok(dir) = std::env::var("UX09_SCREENSHOT_DIR") {
            let shot = h.request("ui.screenshot", json!({"path":format!("{dir}/actual-recovered-{}.png",language.code()),"headless":true}), timeout);
            assert_eq!(shot["ok"], true, "{shot}");
        }
        h.app.session.close_library().unwrap();
        let mut reopened = lightcraft_engine::Session::new();
        reopened
            .open_library_in(
                LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "UX09-public-recovery".into(), on_disk: false },
                false,
            )
            .unwrap();
        assert_eq!(reopened.develop_of(photo).unwrap().light.exposure, 0.4);
        assert_eq!(
            reopened.companion_round(photo, creation.identity.local_round_id.as_str()).unwrap().unwrap()["stages"][0]["execution_outcome"],
            "succeeded"
        );
    }
}

#[test]
fn public_poll_schedules_cleanup_after_the_last_ai_job_has_ended() {
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "UX09-cleanup-timer".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    for text in ["old", "current"] {
        session.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":text}],"rounds":[]})).unwrap();
    }
    let mut app = LightcraftApp::new(session, crate::Services::default());
    let ctx = egui::Context::default();
    for _ in 0..4 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
    }
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| super::poll(&mut app, ui.ctx()));
    output.textures_delta.clear();
    let delay = output.viewport_output.values().map(|v| v.repaint_delay).min().unwrap();
    assert!(delay <= Duration::from_secs(1), "cleanup needs a future frame even without an AI job; delay={delay:?}");
    assert_eq!(app.run("ai.status", json!({})).unwrap()["busy"], false);
}
