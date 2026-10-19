//! Observable edit progress through actual network events and the native UI.
use super::*;
use crate::headless::Headless;

fn live() -> (Headless, mpsc::Receiver<network::Request>, mpsc::SyncSender<network::Event>, network::Request) {
    live_in(lightcraft_engine::Session::with_demo())
}

fn live_in(session: lightcraft_engine::Session) -> (Headless, mpsc::Receiver<network::Request>, mpsc::SyncSender<network::Event>, network::Request) {
    let mut app = LightcraftApp::new(session, crate::Services::default());
    app.ui.settings.gpu = false;
    let (connection, requests, events) = network::test_connection();
    app.ai.connection = Some(connection);
    app.ai.generation = app.session.library_generation();
    let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    for _ in 0..4 {
        h.step();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    if let Some(lightcraft_catalog::Source::File { path }) =
        h.app.session.active().and_then(|photo| h.app.session.catalog.photo(photo)).map(|photo| photo.source.clone())
    {
        while h.app.session.media.availability.exists(&path) != Some(true) {
            assert!(Instant::now() < deadline, "owned procedural file availability");
            h.step();
        }
    }
    h.app.run("ai.start", json!({"prompt":"Improve this photo"})).unwrap();
    let create = native_history_tests::outgoing(&mut h, &requests, "/v1/rounds");
    events.send(network::Event::Server { kind:"round.state".into(), data:json!({"round_id":"write-progress-round","payload":{"submission_seq":create.identity.submission_seq,"photo_ref":create.body["photo_ref"],"state":"awaiting_input"}}) }).unwrap();
    native_history_tests::outgoing(&mut h, &requests, "/input");
    (h, requests, events, create)
}

fn proposal(events: &mpsc::SyncSender<network::Event>, create: &network::Request, stage: &str, call: &str, exposure: f64) {
    events.send(network::Event::Server {kind:"round.state".into(),data:json!({"round_id":"write-progress-round","payload":{"submission_seq":create.identity.submission_seq,"photo_ref":create.body["photo_ref"],"state":format!("awaiting_{stage}_execution")}})}).unwrap();
    events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"write-progress-round","stage":stage,"payload":{"message_id":call,"model_call_id":format!("model-{call}"),"tool_calls":[{"id":call,"name":"apply_edit_plan","arguments":{"operations":[{"type":"set_controls","values":{"light.exposure":exposure}}]}}]}})}).unwrap();
}

#[test]
fn proposed_adjustments_do_not_claim_local_execution_in_either_language() {
    let (mut h, _requests, events, create) = live();
    let photo = h.app.session.active().unwrap();
    let before = h.app.session.develop_of(photo).unwrap();
    proposal(&events, &create, "initial", "edit", 0.4);
    for (language, expected) in [(crate::i18n::Locale::En, "Preparing adjustments"), (crate::i18n::Locale::ZhHans, "正在准备调整方案")] {
        h.app.ui.language = language;
        for _ in 0..4 {
            h.step();
        }
        let paint = h.painted_text().join(" ");
        assert!(paint.contains(expected), "{paint}");
        for incorrect in ["Applying the edit plan", "Applying adjustments", "正在应用调整", "Viewed 1 type", "查看了 1 项"] {
            assert!(!paint.contains(incorrect), "proposal alone cannot show {incorrect}: {paint}");
        }
        assert_eq!(h.app.widgets.iter().filter(|(id, _)| id.starts_with("button:aiToolSegment:")).count(), 1);
        assert_eq!(h.app.session.develop_of(photo).unwrap(), before);
    }
    h.app.run("ai.stop", json!({})).unwrap();
}

