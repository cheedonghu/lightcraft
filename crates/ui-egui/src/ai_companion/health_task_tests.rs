use super::*;
use crate::{Services, headless::Headless};
use lightcraft_engine::Session;

#[test]
fn first_ai_open_exposes_explicit_service_retry_without_submitting() {
    let mut h = Headless::new(LightcraftApp::new(Session::with_demo(), Services::default()), [1100.0, 750.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    for _ in 0..4 {
        h.step();
    }
    assert!(h.app.widgets.iter().any(|(id, _)| id == "button:aiHealthRetry"), "the real panel exposes service retry");
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert!(status["health"]["state"].is_string(), "{status}");
    assert!(status["activeRounds"].as_array().unwrap().is_empty(), "health never submits");
}

fn request(h: &mut Headless, method: &str, params: Value) -> Value {
    let result = h.request(method, params, Duration::from_secs(10));
    assert_eq!(result["ok"], true, "{result}");
    result
}
fn frames(h: &mut Headless) {
    for _ in 0..4 {
        h.step();
    }
}
#[test]
fn global_entry_keeps_original_photo_after_background_completion_and_only_click_navigates() {
    let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
    let (connection, requests, events) = network::test_connection();
    app.ai.connection = Some(connection); // existing actual transport adapter
    app.ai.generation = app.session.library_generation();
    let mut h = Headless::new(app, [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    request(&mut h, "ui.clickWidget", json!({"id":"checkbox:aiConsent"}));
    let photo = h.app.session.active().unwrap();
    let name = h.app.session.catalog.photo(photo).unwrap().file_name.clone();
    h.app.run("ai.start", json!({"prompt":"Explain my original photo"})).unwrap();
    let create = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    let other = h.app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    h.app.run("panel.close", json!({})).unwrap();
    h.app.run("library.select", json!({"ids":[other.0],"active":other.0})).unwrap();
    events.send(network::Event::Server {kind:"round.state".into(),data:json!({"round_id":"global-round","payload":{"submission_seq":create.submission,"photo_ref":create.body["photo_ref"],"state":"analyzing_initial"}})}).unwrap();
    frames(&mut h);
    assert!(h.app.widgets.iter().any(|(id, _)| id == "button:aiTaskEntry"), "task reachable with hidden Tools and another photo selected");
    assert!(h.painted_text().iter().any(|text| text.contains(&name)), "original photo name stays visible");
    events
        .send(network::Event::Server {
            kind: "text.delta".into(),
            data: json!({"round_id":"global-round","payload":{"message_id":"answer","delta":"Original photo explanation"}}),
        })
        .unwrap();
    events
        .send(network::Event::Server {
            kind: "message.completed".into(),
            data: json!({"round_id":"global-round","payload":{"message_id":"answer","tool_calls":[]}}),
        })
        .unwrap();
    events
        .send(network::Event::Server {
            kind: "round.ended".into(),
            data: json!({"round_id":"global-round","payload":{"terminal_state":"completed","reason_code":"answer_only"}}),
        })
        .unwrap();
    frames(&mut h);
    assert_eq!(h.app.session.active(), Some(other), "terminal cannot steal selection");
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "field:aiPrompt"), "terminal cannot open Tools");
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["task"]["photo"], photo.0);
    assert_eq!(status["task"]["terminal"], "completed");
    assert_eq!(status["task"]["unread"], true);
    request(&mut h, "ui.clickWidget", json!({"id":"button:aiTaskEntry"}));
    frames(&mut h);
    assert_eq!(h.app.session.active(), Some(photo));
    assert!(h.app.widgets.iter().any(|(id, _)| id == "field:aiPrompt"));
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["task"]["unread"], false);
    assert!(requests.try_recv().is_err(), "entry navigation does not submit");
}

