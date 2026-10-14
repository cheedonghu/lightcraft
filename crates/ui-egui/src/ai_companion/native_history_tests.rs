//! Caller-observable native history submission checks, using temporary real files.
use super::*;
use lightcraft_catalog::{FsStore, MemStore, Store};
use lightcraft_engine::library::LibraryStores;

struct NativeLibrary {
    dir: std::path::PathBuf,
    catalog: MemStore,
}
impl NativeLibrary {
    fn new() -> Self {
        Self { dir: std::env::temp_dir().join(format!("lightcraft-ux11-{}", LocalRoundId::new().unwrap().as_str())), catalog: MemStore::new() }
    }
    fn open(&self) -> LightcraftApp {
        self.open_with(Box::new(FsStore::open(&self.dir).unwrap()))
    }
    fn open_with(&self, files: Box<dyn Store>) -> LightcraftApp {
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(LibraryStores { catalog: Box::new(self.catalog.clone()), files, dir: self.dir.clone(), on_disk: false }, true)
            .unwrap();
        LightcraftApp::new(session, crate::Services::default())
    }
}
impl Drop for NativeLibrary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn consent(app: LightcraftApp) -> (crate::headless::Headless, mpsc::Receiver<network::Request>, mpsc::SyncSender<network::Event>) {
    let mut app = app;
    let (connection, requests, events) = network::test_connection();
    app.ai.generation = app.session.library_generation();
    app.ai.connection = Some(connection);
    let mut h = crate::headless::Headless::new(app, [1600.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    h.step();
    h.step();
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), Duration::from_secs(10))["ok"], true);
    (h, requests, events)
}
pub(super) fn wait_idle(h: &mut crate::headless::Headless, requests: &mpsc::Receiver<network::Request>) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        poll(&mut h.app, &h.view.ctx);
        assert!(requests.try_recv().is_err(), "native preflight must refuse before any create/upload");
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["busy"] == false && status["workerBusy"] == false {
            return status;
        }
        assert!(Instant::now() < deadline, "native preflight did not retire: {status}");
        std::thread::yield_now();
    }
}
#[test]
fn native_orphan_tool_history_refuses_before_create_and_retains_readable_records() {
    let library = NativeLibrary::new();
    let mut app = library.open();
    let photo = app.session.active().unwrap();
    let saved =
        vec![json!({"role":"user","text":"keep all native text"}), json!({"role":"tool","tool_call_id":"orphan","text":"not a valid success"})];
    app.session.save_companion_tail(photo, None, &saved, &[], &[], &json!({"schema_version":2})).unwrap();
    let before = app.session.companion_messages_page(photo, 0, 2).unwrap().records;
    app.session.close_library().unwrap();
    drop(app);
    let (mut h, requests, _events) = consent(library.open());
    assert_eq!(h.app.run("ai.start", json!({"prompt":"current native request"})).unwrap()["started"], true);
    let status = wait_idle(&mut h, &requests);
    assert!(status["text"].as_str().unwrap().contains("history is incomplete or invalid"), "{status}");
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 2).unwrap().records, before);
    h.app.run("develop.set", json!({"control":"light.exposure","value":0.7})).unwrap();
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.7);
}

#[test]
fn native_current_photo_envelope_overflow_refuses_before_create_without_dropping_history() {
    let library = NativeLibrary::new();
    let mut app = library.open();
    let photo = app.session.active().unwrap();
    app.session
        .save_companion_tail(photo, None, &[json!({"role":"user","text":"x".repeat(256*1024-16)})], &[], &[], &json!({"schema_version":2}))
        .unwrap();
    let before = app.session.companion_messages_page(photo, 0, 1).unwrap().records;
    app.session.close_library().unwrap();
    drop(app);
    let (mut h, requests, _events) = consent(library.open());
    assert_eq!(h.app.run("ai.start", json!({"prompt":"actual current request"})).unwrap()["started"], true);
    let status = wait_idle(&mut h, &requests);
    assert!(status["text"].as_str().unwrap().contains("service limit"), "{status}");
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records, before);
    h.app.run("develop.set", json!({"control":"light.exposure","value":0.7})).unwrap();
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.7);
}

