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
    h.app.session.execute("develop.set", &json!({"ids":[other.0],"values":{"light.exposure":0.7}})).unwrap();
    let editor =
        lightcraft_engine::companion::EditorState { draft: "B_SAVED_DRAFT".into(), scroll_offset: 30.0, follow_tail: false, ..Default::default() };
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
    let third = h.app.session.catalog.photos().find(|p| p.id != photo && p.id != other).unwrap().id;
    h.app
        .session
        .set_companion_editor(
            third,
            lightcraft_engine::companion::EditorState {
                draft: "C_SAVED_DRAFT".into(),
                scroll_offset: 40.0,
                follow_tail: false,
                ..Default::default()
            },
        )
        .unwrap();
    h.app.session.set_photo_companion(third,Some(json!({"schema_version":2,"messages":[{"role":"assistant","text":(0..60).map(|n|format!("C saved history line {n}\n")).collect::<String>()}]}))).unwrap();
    let original_c = h.app.session.develop_of(third).unwrap();
    for ids in [vec![third.0], vec![third.0, other.0], vec![]] {
        h.app.run("library.select", json!({"ids":[other.0],"active":other.0})).unwrap();
        frames(&mut h);
        h.app.run("ai.task", json!({})).unwrap();
        frames(&mut h);
        h.app.session.execute("library.select", &json!({"ids":ids})).unwrap();
        frames(&mut h);
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(status["readonlyPhoto"], Value::Null, "an explicit new selection ends readonly A: {status}");
        assert!(!h.app.widgets.iter().any(|(id, _)| id == "indicator:aiTaskReadOnly"));
        if ids.len() == 1 {
            assert!(h.painted_text().iter().any(|text| text.contains("C_SAVED_DRAFT")));
            let c = h.app.session.companion_editor(third).unwrap();
            assert_eq!(c.draft, "C_SAVED_DRAFT");
            assert_eq!(c.scroll_offset, 40.0);
        } else {
            assert!(h.app.widgets.iter().any(|(id, _)| id == "ai:selectionNotice"));
            assert!(!h.app.widgets.iter().any(|(id, _)| id == "field:aiPrompt"));
        }
        assert_eq!(h.app.session.companion_editor(other).unwrap(), before_b, "A cannot overwrite B reading or draft");
        assert_eq!(*h.app.session.develop_of(third).unwrap(), *original_c, "navigation never restores old effects");
        assert!(requests.try_recv().is_err(), "selection never submits");
        assert_eq!(h.app.session.develop_of(other).unwrap().light.exposure, 0.7);
    }
    h.app.run("library.select", json!({"ids":[other.0]})).unwrap();
    frames(&mut h);
    h.app.run("ai.task", json!({})).unwrap();
    frames(&mut h);
    assert_eq!(h.app.run("ai.returnCurrent", json!({})).unwrap()["returnedToCurrent"], true);
    frames(&mut h);
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["readonlyPhoto"], Value::Null);
    assert_eq!(h.app.session.develop_of(other).unwrap().light.exposure, 0.7);
    assert!(h.painted_text().iter().any(|text| text.contains("B_SAVED_DRAFT")));
    h.app.run("ai.task", json!({})).unwrap();
    frames(&mut h);
    h.app.run("library.select", json!({"ids":[other.0]})).unwrap();
    frames(&mut h);
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["readonlyPhoto"], Value::Null, "explicit same B selection is current-view intent");
    assert!(requests.try_recv().is_err());
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