#[test]
fn continuous_read_and_write_progress_keeps_success_count_during_prepare_apply_and_publish() {
    fn mixed_caption(h: &mut Headless, name: &str, en: &str, zh: &str) {
        for (command, locale, state, count) in
            [("app.language.english", "en", en, "2 types"), ("app.language.simplifiedChinese", "zh-hans", zh, "2 类信息")]
        {
            h.app.run(command, json!({})).unwrap();
            for width in [1000.0, 1400.0] {
                h.size.x = width;
                for _ in 0..4 {
                    h.step();
                }
                let pane = h.app.widgets.iter().find(|(id, _)| id == "ai:messages").unwrap().1;
                let mut found = false;
                for shape in h.view.painted_shapes() {
                    if let egui::epaint::Shape::Text(text) = shape
                        && text.galley.text().contains(state)
                        && text.galley.text().contains(count)
                    {
                        let visible: String = text.galley.rows.iter().flat_map(|row| row.glyphs.iter().map(|glyph| glyph.chr)).collect();
                        assert!(visible.contains(state) && visible.contains(count), "mixed caption must show both facts: {visible}");
                        assert!(text.visual_bounding_rect().right() <= pane.right() + 1.0);
                        assert!(
                            !text
                                .galley
                                .text()
                                .split_whitespace()
                                .any(|word| word.strip_suffix('s').is_some_and(|number| number.parse::<f64>().is_ok())),
                            "untimed write cannot produce a complete segment total"
                        );
                        found = true;
                    }
                }
                assert!(found, "visible mixed progress {locale}: {:?}", h.painted_text());
                super::redesign_tests::screenshots(h, &format!("tool-mixed-{name}-{locale}-{width}"));
            }
        }
        h.app.run("app.language.english", json!({})).unwrap();
        for _ in 0..4 {
            h.step();
        }
    }
    let (_file, session) = PhotoFile::open();
    let (mut h, requests, events, create) = live_in(session);
    let photo = h.app.session.active().unwrap();
    events
        .send(network::Event::Server {
            kind: "round.state".into(),
            data: json!({"round_id":"write-progress-round","payload":{"state":"analyzing_initial","submission_seq":create.identity.submission_seq,"photo_ref":create.body["photo_ref"]}}),
        })
        .unwrap();
    for (n, name) in ["get_photo_state", "get_photo_histogram"].iter().enumerate() {
        events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"write-progress-round","stage":"initial","payload":{"message_id":format!("read-{n}"),"model_call_id":n+1,"tool_calls":[{"id":format!("read-{n}"),"name":name,"arguments":{}}]}})}).unwrap();
        events.send(network::Event::Server {kind:"tool.request".into(),data:json!({"round_id":"write-progress-round","payload":{"tool_call_id":format!("read-{n}"),"tool_request_id":format!("request-{n}"),"tool_name":name,"expected_revision":h.app.session.photo_revision(photo)}})}).unwrap();
        native_history_tests::outgoing(&mut h, &requests, "/tool-results");
    }
    proposal(&events, &create, "initial", "mixed-edit", 0.4);
    for _ in 0..4 {
        h.step();
    }
    let captions: Vec<_> = h.painted_text().into_iter().filter(|text| text.contains("Preparing edits")).collect();
    assert!(captions.iter().any(|text| text.contains("2 types")), "pending write cannot erase completed reads: {captions:?}");
    mixed_caption(&mut h, "preparing", "Preparing edits", "准备调整");
    let (gate, entered) = ReadGate::install(&mut h);
    plan(&h, &events, "initial", "mixed-edit", 0.4);
    h.step();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let captions: Vec<_> = h.painted_text().into_iter().filter(|text| text.contains("Applying edits")).collect();
    assert!(captions.iter().any(|text| text.contains("2 types")), "applying write cannot erase completed reads: {captions:?}");
    mixed_caption(&mut h, "applying", "Applying edits", "正在应用调整");
    assert_eq!(h.app.widgets.iter().filter(|(id, _)| id.starts_with("button:aiToolSegment:")).count(), 1, "empty prose keeps one continuous segment");
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.0);
    gate.release();
    native_history_tests::outgoing(&mut h, &requests, "/execution");
    for _ in 0..4 {
        h.step();
    }
    let captions: Vec<_> = h.painted_text().into_iter().filter(|text| text.contains("Adjustments applied")).collect();
    assert!(captions.iter().any(|text| text.contains("2 types")), "publication cannot erase completed reads: {captions:?}");
    mixed_caption(&mut h, "published", "Adjustments applied", "调整已应用");
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.4);
    assert!(!h.painted_text().join(" ").contains("Viewed 3"), "writes are not another information category");
    // The published feedback closes that segment; review prose and its genuine
    // read must appear afterwards, without inheriting the initial count.
    events.send(network::Event::Server {kind:"round.state".into(),data:json!({"round_id":"write-progress-round","payload":{"state":"analyzing_review","submission_seq":create.identity.submission_seq,"photo_ref":create.body["photo_ref"]}})}).unwrap();
    events
        .send(network::Event::Server {
            kind: "text.delta".into(),
            data: json!({"round_id":"write-progress-round","payload":{"message_id":"review-read","delta":"I will inspect the published result."}}),
        })
        .unwrap();
    events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"write-progress-round","stage":"review","payload":{"message_id":"review-read","model_call_id":4,"tool_calls":[{"id":"review-preview","name":"get_current_preview","arguments":{}}]}})}).unwrap();
    events.send(network::Event::Server {kind:"tool.request".into(),data:json!({"round_id":"write-progress-round","payload":{"tool_call_id":"review-preview","tool_request_id":"request-review-preview","tool_name":"get_current_preview","expected_revision":h.app.session.photo_revision(photo)}})}).unwrap();
    let review = native_history_tests::outgoing(&mut h, &requests, "/tool-results");
    assert_eq!(review.body["outcome"], "succeeded");
    for _ in 0..5 {
        h.step();
    }
    let paint = h.painted_text().join(" ");
    assert!(paint.contains("2 types") && paint.contains("Viewed 1 info type · "), "review has its own successful category: {paint}");
    assert_eq!(h.app.widgets.iter().filter(|(id, _)| id.starts_with("button:aiToolSegment:")).count(), 2);
    events
        .send(network::Event::Server {
            kind: "text.delta".into(),
            data: json!({"round_id":"write-progress-round","payload":{"message_id":"final-answer","delta":"The natural exposure is preserved."}}),
        })
        .unwrap();
    events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"write-progress-round","stage":"review","payload":{"message_id":"final-answer","model_call_id":5,"tool_calls":[]}})}).unwrap();
    events
        .send(network::Event::Server {
            kind: "round.ended".into(),
            data: json!({"round_id":"write-progress-round","payload":{"terminal_state":"completed","last_confirmed_plan_id":"plan-mixed-edit"}}),
        })
        .unwrap();
    for _ in 0..5 {
        h.step();
    }
    assert!(!h.app.widgets.iter().any(|(id, _)| id.starts_with("button:aiToolSegment:")), "normal completion folds both segments");
    assert!(h.painted_text().join(" ").contains("The natural exposure is preserved."));
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["resultCards"].as_array().unwrap().len(), 1);
    for (command, locale) in [("app.language.english", "en"), ("app.language.simplifiedChinese", "zh-hans")] {
        h.app.run(command, json!({})).unwrap();
        for width in [1000.0, 1400.0] {
            h.size.x = width;
            for _ in 0..4 {
                h.step();
            }
            assert!(h.painted_text().join(" ").contains("The natural exposure is preserved."));
            super::redesign_tests::screenshots(&mut h, &format!("journey-completed-{locale}-{width}"));
            let inspect = h.request("ui.inspect", json!({}), Duration::from_secs(10));
            eprintln!("journey completed {locale} {width}: {}", inspect["result"]["perf"]);
        }
    }
}