pub(super) fn outgoing(h: &mut crate::headless::Headless, requests: &mpsc::Receiver<network::Request>, suffix: &str) -> network::Request {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        h.step();
        if let Ok(request) = requests.try_recv() {
            assert!(request.path.ends_with(suffix), "unexpected {}", request.path);
            assert_eq!(h.app.run("ai.status", json!({})).unwrap()["workerBusy"], false, "HTTP publication follows producer retirement");
            return request;
        }
        assert!(Instant::now() < deadline, "{suffix} was never emitted");
        std::thread::yield_now();
    }
}
fn phase(events: &mpsc::SyncSender<network::Event>, creation: &network::Request, state: &str) {
    events.send(network::Event::Server {kind:"round.state".into(),data:json!({"round_id":format!("native-round-{}",creation.identity.submission_seq),"payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"library_generation":creation.identity.scope.library_generation,"photo_id":creation.identity.scope.photo.0},"state":state}})}).unwrap();
}
fn jpeg(blue: u8) -> Vec<u8> {
    lightcraft_engine::export::encode_image(
        &lightcraft_raster::Rgba8::from_fn(32, 32, |x, y| [(x * 7) as u8, (y * 7) as u8, blue, 255]),
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Jpeg, ..Default::default() },
    )
    .unwrap()
}
#[test]
fn native_legal_projection_keeps_all_complete_records_and_reserves_frozen_photo_input() {
    let library = NativeLibrary::new();
    let mut app = library.open();
    let photo = app.session.active().unwrap();
    let mut messages = vec![json!({"role":"user","text":"keep complete previous request"})];
    messages.extend((0..1030).map(|n| json!({"role":"assistant","text":format!("UNFINISHED {n}"),"complete_state":"streaming"})));
    messages.push(json!({"display_kind":"system_notice","text":"LOCAL NOTICE ONLY"}));
    messages.push(json!({"role":"assistant","text":"complete previous explanation","complete_state":"complete","local_secret":"stay local"}));
    app.session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
    let stored = app.session.companion_photo_header(photo).unwrap().unwrap().message_count;
    assert_eq!(stored, 1033);
    app.session.close_library().unwrap();
    drop(app);
    let (mut h, requests, events) = consent(library.open());
    h.app.run("develop.set", json!({"control":"light.exposure","value":0.3})).unwrap();
    h.app.run("ai.start", json!({"prompt":"immutable current request"})).unwrap();
    // A subsequent draft-changing invocation is refused but must not replace the leased prompt.
    assert!(h.app.run("ai.start", json!({"prompt":"later draft must not enter input"})).is_err());
    let creation = outgoing(&mut h, &requests, "/v1/rounds");
    phase(&events, &creation, "awaiting_input");
    let input = outgoing(&mut h, &requests, "/input");
    assert_eq!(input.body["prompt"], "immutable current request");
    assert_eq!(input.body["current_state"]["light"]["exposure"], 0.3);
    assert_eq!(input.body["capabilities"]["capability_version"], "lightcraft-v2");
    assert_eq!(
        input.body["history"],
        json!([{"role":"user","text":"keep complete previous request"},{"role":"assistant","text":"complete previous explanation"}])
    );
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "keep complete previous request");
    assert_eq!(h.app.session.companion_messages_page(photo, 1032, 1).unwrap().records[0]["text"], "complete previous explanation");
    assert_eq!(h.app.session.companion_photo_header(photo).unwrap().unwrap().message_count, stored + 1);
    h.app.run("ai.stop", json!({})).unwrap();
    h.step();
    h.step();
}

#[test]
fn native_historical_images_validate_bytes_owner_and_digest_before_create() {
    let complete = jpeg(80);
    let lossless = vec![
        0xff, 0xd8, 0xff, 0xc4, 0x00, 0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xc3, 0x00, 0x0b, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x01, 0x00,
        0x00, 0x7f, 0xff, 0xd9,
    ];
    for case in ["truncated", "wrong_owner", "digest", "missing", "sof3", "valid"] {
        let library = NativeLibrary::new();
        let mut app = library.open();
        let photo = app.session.active().unwrap();
        let bytes = if case == "truncated" { &complete[..complete.len() - 2] } else { &complete };
        let source = json!({"role":"user","text":format!("retain native {case} preview history"),"images":[{"mime_type":"image/jpeg","data":Base64::encode_string(bytes)}]});
        if case == "wrong_owner" {
            let other = app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
            app.session.save_companion_tail(other, None, &[source], &[], &[], &json!({"schema_version":2})).unwrap();
            let image = app.session.companion_messages_page(other, 0, 1).unwrap().records[0]["images"][0].clone();
            app.session
                .save_companion_tail(
                    photo,
                    None,
                    &[json!({"role":"user","text":"retain wrongly owned preview","images":[image]})],
                    &[],
                    &[],
                    &json!({"schema_version":2}),
                )
                .unwrap();
        } else {
            app.session.save_companion_tail(photo, None, &[source], &[], &[], &json!({"schema_version":2})).unwrap();
        }
        let before = app.session.companion_messages_page(photo, 0, 1).unwrap().records;
        let key = before[0]["images"][0]["attachment"].as_str().unwrap();
        let mut files = FsStore::open(&library.dir).unwrap();
        match case {
            "digest" => files.write_atomic(key, b"corrupt preview bytes").unwrap(),
            "missing" => files.remove(key).unwrap(),
            "sof3" => files.write_atomic(key, &lossless).unwrap(),
            _ => {}
        }
        app.session.close_library().unwrap();
        drop(app);
        let (mut h, requests, _events) = consent(library.open());
        h.app.run("ai.start", json!({"prompt":"real native image request"})).unwrap();
        if case == "valid" {
            outgoing(&mut h, &requests, "/v1/rounds");
        } else {
            let status = wait_idle(&mut h, &requests);
            assert!(!status["text"].as_str().unwrap().is_empty(), "{case}: actionable refusal");
            assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records, before, "{case}: original history retained");
            h.app.run("develop.set", json!({"control":"light.exposure","value":0.7})).unwrap();
            assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.7);
        }
    }
}