fn outgoing(h: &mut Headless, requests: &mpsc::Receiver<network::Request>, suffix: &str) -> network::Request {
    let deadline = Instant::now() + Duration::from_secs(15);
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
fn backend_failure_with_unconfirmed_feedback_keeps_actual_published_edit_and_original_global_scope() {
    let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
    app.ui.settings.gpu = false;
    let (connection, requests, events) = network::test_connection();
    app.ai.connection = Some(connection);
    app.ai.generation = app.session.library_generation();
    let mut h = Headless::new(app, [1100.0, 750.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    request(&mut h, "ui.clickWidget", json!({"id":"checkbox:aiConsent"}));
    let photo = h.app.session.active().unwrap();
    h.app.run("ai.start", json!({"prompt":"Improve this photo"})).unwrap();
    let create = outgoing(&mut h, &requests, "/v1/rounds");
    let phase = |events: &mpsc::SyncSender<network::Event>, state: &str| {
        events.send(network::Event::Server{kind:"round.state".into(),data:json!({"round_id":"published-round","payload":{"submission_seq":create.submission,"photo_ref":create.body["photo_ref"],"state":state}})}).unwrap()
    };
    phase(&events, "awaiting_input");
    outgoing(&mut h, &requests, "/input");
    phase(&events, "awaiting_initial_execution");
    let operations = json!([{"type":"set_controls","values":{"light.exposure":0.4}}]);
    events.send(network::Event::Server{kind:"message.completed".into(),data:json!({"round_id":"published-round","stage":"initial","payload":{"message_id":"proposal","model_call_id":"model-1","tool_calls":[{"id":"edit-tool","name":"apply_edit_plan","arguments":{"operations":operations}}]}})}).unwrap();
    events.send(network::Event::Server{kind:"edit.plan".into(),data:json!({"round_id":"published-round","stage":"initial","payload":{"plan_id":"initial-plan","tool_call_id":"edit-tool","capability_version":"lightcraft-v2","expected_revision":h.app.session.photo_revision(photo),"reason":"Raise exposure slightly","operations":operations}})}).unwrap();
    let execution = outgoing(&mut h, &requests, "/execution");
    assert_eq!(execution.body["outcome"], "succeeded");
    assert_eq!(execution.body["actual_state"]["light"]["exposure"], 0.4);
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.4);
    let other = h.app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    h.app.run("library.select", json!({"ids":[other.0]})).unwrap();
    h.app.run("panel.close", json!({})).unwrap();
    phase(&events, "analyzing_review");
    events.send(network::Event::Server{kind:"round.ended".into(),data:json!({"round_id":"published-round","trace_id":"123456789abc","payload":{"terminal_state":"failed","reason_code":"model_failed","last_confirmed_plan_id":null}})}).unwrap();
    frames(&mut h);
    assert_eq!(h.app.session.active(), Some(other));
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.4);
    let status = h.app.run("ai.status", json!({"photo":photo.0})).unwrap();
    let text = status["text"].as_str().unwrap();
    assert!(text.contains("latest complete edit is kept") && !text.contains("starting settings"), "{status}");
    assert_eq!(status["task"]["photo"], photo.0);
    assert_eq!(status["task"]["terminal"], "failed");
    assert_eq!(status["task"]["unread"], true);
    request(&mut h, "ui.clickWidget", json!({"id":"button:aiTaskEntry"}));
    frames(&mut h);
    assert_eq!(h.app.session.active(), Some(photo));
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.4);
    request(&mut h, "ui.clickWidget", json!({"id":"button:aiCopyTrace"}));
    assert_eq!(h.view.clipboard, "123456789abc");
    assert!(requests.try_recv().is_err());
    h.app.session.execute("develop.set", &json!({"ids":[other.0],"values":{"light.exposure":0.7}})).unwrap();
    h.app.session.execute("photo.delete", &json!({"ids":[photo.0]})).unwrap();
    h.app.run("library.select", json!({"ids":[other.0]})).unwrap();
    h.app.run("ai.task", json!({})).unwrap();
    frames(&mut h);
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["readonlyPhoto"], photo.0);
    request(&mut h, "ui.clickWidget", json!({"id":"button:aiCopyTrace"}));
    assert_eq!(h.view.clipboard, "123456789abc");
    // Catches a read-only A card opening selected B's History; misses native OS input delivery.
    let id = format!("button:aiHistory:{}", status["task"]["localRoundId"].as_str().unwrap());
    request(&mut h, "ui.clickWidget", json!({"id":id}));
    frames(&mut h);
    assert_eq!(request(&mut h, "ui.inspect", json!({}))["result"]["ui"]["right"], "ai");
    // Catches default selected-B mutations while A is shown; misses the future clear confirmation UI.
    assert!(h.app.run("ai.clear", json!({})).is_err(), "readonly task view rejects clearing the selected photo");
    assert!(h.app.run("ai.undo", json!({})).is_err(), "readonly task view rejects an old effect restore");
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiClear" || id == "button:aiUndo"));
    // Catches readonly diagnostics clipped from a supported short pane; misses native window rendering.
    h.app.run("ui.dock", json!({"operation":"float","panel":"tools","rect":[450,100,320,120]})).unwrap();
    h.app.run("ai.task", json!({})).unwrap();
    frames(&mut h);
    super::panel_tests::reachable(&mut h, "button:aiCopyTrace", -1.0);
    request(&mut h, "ui.clickWidget", json!({"id":"button:aiCopyTrace"}));
    assert_eq!(h.view.clipboard, "123456789abc");
    assert_eq!(h.app.session.develop_of(other).unwrap().light.exposure, 0.7);
    assert!(requests.try_recv().is_err());
}