#[test]
fn failed_round_explains_actual_untouched_photo_and_recovery_and_copies_trace() {
    let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
    let (connection, requests, events) = network::test_connection();
    app.ai.connection = Some(connection);
    app.ai.generation = app.session.library_generation();
    let mut h = Headless::new(app, [1100.0, 750.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    request(&mut h, "ui.clickWidget", json!({"id":"checkbox:aiConsent"}));
    let photo = h.app.session.active().unwrap();
    let original = h.app.session.develop_of(photo).unwrap();
    h.app.run("ai.start", json!({"prompt":"Improve my photo"})).unwrap();
    let create = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    events.send(network::Event::Server {kind:"round.state".into(),data:json!({"round_id":"failed-round","payload":{"submission_seq":create.submission,"photo_ref":create.body["photo_ref"],"state":"analyzing_initial"}})}).unwrap();
    events.send(network::Event::Server {kind:"round.ended".into(),data:json!({"round_id":"failed-round","trace_id":"0123456789ab","payload":{"terminal_state":"failed","reason_code":"capability_unsupported"}})}).unwrap();
    frames(&mut h);
    let status = h.app.run("ai.status", json!({})).unwrap();
    let text = status["text"].as_str().unwrap();
    assert!(text.contains("does not support") && text.contains("starting settings") && text.contains("new request"), "{text}");
    assert!(!text.contains("capability_unsupported") && !text.contains("traceId="), "technical codes are secondary, not the primary error: {text}");
    assert_eq!(*h.app.session.develop_of(photo).unwrap(), *original);
    request(&mut h, "ui.clickWidget", json!({"id":"button:aiCopyTrace"}));
    assert_eq!(h.view.clipboard, "0123456789ab");
}

#[test]
fn explicit_disconnect_ends_global_progress_and_explains_manual_recovery() {
    let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
    let (connection, requests, events) = network::test_connection();
    app.ai.connection = Some(connection);
    app.ai.generation = app.session.library_generation();
    let mut h = Headless::new(app, [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    request(&mut h, "ui.clickWidget", json!({"id":"checkbox:aiConsent"}));
    let photo = h.app.session.active().unwrap();
    let original = h.app.session.develop_of(photo).unwrap();
    h.app.run("ai.start", json!({"prompt":"Explain this photo"})).unwrap();
    let create = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    events.send(network::Event::Server {kind:"round.state".into(),data:json!({"round_id":"disconnect-round","payload":{"submission_seq":create.submission,"photo_ref":create.body["photo_ref"],"state":"analyzing_initial"}})}).unwrap();
    frames(&mut h);
    events.send(network::Event::Closed("transport socket closed".into())).unwrap();
    frames(&mut h);
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["busy"], false);
    assert_eq!(status["task"]["terminal"], "disconnected", "{status}");
    assert!(status["text"].as_str().unwrap().contains("Check the local service"), "{status}");
    assert_eq!(*h.app.session.develop_of(photo).unwrap(), *original);
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiStop"));
    assert!(requests.try_recv().is_err(), "disconnect never resubmits");
}

#[test]
fn deleted_task_photo_opens_its_readonly_conversation_without_selecting_another_photo() {
    let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
    let (connection, requests, events) = network::test_connection();
    app.ai.connection = Some(connection);
    app.ai.generation = app.session.library_generation();
    let mut h = Headless::new(app, [1100.0, 750.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    request(&mut h, "ui.clickWidget", json!({"id":"checkbox:aiConsent"}));
    let photo = h.app.session.active().unwrap();
    let other = h.app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    h.app.run("ai.start", json!({"prompt":"Original task photo request"})).unwrap();
    let create = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    events.send(network::Event::Server {kind:"round.state".into(),data:json!({"round_id":"deleted-round","payload":{"submission_seq":create.submission,"photo_ref":create.body["photo_ref"],"state":"analyzing_initial"}})}).unwrap();
    events
        .send(network::Event::Server {
            kind: "text.delta".into(),
            data: json!({"round_id":"deleted-round","payload":{"message_id":"answer","delta":"ORIGINAL_READONLY_ANSWER"}}),
        })
        .unwrap();
    events
        .send(network::Event::Server {
            kind: "message.completed".into(),
            data: json!({"round_id":"deleted-round","payload":{"message_id":"answer","tool_calls":[]}}),
        })
        .unwrap();
    events
        .send(network::Event::Server {
            kind: "round.ended".into(),
            data: json!({"round_id":"deleted-round","payload":{"terminal_state":"completed"}}),
        })
        .unwrap();
    frames(&mut h);
    h.app.session.execute("photo.delete", &json!({"ids":[photo.0]})).unwrap();
    let mut editor = lightcraft_engine::companion::EditorState::default();
    editor.draft = "B_SAVED_DRAFT".into();
    editor.scroll_offset = 30.0;
    editor.follow_tail = false;
    h.app.session.set_companion_editor(other, editor).unwrap();
    h.app.session.set_photo_companion(other, Some(json!({"schema_version":2,"messages":[{"role":"assistant","text":(0..60).map(|n|format!("B saved history line {n}\n")).collect::<String>()}]}))).unwrap();
    h.app.run("library.select", json!({"ids":[other.0],"active":other.0})).unwrap();
    frames(&mut h);
    let before_b = h.app.session.companion_editor(other).unwrap();
    assert_eq!(before_b.draft, "B_SAVED_DRAFT");
    assert!(before_b.scroll_offset > 0.0);
    h.app.run("panel.close", json!({})).unwrap();
    frames(&mut h);
    request(&mut h, "ui.clickWidget", json!({"id":"button:aiTaskEntry"}));
    frames(&mut h);
    assert!(h.app.widgets.iter().any(|(id, _)| id == "indicator:aiTaskReadOnly"), "unavailable original photo has a readonly conversation");
    assert_eq!(h.app.session.active(), Some(other));
    assert!(h.painted_text().iter().any(|text| text.contains("ORIGINAL_READONLY_ANSWER")));
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "field:aiPrompt" || id == "button:aiStart"));
    assert!(h.app.run("ai.start", json!({"prompt":"must not send from readonly"})).is_err());
    assert!(requests.try_recv().is_err());
    assert_eq!(h.app.session.companion_editor(other).unwrap(), before_b, "readonly A leaves B draft and reading position unchanged");
    request(&mut h, "ui.clickWidget", json!({"id":"button:aiSelectedConversation"}));
    frames(&mut h);
    assert_eq!(h.app.session.active(), Some(other));
    assert!(h.app.widgets.iter().any(|(id, _)| id == "field:aiPrompt"));
    assert!(h.painted_text().iter().any(|text| text.contains("B_SAVED_DRAFT")));
    assert_eq!(h.app.session.companion_editor(other).unwrap(), before_b, "return restores B reading position and draft");
}

fn read_http(stream: std::net::TcpStream) -> (std::io::BufReader<std::net::TcpStream>, String, Value) {
    use std::io::{BufRead, Read};
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut reader = std::io::BufReader::new(stream);
    let mut first = String::new();
    assert!(reader.read_line(&mut first).unwrap() > 0);
    let mut length = 0;
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some(size) = line.strip_prefix("Content-Length: ") {
            length = size.trim().parse::<usize>().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let value = if body.is_empty() { Value::Null } else { serde_json::from_slice(&body).unwrap() };
    (reader, first, value)
}
fn reply_http(reader: &mut std::io::BufReader<std::net::TcpStream>, status: u16, body: Value) {
    use std::io::Write;
    let body = body.to_string();
    write!(reader.get_mut(), "HTTP/1.0 {status} OK\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
}
#[test]
fn native_health_is_async_get_only_and_newer_retry_wins_over_blocked_older_result() {
    use std::io::Write;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let (seen_tx, seen_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut old, path, body) = read_http(listener.accept().unwrap().0);
        assert!(path.starts_with("GET /health "), "{path}");
        assert_eq!(body, Value::Null);
        seen_tx.send(()).unwrap();
        let old_worker = std::thread::spawn(move || {
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            let _ = write!(old.get_mut(), "HTTP/1.0 503 Error\r\nContent-Length: 2\r\n\r\n{{}}");
        });
        let (mut newest, path, body) = read_http(listener.accept().unwrap().0);
        assert!(path.starts_with("GET /health "), "{path}");
        assert_eq!(body, Value::Null);
        reply_http(&mut newest, 200, json!({"status":"ok"}));
        old_worker.join().unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(listener.accept().is_err(), "health cannot register/run automatically");
    });
    let mut h = Headless::new(
        LightcraftApp::new(Session::with_demo(), Services { companion_address: Some(address), ..Default::default() }),
        [1000.0, 680.0],
        1.0,
    );
    let before = Instant::now();
    h.app.run("ai.open", json!({})).unwrap();
    h.step();
    assert!(before.elapsed() < Duration::from_secs(1), "blocked HTTP cannot block first-open feedback");
    seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["health"]["state"], "checking");
    assert_eq!(h.app.run("ai.health", json!({})).unwrap()["probe"], 2);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["health"]["state"] == "reachable" {
            assert!(status["activeRounds"].as_array().unwrap().is_empty());
            break;
        }
        assert!(Instant::now() < deadline, "{status}");
        std::thread::yield_now();
    }
    release_tx.send(()).unwrap();
    server.join().unwrap();
    frames(&mut h);
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["health"]["probe"], 2);
    assert_eq!(status["health"]["state"], "reachable");
    assert!(h.painted_text().iter().any(|text| text.contains("Model availability is checked when you submit")));
}