struct PhotoFile(std::path::PathBuf);
impl PhotoFile {
    fn open() -> (Self, lightcraft_engine::Session) {
        let path = std::env::temp_dir().join(format!("lightcraft-write-progress-{}.png", LocalRoundId::new().unwrap().as_str()));
        let bytes = lightcraft_engine::export::encode_image(
            &lightcraft_raster::Rgba8::filled(32, 24, [96, 128, 160, 255]),
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
        )
        .unwrap();
        std::fs::write(&path, bytes).unwrap();
        let mut session = lightcraft_engine::Session::with_demo();
        let (loader, probe) = lightcraft_engine::files::fs_hooks();
        session.media.file_loader = Some(loader);
        session.media.file_probe = Some(probe);
        let source = path.to_string_lossy().into_owned();
        lightcraft_engine::import::import(&mut session, std::slice::from_ref(&source), lightcraft_engine::import::ImportMode::Add).unwrap();
        let photo = session.catalog.photos().find(|p| p.source == lightcraft_catalog::Source::File { path: source.clone() }).unwrap().id;
        session.execute("library.select", &json!({"ids":[photo.0],"active":photo.0})).unwrap();
        (Self(path), session)
    }
}
impl Drop for PhotoFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct ReadGate(Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);
impl ReadGate {
    fn release(&self) {
        let (lock, ready) = &*self.0;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }
    fn install(h: &mut Headless) -> (Self, mpsc::Receiver<()>) {
        let gate = Self(Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())));
        let held = gate.0.clone();
        let (entered, waiting) = mpsc::sync_channel(16);
        let (loader, _) = lightcraft_engine::files::fs_hooks();
        h.app.session.media.clear_sources();
        h.app.session.media.file_loader = Some(Arc::new(move |path, edge| {
            let _ = entered.try_send(());
            let (lock, ready) = &*held;
            let released = ready
                .wait_timeout_while(lock.lock().map_err(|_| "test gate poisoned")?, Duration::from_secs(20), |released| !*released)
                .map_err(|_| "test gate poisoned")?;
            if !*released.0 {
                return Err("test render gate timed out".into());
            }
            loader(path, edge)
        }));
        (gate, waiting)
    }
}
impl Drop for ReadGate {
    fn drop(&mut self) {
        self.release();
    }
}