// Verifies implemented UI through actual frame/control surfaces; no native OS compositor claim.
#[test]
fn health_and_global_entry_are_readable_in_bilingual_normal_narrow_and_short_tools() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        for layout in ["normal", "narrow", "float120", "float240", "stack120", "stack240"] {
            let app = LightcraftApp::new(Session::with_demo(), Services { companion_address: Some("192.0.2.1:18080".into()), ..Default::default() });
            let mut h = Headless::new(app, if layout == "narrow" { [880.0, 600.0] } else { [1280.0, 800.0] }, 1.0);
            h.app.ui.language = language;
            let (connection, requests, events) = network::test_connection();
            h.app.ai.connection = Some(connection);
            h.app.ai.generation = h.app.session.library_generation();
            h.app.run("ai.open", json!({})).unwrap();
            frames(&mut h);
            request(&mut h, "ui.clickWidget", json!({"id":"checkbox:aiConsent"}));
            let photo = h.app.session.active().unwrap();
            h.app.run("ai.start", json!({"prompt":"Explain this photograph"})).unwrap();
            let create = requests.recv_timeout(Duration::from_secs(2)).unwrap();
            events.send(network::Event::Server{kind:"round.state".into(),data:json!({"round_id":"layout-round","payload":{"submission_seq":create.submission,"photo_ref":create.body["photo_ref"],"state":"analyzing_initial"}})}).unwrap();
            frames(&mut h);
            if layout.starts_with("float") || layout.starts_with("stack") {
                super::panel_tests::short_tools(&mut h, layout.starts_with("stack"), if layout.ends_with("120") { 120.0 } else { 240.0 });
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while h.app.run("ai.status", json!({})).unwrap()["health"]["state"] != "unreachable" {
                h.step();
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            let task = h.app.widgets.iter().find(|(id, _)| id == "button:aiTaskEntry").unwrap().1;
            let tools = h.app.widgets.iter().find(|(id, _)| id == "panel:right_panel").unwrap().1;
            assert!(!task.intersects(tools));
            for literal in [crate::i18n::tr("AI Task"), crate::i18n::tr("AI is analyzing the photo…")] {
                assert!(h.view.painted_shapes().iter().any(|shape|matches!(shape,egui::epaint::Shape::Text(t) if t.opacity_factor>0.0 && t.galley.text()==literal && task.contains_rect(t.visual_bounding_rect()))),"{layout}/{} task literal {literal}",language.code());
            }
            let retry = super::panel_tests::reachable(&mut h, "button:aiHealthRetry", -1.0);
            let literal = crate::i18n::tr("Check Again");
            assert!(h.view.painted_shapes().iter().any(|shape|matches!(shape,egui::epaint::Shape::Text(t) if t.opacity_factor>0.0 && t.galley.text()==literal && retry.contains_rect(t.visual_bounding_rect()))),"{layout}/{} complete retry label",language.code());
            let expected = h.app.run("ai.status", json!({})).unwrap()["task"]["localRoundId"].clone();
            request(&mut h, "ui.clickWidget", json!({"id":"button:aiHealthRetry"}));
            frames(&mut h);
            assert_eq!(h.app.run("ai.status", json!({})).unwrap()["task"]["localRoundId"], expected);
            assert!(requests.try_recv().is_err(), "retry does not submit or reconnect");
            if let Ok(dir) = std::env::var("UX20_SCREENSHOT_DIR") {
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
                        json!({"path":format!("{dir}/retry-{layout}-{}.png",language.code()),"headless":true}),
                        Duration::from_secs(30)
                    )["ok"],
                    true
                );
            }
            super::panel_tests::reachable(&mut h, "button:aiStop", 1.0);
            if let Ok(dir) = std::env::var("UX20_SCREENSHOT_DIR") {
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
                        json!({"path":format!("{dir}/health-task-{layout}-{}.png",language.code()),"headless":true}),
                        Duration::from_secs(30)
                    )["ok"],
                    true
                );
            }
            let other = h.app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
            h.app.run("panel.close", json!({})).unwrap();
            h.app.run("library.select", json!({"ids":[other.0]})).unwrap();
            frames(&mut h);
            assert!(h.app.widgets.iter().any(|(id, _)| id == "button:aiTaskEntry"));
            assert_eq!(h.app.session.active(), Some(other));
            request(&mut h, "ui.clickWidget", json!({"id":"button:aiTaskEntry"}));
            frames(&mut h);
            assert_eq!(h.app.session.active(), Some(photo));
            events.send(network::Event::Closed("layout verification connection closed".into())).unwrap();
            frames(&mut h);
            h.app.session.execute("photo.delete", &json!({"ids":[photo.0]})).unwrap();
            h.app.run("library.select", json!({"ids":[other.0]})).unwrap();
            h.app.run("ai.task", json!({})).unwrap();
            frames(&mut h);
            super::panel_tests::reachable(&mut h, "button:aiSelectedConversation", -1.0);
            if let Ok(dir) = std::env::var("UX20_SCREENSHOT_DIR") {
                assert_eq!(
                    h.request(
                        "ui.screenshot",
                        json!({"path":format!("{dir}/readonly-{layout}-{}.png",language.code()),"headless":true}),
                        Duration::from_secs(30)
                    )["ok"],
                    true
                );
            }
            request(&mut h, "ui.clickWidget", json!({"id":"button:aiSelectedConversation"}));
            frames(&mut h);
            assert_eq!(h.app.session.active(), Some(other));
            assert_eq!(h.app.run("ai.status", json!({})).unwrap()["readonlyPhoto"], Value::Null);
        }
    }
}

