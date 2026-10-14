//! Transport acceptance: a confirmed rejection is different from uncertain delivery.
use super::*;
#[test]
fn only_complete_structured_client_errors_are_confirmed_rejections() {
    for (status, incomplete, rejected) in [(409, false, true), (429, false, true), (503, false, false), (409, true, false)] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(socket);
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.strip_prefix("Content-Length: ") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let reply = r#"{"code":"queue_full","trace_id":"abcdef123456"}"#;
            let length = reply.len() + if incomplete { 3 } else { 0 };
            write!(reader.get_mut(), "HTTP/1.0 {status} Error\r\nContent-Length: {length}\r\n\r\n{reply}").unwrap();
        });
        let error = post_request(address, "/v1/rounds", "", &json!({}), &AtomicBool::new(false)).err().unwrap();
        assert_eq!(error.rejected, rejected, "status={status}, incomplete={incomplete}");
        server.join().unwrap();
    }
}

#[test]
fn cancellation_after_partial_write_stops_remaining_content() {
    struct CancellingWriter {
        bytes: usize,
        round: Arc<AtomicBool>,
    }
    impl Write for CancellingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let n = bytes.len().min(7);
            self.bytes += n;
            self.round.store(true, Ordering::Relaxed);
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let round = Arc::new(AtomicBool::new(false));
    let mut writer = CancellingWriter { bytes: 0, round: round.clone() };
    let error = write_request(&mut writer, &[1; 64 * 1024], &AtomicBool::new(false), Some(&round), &mut 0, Instant::now()).unwrap_err();
    assert!(!error.rejected);
    assert!(!error.cancelled_before_send);
    assert_eq!(writer.bytes, 7, "cancelled request must never write another block");
}

#[test]
fn cancelled_create_never_opens_a_socket() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let round = AtomicBool::new(true);
    let error = post_scoped(listener.local_addr().unwrap(), "/v1/rounds", "", &json!({}), &AtomicBool::new(false), Some(&round)).unwrap_err();
    assert!(error.cancelled_before_send);
    assert!(matches!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock));
}

#[test]
fn encoded_content_is_cancelled_before_its_first_byte() {
    let mut writer = Vec::new();
    let data = serde_json::to_vec(&json!({"preview":"already encoded"})).unwrap();
    let error = write_request(&mut writer, &data, &AtomicBool::new(false), Some(&AtomicBool::new(true)), &mut 0, Instant::now()).unwrap_err();
    assert!(error.cancelled_before_send);
    assert!(writer.is_empty());
}

#[test]
fn stop_has_reserved_capacity_and_precedes_cancelled_content() {
    use lightcraft_engine::companion::{LocalRoundId, PhotoScope};
    let identity = RoundIdentity {
        scope: PhotoScope { library_generation: 1, photo: lightcraft_catalog::PhotoId(1) },
        local_round_id: LocalRoundId::new().unwrap(),
        submission_seq: 1,
        connection_generation: 1,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let request = |path: &str| Request {
        path: path.into(),
        body: json!({}),
        submission: 0,
        round: "round".into(),
        identity: identity.clone(),
        cancel: cancel.clone(),
    };
    let (content_tx, content) = mpsc::sync_channel(8);
    let (control_tx, controls) = mpsc::sync_channel(8);
    for _ in 0..8 {
        content_tx.try_send(request("/v1/rounds/round/input")).unwrap();
    }
    assert!(content_tx.try_send(request("/v1/rounds/round/input")).is_err());
    cancel.store(true, Ordering::Relaxed);
    control_tx.try_send(request("/v1/rounds/round/stop")).unwrap();
    let stop = next_request(&content, &controls).unwrap();
    assert!(stop.path.ends_with("/stop"));
    let mut encoded = Vec::new();
    write_request(&mut encoded, b"stop control", &AtomicBool::new(false), None, &mut 0, Instant::now()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(encoded, b"stop control");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    for _ in 0..8 {
        let r = next_request(&content, &controls).unwrap();
        assert!(
            post_scoped(listener.local_addr().unwrap(), &r.path, "", &r.body, &AtomicBool::new(false), Some(&r.cancel))
                .unwrap_err()
                .cancelled_before_send
        );
    }
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn cancelling_while_waiting_for_http_receipt_closes_only_that_request_promptly() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let round = Arc::new(AtomicBool::new(false));
    let server_round = round.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length: ") {
                length = n.trim().parse::<usize>().unwrap();
            }
        }
        reader.read_exact(&mut vec![0; length]).unwrap();
        server_round.store(true, Ordering::Relaxed);
        done_rx.recv_timeout(Duration::from_secs(1)).expect("cancelled POST remained blocked on the missing receipt");
        let mut byte = [0];
        assert_eq!(reader.read(&mut byte).unwrap(), 0, "cancel closes the content socket");
    });
    let error = post_scoped(address, "/v1/rounds", "", &json!({"photo":"fixture"}), &AtomicBool::new(false), Some(&round)).unwrap_err();
    assert!(!error.cancelled_before_send, "already delivered creation remains uncertain");
    assert!(!error.rejected);
    done_tx.send(()).unwrap();
    server.join().unwrap();
}