fn plan(h: &Headless, events: &mpsc::SyncSender<network::Event>, stage: &str, call: &str, exposure: f64) {
    let photo = h.app.session.active().unwrap();
    events.send(network::Event::Server {kind:"edit.plan".into(),data:json!({"round_id":"write-progress-round","stage":stage,"payload":{"plan_id":format!("plan-{call}"),"tool_call_id":call,"capability_version":"lightcraft-v2","expected_revision":h.app.session.photo_revision(photo),"operations":[{"type":"set_controls","values":{"light.exposure":exposure}}]}})}).unwrap();
}

#[test]
fn real_render_progress_changes_only_at_local_dispatch_and_publication_and_survives_feedback_failure() {
    let (_file, session) = PhotoFile::open();
    let (mut h, requests, events, create) = live_in(session);
    let photo = h.app.session.active().unwrap();
    let before = h.app.session.develop_of(photo).unwrap();
    proposal(&events, &create, "initial", "edit", 0.4);
    for _ in 0..3 {
        h.step();
    }
    let (gate, entered) = ReadGate::install(&mut h);
    plan(&h, &events, "initial", "edit", 0.4);
    h.step();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let paint = h.painted_text().join(" ");
    assert!(paint.contains("Applying adjustments"), "{paint}");
    assert!(!paint.contains("Preparing adjustments") && !paint.contains("Edits applied."), "{paint}");
    assert_eq!(h.app.session.develop_of(photo).unwrap(), before);
    captures(&mut h, "applying", "Applying adjustments", "正在应用调整");
    h.app.ui.language = crate::i18n::Locale::En;
    gate.release();
    let execution = native_history_tests::outgoing(&mut h, &requests, "/execution");
    for _ in 0..4 {
        h.step();
    }
    let paint = h.painted_text().join(" ");
    assert!(paint.contains("Adjustments applied; awaiting confirmation"), "{paint}");
    assert!(!paint.contains("Applying adjustments") && !paint.contains("Preparing adjustments"), "{paint}");
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.4);
    assert!(!paint.contains("Viewed 1 type of photo information"));
    captures(&mut h, "published", "Adjustments applied; awaiting confirmation", "调整已应用，正在等待反馈确认");
    h.app.ui.language = crate::i18n::Locale::En;
    let mut failure: network::RequestFailure = "feedback transport failed".into();
    failure.execution_plan_id = execution.body["plan_id"].as_str().map(str::to_owned);
    events
        .send(network::Event::Response {
            submission: execution.submission,
            round: execution.round,
            path: execution.path,
            identity: execution.identity,
            result: Err(failure),
        })
        .unwrap();
    for _ in 0..4 {
        h.step();
    }
    open_process(&mut h, create.identity.local_round_id.as_str());
    let paint = h.painted_text().join(" ");
    assert!(paint.contains("Adjustments applied; feedback not delivered"), "{paint}");
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.4);
    assert!(!paint.contains("photo remains at this round's starting settings") && !paint.contains("plan_proposed"));
    captures(&mut h, "feedback-failed", "Adjustments applied; feedback not delivered", "调整已应用，执行反馈未送达");
}

