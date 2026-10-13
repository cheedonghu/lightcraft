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
