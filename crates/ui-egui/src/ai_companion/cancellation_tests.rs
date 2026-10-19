use super::*;

#[test]
fn uncertain_creation_blocks_new_tasks_until_the_old_connection_workers_close() {
    let (mut app, mut state, photo, requests, events) = super::identity_tests::started();
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    events
        .try_send(network::Event::Response {
            submission: identity.submission_seq,
            round: String::new(),
            path: "/v1/rounds".into(),
            identity: identity.clone(),
            result: Err("response lost after delivery".into()),
        })
        .unwrap();
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(state.jobs.get(&photo).unwrap().stopped);
    assert!(state.connection.as_ref().unwrap().is_closing());
    assert_eq!(app.session.companion_task_photo(), Some(photo));
    assert!(state.start(&mut app).is_err());
    assert!(requests.try_recv().is_err());
    // The real transport sets alive only after joining its request worker.
    state.connection.as_ref().unwrap().alive.store(false, Ordering::Relaxed);
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(state.jobs.is_empty());
    assert!(app.session.companion_task_photo().is_none());
}

#[test]
fn backend_photo_busy_has_only_an_explicit_fresh_manual_retry() {
    let (mut app, mut state, photo, requests, events) = super::identity_tests::started();
    let previous = state.jobs.get(&photo).unwrap().identity.clone();
    let mut error: network::RequestFailure = "photo_busy; traceId=local-fixture".into();
    error.rejected = true;
    error.code = Some("photo_busy".into());
    error.trace = Some("local-fixture".into());
    events
        .try_send(network::Event::Response {
            submission: previous.submission_seq,
            round: String::new(),
            path: "/v1/rounds".into(),
            identity: previous.clone(),
            result: Err(error),
        })
        .unwrap();
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(!state.jobs.contains_key(&photo));
    assert!(state.connection.as_ref().is_some_and(|c| !c.is_closing()));
    assert!(state.conversations.get(&photo).unwrap().notice.contains("previous task is still finishing"));
    assert!(!state.conversations.get(&photo).unwrap().notice.contains("photo_busy"));
    assert!(requests.try_recv().is_err(), "no automatic retry or resume");
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(requests.try_recv().is_err());
    state.start(&mut app).unwrap();
    let fresh = requests.try_recv().unwrap();
    assert_eq!(fresh.path, "/v1/rounds");
    assert_ne!(fresh.identity.local_round_id, previous.local_round_id);
    assert_ne!(fresh.identity.submission_seq, previous.submission_seq);
    assert!(fresh.body.get("plan_id").is_none());
}