#[test]
fn completed_round_uses_backend_plan_confirmation_and_never_leaves_a_feedback_wait_running() {
    for confirmed in [false, true] {
        let (mut h, requests, events, create) = live();
        let photo = h.app.session.active().unwrap();
        proposal(&events, &create, "initial", "edit", 0.4);
        plan(&h, &events, "initial", "edit", 0.4);
        native_history_tests::outgoing(&mut h, &requests, "/execution");
        events.send(network::Event::Server {kind:"round.ended".into(),data:json!({"round_id":"write-progress-round","payload":{"terminal_state":"completed","last_confirmed_plan_id":if confirmed {Some("plan-edit")} else {None}}})}).unwrap();
        for _ in 0..4 {
            h.step();
        }
        let id = format!("button:aiProcess:{}", create.identity.local_round_id.as_str());
        assert_eq!(h.request("ui.clickWidget", json!({"id":id}), Duration::from_secs(10))["ok"], true);
        for _ in 0..4 {
            h.step();
        }
        let paint = h.painted_text().join(" ");
        assert!(
            !paint.contains("awaiting confirmation") && !paint.contains("Applying adjustments") && !paint.contains("Preparing adjustments"),
            "{paint}"
        );
        let expected = if confirmed { "Adjustments applied" } else { "Adjustments applied; feedback unconfirmed" };
        assert!(paint.contains(expected), "{paint}");
        if confirmed {
            assert!(!paint.contains("feedback unconfirmed"), "{paint}");
        }
        assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.4);
    }
}

#[test]
fn uncorrelated_feedback_failure_cannot_claim_every_initial_and_correction_receipt_failed() {
    let (mut h, requests, events, create) = live();
    let photo = h.app.session.active().unwrap();
    let mut first = None;
    for (stage, call, exposure) in [("initial", "initial", 0.4), ("correction", "correction", 0.2)] {
        proposal(&events, &create, stage, call, exposure);
        plan(&h, &events, stage, call, exposure);
        let execution = native_history_tests::outgoing(&mut h, &requests, "/execution");
        if first.is_none() {
            first = Some(execution);
        }
    }
    let execution = first.unwrap();
    events
        .send(network::Event::Response {
            submission: execution.submission,
            round: execution.round,
            path: execution.path,
            identity: execution.identity,
            result: Err("legacy uncorrelated feedback transport failure".into()),
        })
        .unwrap();
    for _ in 0..4 {
        h.step();
    }
    open_process(&mut h, create.identity.local_round_id.as_str());
    let paint = h.painted_text().join(" ");
    assert!(paint.contains("Adjustments applied; feedback unconfirmed"), "{paint}");
    assert!(!paint.contains("feedback not delivered"), "an uncorrelated failure cannot be assigned to every plan: {paint}");
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.2);
}