struct ReadGate {
    entered: mpsc::SyncSender<()>,
    release: std::sync::Mutex<mpsc::Receiver<()>>,
    used: AtomicBool,
    armed: AtomicBool,
    reads: std::sync::Mutex<Vec<(String, usize)>>,
}
struct GatedFiles {
    inner: FsStore,
    gate: Arc<ReadGate>,
    background: bool,
}
impl GatedFiles {
    fn observe(&self, name: &str, length: usize) {
        if !self.background || !self.gate.armed.load(Ordering::Acquire) || std::thread::current().name() != Some("companion-model-history") {
            return;
        }
        self.gate.reads.lock().unwrap().push((name.into(), length));
        if name.contains("/previews/") && !self.gate.used.swap(true, Ordering::Relaxed) {
            self.gate.entered.send(()).unwrap();
            self.gate.release.lock().unwrap().recv_timeout(Duration::from_secs(30)).unwrap();
        }
    }
}
impl Store for GatedFiles {
    fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.inner.read(name)
    }
    fn read_limited(&mut self, name: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.observe(name, usize::try_from(max).unwrap());
        self.inner.read_limited(name, max)
    }
    fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.observe(name, length);
        self.inner.read_range(name, offset, length)
    }
    fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.write_atomic(name, bytes)
    }
    fn write_atomic_with(&mut self, name: &str, fill: &mut dyn FnMut(&mut dyn std::io::Write) -> std::io::Result<()>) -> std::io::Result<u64> {
        self.inner.write_atomic_with(name, fill)
    }
    fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.append(name, bytes)
    }
    fn truncate(&mut self, name: &str, length: u64) -> std::io::Result<()> {
        self.inner.truncate(name, length)
    }
    fn remove(&mut self, name: &str) -> std::io::Result<()> {
        self.inner.remove(name)
    }
    fn describe(&self) -> String {
        self.inner.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.inner.retention_namespace()
    }
    fn conservative_retention(&self) -> bool {
        self.inner.conservative_retention()
    }
    fn background_writer(&self) -> Option<Box<dyn Store>> {
        Some(Box::new(Self { inner: FsStore::open(self.inner.dir()).unwrap(), gate: self.gate.clone(), background: true }))
    }
}
#[test]
fn native_stop_during_preview_read_prevents_old_input_after_reconsent_and_allows_fresh_submission() {
    let library = NativeLibrary::new();
    let mut app = library.open();
    let photo = app.session.active().unwrap();
    let messages = vec![
        json!({"role":"user","text":"retain both previews after stop","images":[{"mime_type":"image/jpeg","data":Base64::encode_string(&jpeg(80))},{"mime_type":"image/jpeg","data":Base64::encode_string(&jpeg(160))}]}),
    ];
    app.session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
    let before = app.session.companion_messages_page(photo, 0, 1).unwrap().records;
    app.session.close_library().unwrap();
    drop(app);
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let gate = Arc::new(ReadGate {
        entered: entered_tx,
        release: std::sync::Mutex::new(release_rx),
        used: AtomicBool::new(false),
        armed: AtomicBool::new(false),
        reads: std::sync::Mutex::new(Vec::new()),
    });
    let (mut h, requests, events) =
        consent(library.open_with(Box::new(GatedFiles { inner: FsStore::open(&library.dir).unwrap(), gate: gate.clone(), background: false })));
    // Opening the panel now reads the visible original preview too. Finish that
    // public view load before arming the actual filesystem pause for Start.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        h.step();
        if h.app.run("ai.status", json!({})).unwrap()["historyLoading"] == false {
            break;
        }
        assert!(Instant::now() < deadline, "visible history did not finish loading");
        std::thread::yield_now();
    }
    gate.armed.store(true, Ordering::Release);
    let old = h.app.run("ai.start", json!({"prompt":"old cancelled submission"})).unwrap()["localRoundId"].clone();
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(requests.try_recv().is_err());
    h.app.run("ai.stop", json!({})).unwrap();
    h.step();
    h.step();
    let timeout = Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
    let revoked = h.app.run("ai.start", json!({})).unwrap_err();
    assert!(revoked.contains("Please consent"), "{revoked}");
    h.step();
    h.step();
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
    let refused = h.app.run("ai.start", json!({"prompt":"must not overtake retiring reader"})).unwrap_err();
    assert!(refused.contains("Another AI task"), "{refused}");
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["workerBusy"], true);
    release_tx.send(()).unwrap();
    wait_idle(&mut h, &requests);
    let reads = gate.reads.lock().unwrap().clone();
    assert_eq!(reads.iter().filter(|(name, _)| name.contains("/previews/")).count(), 1, "stop prevents reading the second image");
    assert!(reads.iter().all(|(_, length)| *length <= 32 * 1024 * 1024), "native read calls remain bounded");
    assert!(reads.iter().any(|(name, length)| name.contains("/blocks/") && *length < 4096), "records cross actual bounded range reads");
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records, before);
    h.app.run("develop.set", json!({"control":"light.exposure","value":0.7})).unwrap();
    h.app.run("ai.start", json!({"prompt":"fresh submission after retirement"})).unwrap();
    let creation = outgoing(&mut h, &requests, "/v1/rounds");
    assert_ne!(creation.identity.local_round_id.as_str(), old.as_str().unwrap());
    phase(&events, &creation, "awaiting_input");
    let input = outgoing(&mut h, &requests, "/input");
    assert_eq!(input.body["prompt"], "fresh submission after retirement");
    assert_eq!(input.body["current_state"]["light"]["exposure"], 0.7);
    assert_eq!(input.body["history"][0]["text"], "retain both previews after stop");
    h.app.run("ai.stop", json!({})).unwrap();
    h.step();
    h.step();
}