struct SilentServer {
    address: String,
    stop: Arc<AtomicBool>,
    requests: Arc<std::sync::Mutex<Vec<String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl SilentServer {
    fn new() -> Self {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = requests.clone();
        let thread = std::thread::spawn(move || {
            let client = "a".repeat(32);
            let token = "b".repeat(32);
            let owner = "c".repeat(32);
            let mut events: Option<std::net::TcpStream> = None;
            let mut seq = 0u64;
            let mut creation = Value::Null;
            let mut ping_at = Instant::now();
            let mut ping_seq = 0;
            while !stopped.load(Ordering::Relaxed) {
                let stream = match listener.accept() {
                    Ok((stream, _)) => Some(stream),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
                    Err(e) => panic!("{e}"),
                };
                if let Some(stream) = stream {
                    let (mut reader, path, body) = read_http(stream);
                    observed.lock().unwrap().push(path.clone());
                    if path.starts_with("GET /health ") {
                        reply_http(&mut reader, 200, json!({"status":"ok"}));
                    } else if path.starts_with("POST /v1/clients ") {
                        reply_http(&mut reader, 200, json!({"client_id":client,"client_token":token}));
                    } else if path.starts_with(&format!("GET /v1/clients/{client}/events ")) {
                        let mut stream = reader.into_inner();
                        write!(stream, "HTTP/1.0 200 OK\r\n\r\n").unwrap();
                        seq += 1;
                        write!(stream, "event: connected\ndata: {}\n\n", json!({"protocol_version":"v1","event_seq":seq,"connection_id":owner}))
                            .unwrap();
                        events = Some(stream);
                    } else if path.starts_with("POST /v1/rounds ") {
                        creation = body;
                        reply_http(&mut reader, 202, json!({"accepted":true,"round_id":"silent-round"}));
                        seq += 1;
                        write!(events.as_mut().unwrap(),"event: round.state\ndata: {}\n\n",json!({"protocol_version":"v1","event_seq":seq,"connection_id":owner,"round_id":"silent-round","payload":{"state":"awaiting_input","submission_seq":creation["submission_seq"],"photo_ref":creation["photo_ref"]}})).unwrap();
                    } else if path.starts_with("POST /v1/rounds/silent-round/input ") {
                        assert!(body["prompt"].as_str().unwrap().contains("silent analysis"));
                        reply_http(&mut reader, 202, json!({"accepted":true}));
                        seq += 1;
                        write!(events.as_mut().unwrap(),"event: round.state\ndata: {}\n\n",json!({"protocol_version":"v1","event_seq":seq,"connection_id":owner,"round_id":"silent-round","payload":{"state":"analyzing_initial","submission_seq":creation["submission_seq"],"photo_ref":creation["photo_ref"]}})).unwrap();
                        ping_at = Instant::now() - Duration::from_secs(5);
                    } else if path.starts_with(&format!("POST /v1/clients/{client}/heartbeat ")) {
                        assert!(body.get("preview").is_none());
                        reply_http(&mut reader, 200, json!({"accepted":true}));
                    } else {
                        panic!("unexpected request {path}");
                    }
                }
                if !creation.is_null()
                    && ping_at.elapsed() >= Duration::from_secs(5)
                    && let Some(stream) = events.as_mut()
                {
                    seq += 1;
                    ping_seq += 1;
                    write!(
                        stream,
                        "event: ping\ndata: {}\n\n",
                        json!({"protocol_version":"v1","event_seq":seq,"connection_id":owner,"payload":{"ping_seq":ping_seq}})
                    )
                    .unwrap();
                    ping_at = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        Self { address, stop, requests, thread: Some(thread) }
    }
    fn close(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
impl Drop for SilentServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[test]
fn native_acknowledged_heartbeats_keep_silent_model_analysis_alive_after_thirty_seconds_then_disconnect() {
    let mut server = SilentServer::new();
    let mut app = LightcraftApp::new(Session::with_demo(), Services { companion_address: Some(server.address.clone()), ..Default::default() });
    app.ui.settings.gpu = false;
    let mut h = Headless::new(app, [1100.0, 750.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    request(&mut h, "ui.clickWidget", json!({"id":"checkbox:aiConsent"}));
    h.app.run("ai.start", json!({"prompt":"silent analysis of the current photo"})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["task"]["phase"] == "analyzing_initial" {
            break;
        }
        assert!(Instant::now() < deadline, "{status}");
        std::thread::sleep(Duration::from_millis(5));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        h.step();
        if server.requests.lock().unwrap().iter().any(|p| p.contains("/heartbeat ")) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["transport"]["heartbeats"].as_u64().unwrap_or(0) > 0 {
            break;
        }
        assert!(Instant::now() < deadline, "successful heartbeat is independent from model text: {status}");
        std::thread::sleep(Duration::from_millis(5));
    }
    let silent_since = Instant::now();
    while silent_since.elapsed() < Duration::from_secs(32) {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(status["busy"], true, "model silence cannot end processing: {status}");
        std::thread::sleep(Duration::from_millis(25));
    }
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["task"]["phase"], "analyzing_initial");
    assert_eq!(status["transport"]["lastTextAgeMs"], Value::Null);
    assert!(status["transport"]["heartbeats"].as_u64().unwrap() >= 6);
    server.close();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["busy"] == false {
            assert_eq!(status["task"]["terminal"], "disconnected");
            break;
        }
        assert!(Instant::now() < deadline, "{status}");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(server.requests.lock().unwrap().iter().filter(|p| p.starts_with("POST /v1/rounds ")).count(), 1, "no automatic resume");
}

#[test]
fn async_health_failure_between_consent_down_and_up_keeps_the_real_checkbox_click_valid() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let (seen_tx, seen_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut reader, path, _) = read_http(listener.accept().unwrap().0);
        assert!(path.starts_with("GET /health "));
        seen_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        reply_http(&mut reader, 503, json!({}));
    });
    let mut app = LightcraftApp::new(Session::with_demo(), Services { companion_address: Some(address), ..Default::default() });
    let (connection, requests, _events) = network::test_connection();
    app.ai.connection = Some(connection);
    app.ai.generation = app.session.library_generation();
    let mut h = Headless::new(app, [1000.0, 680.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let before = h.app.widgets.iter().find(|(id, _)| id == "checkbox:aiConsent").unwrap().1;
    let point = before.center();
    h.app.synthetic.extend([
        egui::Event::PointerMoved(point),
        egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
    ]);
    h.step();
    release_tx.send(()).unwrap();
    server.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        h.step();
        if h.app.run("ai.status", json!({})).unwrap()["health"]["state"] == "unreachable" {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        h.app.widgets.iter().find(|(id, _)| id == "checkbox:aiConsent").unwrap().1,
        before,
        "async health must preserve the existing control rectangle before release"
    );
    h.app.synthetic.push(egui::Event::PointerButton {
        pos: point,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    frames(&mut h);
    assert_eq!(h.app.run("ai.start", json!({"prompt":"one real consent click"})).unwrap()["started"], true);
    assert_eq!(requests.recv_timeout(Duration::from_secs(1)).unwrap().path, "/v1/rounds");
}