fn open_process(h: &mut Headless, round: &str) {
    if !h.app.widgets.iter().any(|(id, _)| id.starts_with("button:aiToolSegment:")) {
        let id = format!("button:aiProcess:{round}");
        // Automatic failure expansion adds readable content without changing the
        // reader's scroll intent. Scroll to the actual process before toggling.
        super::panel_tests::reachable(h, &id, 1.0);
        if !h.app.widgets.iter().any(|(id, _)| id.starts_with("button:aiToolSegment:")) {
            assert_eq!(h.request("ui.clickWidget", json!({"id":id}), Duration::from_secs(10))["ok"], true);
        }
    }
    for _ in 0..4 {
        h.step();
    }
}

fn captures(h: &mut Headless, phase: &str, english: &str, chinese: &str) {
    for (language, expected) in [(crate::i18n::Locale::En, english), (crate::i18n::Locale::ZhHans, chinese)] {
        h.app.ui.language = language;
        for width in [1000.0, 1400.0] {
            h.size.x = width;
            for _ in 0..4 {
                h.step();
            }
            assert!(h.painted_text().join(" ").contains(expected), "{:?}", h.painted_text());
            let pane = h.app.widgets.iter().find(|(id, _)| id == "ai:messages").unwrap().1;
            let segment = h.app.widgets.iter().find(|(id, _)| id.starts_with("button:aiToolSegment:")).unwrap().1;
            assert!(segment.height() < 30.0 && segment.right() <= pane.right() + 1.0, "single-line progress fits {segment:?} in {pane:?}");
            super::redesign_tests::screenshots(h, &format!("write-{phase}-{}-{width}", language.code()));
            let inspect = h.request("ui.inspect", json!({}), Duration::from_secs(10));
            assert!(inspect["result"]["perf"]["frameMs"].is_number(), "{inspect}");
            eprintln!("write-{phase} {} {width}: {}", language.code(), inspect["result"]["perf"]);
        }
    }
}

#[test]
fn rejected_or_failed_candidate_never_reports_applied_and_preserves_the_original_photo() {
    for rejected in [true, false] {
        let (_file, session) = PhotoFile::open();
        let (mut h, requests, events, create) = live_in(session);
        let photo = h.app.session.active().unwrap();
        let before = h.app.session.develop_of(photo).unwrap();
        let exposure = if rejected { 9.0 } else { 0.4 };
        proposal(&events, &create, "initial", "edit", exposure);
        for _ in 0..3 {
            h.step();
        }
        if !rejected {
            h.app.session.media.clear_sources();
            h.app.session.media.file_loader = Some(Arc::new(|_, _| Err("owned fixture decode failure".into())));
        }
        plan(&h, &events, "initial", "edit", exposure);
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.app.run("ai.status", json!({})).unwrap()["busy"] != false {
            assert!(Instant::now() < deadline, "failed candidate retires");
            h.step();
        }
        for _ in 0..4 {
            h.step();
        }
        open_process(&mut h, create.identity.local_round_id.as_str());
        let expected = if rejected { "Adjustments were not applied: plan rejected" } else { "Adjustments were not applied: rendering failed" };
        let paint = h.painted_text().join(" ");
        assert!(paint.contains(expected), "{paint}");
        assert!(!paint.contains("Edits applied.") && !paint.contains("Applying adjustments") && !paint.contains("Preparing adjustments"), "{paint}");
        assert_eq!(h.app.session.develop_of(photo).unwrap(), before);
        while let Ok(request) = requests.try_recv() {
            if request.path.ends_with("/execution") {
                assert_eq!(request.body["outcome"], "failed");
            }
        }
    }
}