fn full_production_feedback_barrier(route: &str, prior_dirty: bool) {
    use lightcraft_catalog::{MemStore, Store};
    use std::io;
    struct BarrierStore {
        inner: MemStore,
        armed: Arc<AtomicBool>,
        painted: Arc<AtomicBool>,
        fail: Arc<AtomicBool>,
        premature: Arc<AtomicBool>,
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Store for BarrierStore {
        fn read(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
            self.inner.read(name)
        }
        fn read_limited(&mut self, name: &str, max: u64) -> io::Result<Option<Vec<u8>>> {
            self.inner.read_limited(name, max)
        }
        fn read_range(&mut self, name: &str, offset: u64, length: usize) -> io::Result<Vec<u8>> {
            self.inner.read_range(name, offset, length)
        }
        fn remove(&mut self, name: &str) -> io::Result<()> {
            self.inner.remove(name)
        }
        fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
            if name.starts_with("companion/") {
                if self.armed.swap(false, Ordering::Relaxed) {
                    if !self.painted.load(Ordering::Relaxed) {
                        self.premature.store(true, Ordering::Relaxed);
                        return Err(io::Error::other("archive I/O before feedback frame"));
                    }
                    self.entered.send(()).unwrap();
                    self.release.recv_timeout(Duration::from_secs(3)).expect("storage barrier was not released");
                }
                if self.fail.load(Ordering::Relaxed) {
                    return Err(io::Error::other("fixture disk unavailable"));
                }
            }
            self.inner.write_atomic(name, bytes)
        }
        fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
            self.inner.append(name, bytes)
        }
        fn truncate(&mut self, name: &str, length: u64) -> io::Result<()> {
            self.inner.truncate(name, length)
        }
        fn describe(&self) -> String {
            "UI feedback storage barrier".into()
        }
    }
    {
        let catalog = MemStore::new();
        let files = MemStore::new();
        let armed = Arc::new(AtomicBool::new(false));
        let painted = Arc::new(AtomicBool::new(false));
        let fail = Arc::new(AtomicBool::new(false));
        let premature = Arc::new(AtomicBool::new(false));
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                lightcraft_engine::library::LibraryStores {
                    catalog: Box::new(catalog.clone()),
                    files: Box::new(BarrierStore {
                        inner: files.clone(),
                        armed: armed.clone(),
                        painted: painted.clone(),
                        fail: fail.clone(),
                        premature: premature.clone(),
                        entered: entered_tx,
                        release: release_rx,
                    }),
                    dir: "feedback-fixture".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let mut app = LightcraftApp::new(session, crate::Services::default());
        let photo = app.session.active().unwrap();
        let (connection, requests, _) = network::test_connection();
        app.ai = State { open: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
        let mut state = std::mem::take(&mut app.ai);
        state.start(&mut app).unwrap();
        requests.try_recv().unwrap();
        let identity = state.jobs.get(&photo).unwrap().identity.clone();
        state.server(&mut app, &egui::Context::default(), "round.state", &json!({"round_id":"round","payload":{"submission_seq":identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":identity.scope.library_generation},"state":"queued"}})).unwrap();
        let gate = state.jobs.get(&photo).unwrap().cancel.clone();
        app.ai = state;
        app.run("ai.open", json!({})).unwrap();
        app.ui.settings.gpu = false;
        let (control_tx, control_rx) = mpsc::channel();
        app = app.with_control(control_rx);
        // Real egui pointer input in the app's headless surface, never OS input.
        let mut view = crate::headless::HeadlessView::new();
        let raw = |events| crate::headless::HeadlessView::raw_input(egui::vec2(1000.0, 700.0), 1.0, 10.0, events);
        for _ in 0..2 {
            view.run(raw(vec![]), |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
        }
        let widget = "button:aiStop";
        let pos = app.widgets.iter().find(|(id, _)| id == widget).unwrap().1.center();
        view.run(raw(vec![egui::Event::PointerMoved(pos)]), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        view.run(
            raw(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
            ]),
            |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
        );
        if prior_dirty {
            fail.store(true, Ordering::Relaxed);
            let mut state = std::mem::take(&mut app.ai);
            assert!(state.save(&mut app, photo).is_err());
            app.ai = state;
        }
        let began = Instant::now();
        let (request, response) = crate::ControlRequest::new("ui.menu.invoke", json!({"command":"ai.stop"}));
        if route == "command" {
            fail.store(true, Ordering::Relaxed);
            armed.store(true, Ordering::Relaxed);
            control_tx.send(request).unwrap();
            view.run(raw(vec![]), |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            assert_eq!(response.try_recv().unwrap()["ok"], true);
        } else {
            let mut passes = 0;
            view.run(
                raw(vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }]),
                |ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                    if passes == 0 {
                        assert!(app.ai.jobs.is_empty(), "the real widget must cancel before the discard barrier is armed");
                        armed.store(true, Ordering::Relaxed);
                        ui.ctx().request_discard("deterministic cancellation feedback rerun");
                    }
                    passes += 1;
                },
            );
            assert!(passes >= 2);
        }
        assert!(!premature.load(Ordering::Relaxed), "archive I/O ran before the cancellation feedback frame completed");
        assert!(began.elapsed() < Duration::from_secs(1), "stop feedback must not wait for disk");
        assert!(gate.load(Ordering::Relaxed));
        assert!(app.ai.jobs.is_empty());
        let painted_text = view.painted_text();
        assert!(painted_text.iter().any(|t| t.contains("Keeping the latest complete result")), "route={route}; painted={painted_text:?}");
        assert!(entered_rx.try_recv().is_err(), "no storage started in the feedback frame");
        if let Ok(dir) = std::env::var("UX18_FEEDBACK_SCREENSHOT_DIR") {
            let frame = view.paint(&std::collections::HashMap::new());
            let image = lightcraft_raster::Rgba8::from_fn(frame.size[0], frame.size[1], |x, y| frame.pixels[y * frame.size[0] + x].to_array());
            let bytes = lightcraft_engine::export::encode_image(
                &image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(format!("{route}-feedback.png")), bytes).unwrap();
        }
        painted.store(true, Ordering::Relaxed);
        let monitor = std::thread::spawn(move || {
            entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            release_tx.send(()).unwrap();
        });
        view.run(raw(vec![]), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        monitor.join().unwrap();
        assert!(app.ai.pending_local_saves.contains(&photo));
        assert!(app.ai.local_save_retry_at.is_some());
        assert!(app.ai.text.contains("fixture disk unavailable"));
        assert!(gate.load(Ordering::Relaxed));
        fail.store(false, Ordering::Relaxed);
        app.ai.local_save_retry_at = None; // advance only the retry barrier, never replay an edit/request.
        view.run(raw(vec![]), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        assert!(app.ai.pending_local_saves.is_empty());
        assert!(app.ai.jobs.is_empty());
        assert!(gate.load(Ordering::Relaxed));
        assert!(requests.try_recv().unwrap().path.ends_with("/stop"));
        assert!(requests.try_recv().is_err(), "storage retry never creates a new network task");
        app.session.close_library().unwrap();
        let mut reopened = lightcraft_engine::Session::new();
        reopened
            .open_library_in(
                lightcraft_engine::library::LibraryStores {
                    catalog: Box::new(catalog),
                    files: Box::new(files),
                    dir: "feedback-fixture".into(),
                    on_disk: false,
                },
                false,
            )
            .unwrap();
        let saved = reopened.load_companion_conversation(photo).unwrap().unwrap();
        assert_eq!(saved["round_records"][0]["local_terminal"], "cancelled");
    }
}

#[test]
fn full_app_dirty_retry_cannot_block_stop_feedback_on_discard() {
    full_production_feedback_barrier("stop", true);
}
#[test]
fn full_app_control_stop_defers_storage_until_its_feedback_frame_completes() {
    full_production_feedback_barrier("command", false);
}

#[test]
fn cancelled_job_removed_from_active_map_still_flushes_before_library_change() {
    let (mut app, mut state, photo, _, _) = super::identity_tests::started();
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    state.server(&mut app, &egui::Context::default(), "round.state", &json!({"round_id":"round","payload":{"submission_seq":identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":identity.scope.library_generation},"state":"queued"}})).unwrap();
    state.pending_local_saves.insert(photo);
    state.stop(photo).unwrap();
    assert!(state.jobs.is_empty());
    assert!(state.pending_local_saves.contains(&photo));
    app.ai = state;
    before_library_change(&mut app).unwrap();
    assert!(app.ai.pending_local_saves.is_empty());
    assert_eq!(app.session.photo_companion(photo).unwrap().unwrap()["round_records"][0]["local_terminal"], "cancelled");
}