#[test]
fn real_connection_discards_cancelled_queue_but_delivers_priority_stop_and_heartbeat() {
    use lightcraft_engine::companion::{LocalRoundId, PhotoScope};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let client = "a".repeat(32);
    let owner = "b".repeat(32);
    let token = "c".repeat(32);
    let (registered_tx, registered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (verified_tx, verified_rx) = mpsc::channel();
    fn read_request(stream: TcpStream) -> (BufReader<TcpStream>, String, Value) {
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut reader = BufReader::new(stream);
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        let mut size = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length: ") {
                size = n.trim().parse::<usize>().unwrap();
            }
        }
        let mut body = vec![0; size];
        reader.read_exact(&mut body).unwrap();
        let body = if body.is_empty() { Value::Null } else { serde_json::from_slice(&body).unwrap() };
        (reader, first, body)
    }
    fn reply(reader: &mut BufReader<TcpStream>, status: u16, body: Value) {
        let body = body.to_string();
        write!(reader.get_mut(), "HTTP/1.0 {status} OK\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    }
    let server = std::thread::spawn(move || {
        let (mut registration, first, _) = read_request(listener.accept().unwrap().0);
        assert!(first.starts_with("POST /v1/clients "));
        registered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        reply(&mut registration, 200, json!({"client_id":client,"client_token":token}));
        drop(registration);
        let (mut events, first, _) = read_request(listener.accept().unwrap().0);
        assert!(first.starts_with(&format!("GET /v1/clients/{client}/events ")));
        write!(
            events.get_mut(),
            "HTTP/1.0 200 OK\r\n\r\nevent: connected\ndata: {}\n\n",
            json!({"protocol_version":"v1","event_seq":1,"connection_id":owner})
        )
        .unwrap();
        let (mut stop, first, body) = read_request(listener.accept().unwrap().0);
        assert!(first.starts_with("POST /v1/rounds/round/stop "), "content overtook control: {first}");
        assert_eq!(body["reason"], "user_stop");
        reply(&mut stop, 202, json!({"accepted":true}));
        drop(stop);
        // Heartbeat runs on the SSE reader, independent of cancelled content.
        write!(
            events.get_mut(),
            "event: ping\ndata: {}\n\n",
            json!({"protocol_version":"v1","event_seq":2,"connection_id":owner,"payload":{"ping_seq":1}})
        )
        .unwrap();
        let (mut heartbeat, first, body) = read_request(listener.accept().unwrap().0);
        assert!(first.starts_with(&format!("POST /v1/clients/{client}/heartbeat ")));
        assert_eq!(body["ping_seq"], 1);
        reply(&mut heartbeat, 200, json!({"accepted":true}));
        drop(heartbeat);
        verified_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        listener.set_nonblocking(true).unwrap();
        assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock, "cancelled contents reached the backend");
    });
    let connection = connect(&address.to_string()).unwrap();
    registered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let gate = Arc::new(AtomicBool::new(false));
    let identity = RoundIdentity {
        scope: PhotoScope { library_generation: 1, photo: lightcraft_catalog::PhotoId(1) },
        local_round_id: LocalRoundId::new().unwrap(),
        submission_seq: 1,
        connection_generation: 1,
    };
    for i in 0..8 {
        connection
            .requests
            .try_send(Request {
                path: if i == 0 { "/v1/rounds".into() } else { "/v1/rounds/round/input".into() },
                body: json!({"preview":"not uploaded"}),
                submission: u64::from(i == 0),
                round: "round".into(),
                identity: identity.clone(),
                cancel: gate.clone(),
            })
            .unwrap();
    }
    gate.store(true, Ordering::Relaxed);
    connection
        .controls
        .try_send(Request {
            path: "/v1/rounds/round/stop".into(),
            body: json!({"reason":"user_stop"}),
            submission: 0,
            round: "round".into(),
            identity,
            cancel: gate,
        })
        .unwrap();
    release_tx.send(()).unwrap();
    assert!(matches!(connection.events.recv_timeout(Duration::from_secs(3)).unwrap(), Event::Connected { .. }));
    let mut responses = 0;
    let mut heartbeat = false;
    while responses < 9 || !heartbeat {
        match connection.events.recv_timeout(Duration::from_secs(3)).unwrap() {
            Event::Heartbeat { sequence } => {
                assert_eq!(sequence, 1);
                heartbeat = true;
            }
            Event::Response { path, result, .. } => {
                if responses == 0 {
                    assert!(path.ends_with("/stop"));
                    assert!(result.is_ok());
                } else {
                    assert!(result.unwrap_err().cancelled_before_send);
                }
                responses += 1;
            }
            _ => panic!("unexpected transport event"),
        }
    }
    assert_eq!(responses, 9);
    verified_tx.send(()).unwrap();
    server.join().unwrap();
    connection.close();
}

#[test]
fn execution_receipt_requires_complete_202_and_explicit_acceptance() {
    for (status, body, incomplete, accepted) in [
        (202, r#"{"accepted":true}"#, false, true),
        (202, r#"{"accepted":false}"#, false, false),
        (202, "{}", false, false),
        (200, r#"{"accepted":true}"#, false, false),
        (202, r#"{"accepted":true}"#, true, false),
    ] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(socket);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(n) = line.strip_prefix("Content-Length: ") {
                    length = n.trim().parse().unwrap();
                }
            }
            let mut request = vec![0; length];
            reader.read_exact(&mut request).unwrap();
            write!(reader.get_mut(), "HTTP/1.0 {status} OK\r\nContent-Length: {}\r\n\r\n{body}", body.len() + usize::from(incomplete)).unwrap();
        });
        let result = post_request(address, "/v1/rounds/r/execution", "", &json!({"plan_id":"p"}), &AtomicBool::new(false));
        assert_eq!(result.is_ok(), accepted, "status={status} body={body} incomplete={incomplete}");
        server.join().unwrap();
    }
}