// Catches losing Stop when a real task source disappears while B stays selected;
// misses OS pointer delivery and a live model. Only this owned procedural file is removed.
#[test]
fn unavailable_active_original_task_still_has_scoped_stop_after_real_input() {
    let path = format!(
        "/tmp/lightcraft-ux-execution-20261013/20/missing-active-{}.png",
        lightcraft_engine::companion::LocalRoundId::new().unwrap().as_str()
    );
    let bytes = lightcraft_engine::export::encode_image(
        &lightcraft_raster::Rgba8::filled(32, 24, [96, 128, 160, 255]),
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    std::fs::write(&path, bytes).unwrap();
    let mut session = Session::with_demo();
    let (loader, probe) = lightcraft_engine::files::fs_hooks();
    session.media.file_loader = Some(loader);
    session.media.file_probe = Some(probe);
    lightcraft_engine::import::import(&mut session, std::slice::from_ref(&path), lightcraft_engine::import::ImportMode::Add).unwrap();
    let photo = session.catalog.photos().find(|p| p.source == lightcraft_catalog::Source::File { path: path.clone() }).unwrap().id;
    session.execute("library.select", &json!({"ids":[photo.0]})).unwrap();
    let other = session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    let before = session.develop_of(other).unwrap();
    let mut app = LightcraftApp::new(session, Services::default());
    app.ui.settings.gpu = false;
    let (connection, requests, events) = network::test_connection();
    app.ai.connection = Some(connection);
    app.ai.generation = app.session.library_generation();
    let mut h = Headless::new(app, [1100.0, 750.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    frames(&mut h);
    request(&mut h, "ui.clickWidget", json!({"id":"checkbox:aiConsent"}));
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.app.session.media.availability.exists(&path) != Some(true) {
        h.step();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    h.app.run("ai.start", json!({"prompt":"Improve my photo"})).unwrap();
    let create = outgoing(&mut h, &requests, "/v1/rounds");
    events.send(network::Event::Server{kind:"round.state".into(),data:json!({"round_id":"missing-active","payload":{"submission_seq":create.submission,"photo_ref":create.body["photo_ref"],"state":"awaiting_input"}})}).unwrap();
    outgoing(&mut h, &requests, "/input");
    events.send(network::Event::Server{kind:"round.state".into(),data:json!({"round_id":"missing-active","payload":{"submission_seq":create.submission,"photo_ref":create.body["photo_ref"],"state":"analyzing_initial"}})}).unwrap();
    frames(&mut h);
    std::fs::remove_file(&path).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while h.app.session.media.availability.exists(&path) != Some(false) {
        h.step();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    h.app.run("library.select", json!({"ids":[other.0]})).unwrap();
    h.app.run("panel.close", json!({})).unwrap();
    frames(&mut h);
    request(&mut h, "ui.clickWidget", json!({"id":"button:aiTaskEntry"}));
    frames(&mut h);
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(
        status["readonlyPhoto"],
        photo.0,
        "{status} inspect:{} source:{:?}",
        request(&mut h, "ui.inspect", json!({})),
        h.app.session.catalog.photo(photo).unwrap().source
    );
    assert_eq!(h.app.session.active(), Some(other));
    assert_eq!(status["task"]["phase"], "analyzing_initial");
    assert_eq!(status["task"]["terminal"], Value::Null);
    request(&mut h, "ui.clickWidget", json!({"id":"button:aiStop"}));
    frames(&mut h);
    let cancel = requests.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(cancel.path, "/v1/rounds/missing-active/stop");
    assert_eq!(*h.app.session.develop_of(other).unwrap(), *before);
    assert_eq!(h.app.session.active(), Some(other));
    assert!(requests.try_recv().is_err(), "stopping A sends no new photo input or round");
}

// Catches losing persisted trace IDs on real cold scoped history reads, including pure answers;
// misses physical OS clipboard integration and model/provider diagnostics quality.
#[test]
fn cold_verified_history_keeps_trace_and_consent_controls_scoped_while_loading() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    for background in [false, true] {
        let catalog = MemStore::new();
        let mut files = MemStore::new();
        files.background = background;
        let open = |catalog: MemStore, files: MemStore| {
            let mut session = Session::new();
            session
                .open_library_in(LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "cold-trace".into(), on_disk: false }, true)
                .unwrap();
            session
        };
        let mut session = open(catalog.clone(), files.clone());
        let photo = session.active().unwrap();
        let other = session.catalog.photos().find(|p| p.id != photo).unwrap().id;
        session.save_companion_conversation(photo,json!({"schema_version":2,"messages":[{"role":"assistant","text":"Saved failure remains readable.","local_round_id":"cold-trace-round"}],"round_records":[{"photo_id":photo.0,"local_round_id":"cold-trace-round","local_terminal":"failed","trace_id":"abcdef012345-cold","stages":[],"request":"ignored large source context".repeat(120_000)}],"rounds":[]})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while session.companion_history_unsaved(photo) {
            session.persist().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        session.persist().unwrap();
        drop(session);
        let mut h = Headless::new(
            LightcraftApp::new(open(catalog, files), Services { companion_address: Some("192.0.2.1:18080".into()), ..Default::default() }),
            [1100.0, 750.0],
            1.0,
        );
        h.app.run("ai.open", json!({})).unwrap();
        // Catches asynchronous diagnostic completion invalidating a held consent click;
        // misses physical OS input. Fallback continuation gives a deterministic pending read.
        let held = if !background {
            frames(&mut h);
            assert_eq!(h.app.run("ai.status", json!({})).unwrap()["historyLoading"], true);
            let rect = h.app.widgets.iter().find(|(id, _)| id == "checkbox:aiConsent").unwrap().1;
            let point = rect.center();
            h.app.synthetic.extend([
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
            ]);
            h.step();
            Some((rect, point))
        } else {
            None
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            frames(&mut h);
            let status = h.app.run("ai.status", json!({})).unwrap();
            if status["historyLoading"] == false {
                assert_eq!(status["resultCards"], json!([]), "a pure answer does not gain an editing card");
                assert!(status["text"].as_str().unwrap().contains("Saved failure remains readable."));
                break;
            }
            assert!(Instant::now() < deadline, "{status}");
            std::thread::yield_now();
        }
        let mut owned = None;
        if let Some((before, point)) = held {
            assert_eq!(
                h.app.widgets.iter().find(|(id, _)| id == "checkbox:aiConsent").unwrap().1,
                before,
                "cold trace completion must not move a held consent control"
            );
            h.app.synthetic.push(egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
            frames(&mut h);
            let (connection, requests, events) = network::test_connection();
            h.app.ai.connection = Some(connection);
            h.app.ai.generation = h.app.session.library_generation();
            owned = Some((requests, events));
        }
        request(&mut h, "ui.clickWidget", json!({"id":"button:aiCopyTrace"}));
        assert_eq!(h.view.clipboard, "abcdef012345-cold");
        if let Some((requests, _events)) = owned.as_ref() {
            assert_eq!(h.app.run("ai.start", json!({"prompt":"new explicit request"})).unwrap()["started"], true);
            assert_eq!(requests.recv_timeout(Duration::from_secs(1)).unwrap().path, "/v1/rounds");
        }
        h.app.run("library.select", json!({"ids":[other.0]})).unwrap();
        frames(&mut h);
        assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiCopyTrace"), "B cannot copy A's diagnostic");
        h.app
            .session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(MemStore::new()),
                    files: Box::new(MemStore::new()),
                    dir: "other-cold-library".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        h.app.run("ai.open", json!({})).unwrap();
        frames(&mut h);
        assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiCopyTrace"), "same photo IDs in a new library cannot reuse old diagnostics");
    }
}