#[test]
fn native_stop_and_failure_reopen_next_input_with_every_true_tool_result_in_original_order() {
    for terminal in ["cancelled", "failed"] {
        let library = NativeLibrary::new();
        let (mut h, requests, events) = consent(library.open());
        let photo = h.app.session.active().unwrap();
        h.app.run("ai.start", json!({"prompt":"first real native round"})).unwrap();
        let creation = outgoing(&mut h, &requests, "/v1/rounds");
        phase(&events, &creation, "awaiting_input");
        outgoing(&mut h, &requests, "/input");
        let round = creation.round.clone();
        // Creation's round is empty until admission; use the real association supplied by phase.
        assert!(round.is_empty());
        let round = format!("native-round-{}", creation.identity.submission_seq);
        let calls: Vec<_> =
            ["z-done", "b-unexecuted", "a-unexecuted"].iter().map(|id| json!({"id":id,"name":"get_photo_state","arguments":{}})).collect();
        events
            .send(network::Event::Server {
                kind: "message.completed".into(),
                data: json!({"round_id":round,"payload":{"message_id":"three-tools","model_call_id":"call-1","tool_calls":calls}}),
            })
            .unwrap();
        events.send(network::Event::Server{kind:"tool.request".into(),data:json!({"round_id":round,"payload":{"tool_call_id":"z-done","tool_request_id":"first-read","expected_revision":h.app.session.photo_revision(photo),"tool_name":"get_photo_state"}})}).unwrap();
        let executed = outgoing(&mut h, &requests, "/tool-results");
        assert_eq!(executed.body["outcome"], "succeeded");
        events
            .send(network::Event::Server {
                kind: "text.delta".into(),
                data: json!({"round_id":round,"payload":{"message_id":"unfinished","delta":"UNFINISHED MUST NOT ENTER NEXT INPUT"}}),
            })
            .unwrap();
        h.step();
        if terminal == "cancelled" {
            h.app.run("ai.stop", json!({})).unwrap();
            outgoing(&mut h, &requests, "/stop");
        } else {
            events
                .send(network::Event::Server {
                    kind: "round.ended".into(),
                    data: json!({"round_id":round,"payload":{"terminal_state":"failed","reason_code":"model_error"}}),
                })
                .unwrap();
        }
        wait_idle(&mut h, &requests);
        h.step();
        h.step();
        before_library_change(&mut h.app).unwrap();
        h.app.session.close_library().unwrap();
        drop(h);
        drop(events);
        drop(requests);
        let (mut h, requests, events) = consent(library.open());
        h.app.run("ai.start", json!({"prompt":"new request after reopening"})).unwrap();
        let creation = outgoing(&mut h, &requests, "/v1/rounds");
        phase(&events, &creation, "awaiting_input");
        let input = outgoing(&mut h, &requests, "/input");
        let history = input.body["history"].as_array().unwrap();
        let tools: Vec<_> = history.iter().filter(|m| m["role"] == "tool").collect();
        assert_eq!(tools.iter().map(|m| m["tool_call_id"].as_str().unwrap()).collect::<Vec<_>>(), ["z-done", "b-unexecuted", "a-unexecuted"]);
        let result: Value = serde_json::from_str(tools[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(result["settings"]["light"]["exposure"], 0.0);
        for tool in &tools[1..] {
            let result: Value = serde_json::from_str(tool["text"].as_str().unwrap()).unwrap();
            assert_eq!(result["outcome"], terminal);
            assert_eq!(result["executed"], false);
        }
        assert!(!input.body.to_string().contains("UNFINISHED MUST NOT ENTER NEXT INPUT"));
        assert!(history.iter().all(|m| m.get("local_round_id").is_none() && m.get("local_message_id").is_none() && m.get("stage").is_none()));
        if terminal == "cancelled"
            && let Ok(path) = std::env::var("UX11_NATIVE_HISTORY_FIXTURE")
        {
            std::fs::write(path, serde_json::to_vec_pretty(&input.body).unwrap()).unwrap();
        }
        h.app.run("ai.stop", json!({})).unwrap();
        h.step();
        h.step();
    }
}

#[test]
fn native_invalid_plan_reopens_as_proposal_and_separate_actual_failure_without_fake_effect() {
    let library = NativeLibrary::new();
    let (mut h, requests, events) = consent(library.open());
    let photo = h.app.session.active().unwrap();
    h.app.run("ai.start", json!({"prompt":"real plan validation round"})).unwrap();
    let creation = outgoing(&mut h, &requests, "/v1/rounds");
    phase(&events, &creation, "awaiting_input");
    outgoing(&mut h, &requests, "/input");
    phase(&events, &creation, "awaiting_initial_execution");
    let round = format!("native-round-{}", creation.identity.submission_seq);
    let operations = json!([{"type":"set_controls","values":{"unsupported.control":1.0}}]);
    events.send(network::Event::Server{kind:"message.completed".into(),data:json!({"round_id":round,"stage":"initial","payload":{"message_id":"proposal","model_call_id":"model-1","tool_calls":[{"id":"invalid-plan-tool","name":"apply_edit_plan","arguments":{"operations":operations}}]}})}).unwrap();
    events.send(network::Event::Server{kind:"edit.plan".into(),data:json!({"round_id":round,"stage":"initial","payload":{"plan_id":"bad-plan","tool_call_id":"invalid-plan-tool","capability_version":"lightcraft-v2","expected_revision":h.app.session.photo_revision(photo),"reason":"an invalid control","operations":operations}})}).unwrap();
    outgoing(&mut h, &requests, "/stop");
    wait_idle(&mut h, &requests);
    h.step();
    h.step();
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.0);
    before_library_change(&mut h.app).unwrap();
    h.app.session.close_library().unwrap();
    drop(h);
    drop(events);
    drop(requests);
    let (mut h, requests, events) = consent(library.open());
    h.app.run("ai.start", json!({"prompt":"continue after validation failure"})).unwrap();
    let creation = outgoing(&mut h, &requests, "/v1/rounds");
    phase(&events, &creation, "awaiting_input");
    let input = outgoing(&mut h, &requests, "/input");
    let history = input.body["history"].as_array().unwrap();
    let proposed = history.iter().find(|m| m["role"] == "tool" && m["tool_call_id"] == "invalid-plan-tool").unwrap();
    assert_eq!(proposed["text"], "plan_proposed; await actual client execution feedback");
    let feedback = history.iter().find_map(|m| m["text"].as_str().and_then(|s| s.strip_prefix("Actual execution feedback: "))).unwrap();
    let feedback: Value = serde_json::from_str(feedback).unwrap();
    assert_eq!(feedback["outcome"], "failed");
    assert_eq!(feedback["reason_code"], "validation_failed");
    assert_eq!(feedback["executed"], false);
    assert_eq!(feedback["actual_state"]["light"]["exposure"], 0.0);
    assert_eq!(input.body["current_state"]["light"]["exposure"], 0.0);
    assert_eq!(history.iter().filter(|m| m["tool_call_id"] == "invalid-plan-tool").count(), 1);
    h.app.run("ai.stop", json!({})).unwrap();
    h.step();
    h.step();
}