#[test]
fn native_failed_execution_receipt_carries_only_its_actual_request_plan() {
    use std::io::{BufRead, Read, Write};
    use std::net::{TcpListener, TcpStream};
    fn read(socket: TcpStream) -> (std::io::BufReader<TcpStream>, Value) {
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reader = std::io::BufReader::new(socket);
        let mut size = 0;
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if line == "\r\n" {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length: ") {
                size = n.trim().parse::<usize>().unwrap();
            }
        }
        let mut body = vec![0; size];
        reader.read_exact(&mut body).unwrap();
        (reader, if body.is_empty() { Value::Null } else { serde_json::from_slice(&body).unwrap() })
    }
    fn reply(reader: &mut std::io::BufReader<TcpStream>, status: u16, body: Value) {
        let body = body.to_string();
        write!(reader.get_mut(), "HTTP/1.0 {status} OK\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (verified_tx, verified_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut registration, _) = read(listener.accept().unwrap().0);
        reply(&mut registration, 200, json!({"client_id":"a".repeat(32),"client_token":"b".repeat(32)}));
        drop(registration);
        let (mut events, _) = read(listener.accept().unwrap().0);
        write!(
            events.get_mut(),
            "HTTP/1.0 200 OK\r\n\r\nevent: connected\ndata: {}\n\n",
            json!({"protocol_version":"v1","event_seq":1,"connection_id":"c".repeat(32)})
        )
        .unwrap();
        let (mut execution, body) = read(listener.accept().unwrap().0);
        assert_eq!(body["plan_id"], "actual-plan");
        assert!(body.get("_client_execution_plan_id").is_none() && body.get("execution_plan_id").is_none());
        reply(&mut execution, 503, json!({"code":"network_failure","trace_id":"local-test"}));
        verified_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let connection = network::connect(&address.to_string()).unwrap();
    assert!(matches!(connection.events.recv_timeout(Duration::from_secs(5)).unwrap(), network::Event::Connected { .. }));
    let identity = RoundIdentity {
        scope: PhotoScope { library_generation: 1, photo: PhotoId(1) },
        local_round_id: LocalRoundId::new().unwrap(),
        submission_seq: 1,
        connection_generation: 1,
    };
    connection
        .requests
        .send(network::Request {
            path: "/v1/rounds/round/execution".into(),
            body: json!({"plan_id":"actual-plan","outcome":"succeeded"}),
            submission: 0,
            round: "round".into(),
            identity,
            cancel: Arc::new(AtomicBool::new(false)),
        })
        .unwrap();
    let network::Event::Response { result: Err(failure), .. } = connection.events.recv_timeout(Duration::from_secs(5)).unwrap() else {
        panic!("expected actual failed HTTP execution receipt")
    };
    assert_eq!(failure.execution_plan_id.as_deref(), Some("actual-plan"));
    assert_eq!(failure.code.as_deref(), Some("network_failure"));
    verified_tx.send(()).unwrap();
    connection.close();
    server.join().unwrap();
}

#[test]
fn positive_backend_confirmation_keeps_tool_progress_and_retained_result_card_consistent() {
    let (mut h, requests, events, create) = live();
    let photo = h.app.session.active().unwrap();
    proposal(&events, &create, "initial", "edit", 0.4);
    plan(&h, &events, "initial", "edit", 0.4);
    native_history_tests::outgoing(&mut h, &requests, "/execution");
    events.send(network::Event::Server {kind:"round.ended".into(),data:json!({"round_id":"write-progress-round","payload":{"terminal_state":"failed","reason_code":"model_failed","last_confirmed_plan_id":"plan-edit"}})}).unwrap();
    for _ in 0..4 {
        h.step();
    }
    open_process(&mut h, create.identity.local_round_id.as_str());
    let paint = h.painted_text().join(" ");
    assert!(
        !paint.contains("feedback was not confirmed") && !paint.contains("feedback unconfirmed"),
        "positive receipt proof must agree in card and progress: {paint}"
    );
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["resultCards"][0]["stages"][0]["feedback"], "confirmed", "{status}");
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.4);
}
