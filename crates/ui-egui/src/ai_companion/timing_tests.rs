//! Durable timing observed through real event dispatch, Session storage and UI.
use super::*;

#[test]
fn real_read_timing_survives_reopen_and_replay_without_entering_model_input() {
    let library = NativeLibrary::new();
    let (mut h, requests, events) = consent(library.open());
    let photo = h.app.session.active().unwrap();
    h.app.run("ai.start", json!({"prompt":"Read photo information"})).unwrap();
    let creation = outgoing(&mut h, &requests, "/v1/rounds");
    phase(&events, &creation, "awaiting_input");
    outgoing(&mut h, &requests, "/input");
    phase(&events, &creation, "analyzing_initial");
    let round = format!("native-round-{}", creation.identity.submission_seq);
    let local = creation.identity.local_round_id.as_str().to_owned();
    events.send(network::Event::Server { kind:"message.completed".into(), data:json!({"round_id":round,"stage":"initial","payload":{"message_id":"timed-read","model_call_id":1,"tool_calls":[{"id":"state","name":"get_photo_state","arguments":{}}]}}) }).unwrap();
    let payload = json!({"round_id":round,"payload":{"tool_call_id":"state","tool_request_id":"timed-request","tool_name":"get_photo_state","expected_revision":h.app.session.photo_revision(photo)}});
    events.send(network::Event::Server { kind: "tool.request".into(), data: payload.clone() }).unwrap();
    outgoing(&mut h, &requests, "/tool-results");
    let saved = h.app.session.companion_messages_page(photo, 0, 10).unwrap().records;
    let measured = saved.iter().find(|m| m["role"] == "tool").unwrap().clone();
    assert!(measured["display_elapsed_us"].as_u64().is_some(), "actual read must record dispatch time: {measured}");
    std::thread::sleep(Duration::from_millis(30));
    events.send(network::Event::Server { kind: "tool.request".into(), data: payload.clone() }).unwrap();
    outgoing(&mut h, &requests, "/tool-results");
    let replayed = h.app.session.companion_messages_page(photo, 0, 10).unwrap().records;
    assert_eq!(replayed.iter().filter(|m| m["role"] == "tool").count(), 1);
    assert_eq!(replayed.iter().find(|m| m["role"] == "tool").unwrap(), &measured);
    events
        .send(network::Event::Server { kind: "round.ended".into(), data: json!({"round_id":round,"payload":{"terminal_state":"completed"}}) })
        .unwrap();
    wait_idle(&mut h, &requests);
    events.send(network::Event::Server { kind: "tool.request".into(), data: payload }).unwrap();
    for _ in 0..4 {
        h.step();
    }
    assert!(requests.try_recv().is_err(), "late tool requests after terminal must remain retired");
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 10).unwrap().records.iter().filter(|m| m["role"] == "tool").count(), 1);
    before_library_change(&mut h.app).unwrap();
    h.app.session.close_library().unwrap();
    drop(h);
    let (mut h, requests, events) = consent(library.open());
    for _ in 0..6 {
        h.step();
    }
    assert_eq!(h.request("ui.clickWidget", json!({"id":format!("button:aiProcess:{local}")}), Duration::from_secs(10))["ok"], true);
    for _ in 0..4 {
        h.step();
    }
    let painted = h.painted_text().join(" ");
    assert!(painted.contains("Viewed 1 info type · ") && painted.contains('s'), "{painted}");
    h.app.run("ai.start", json!({"prompt":"Continue after reopening"})).unwrap();
    let creation = outgoing(&mut h, &requests, "/v1/rounds");
    phase(&events, &creation, "awaiting_input");
    let input = outgoing(&mut h, &requests, "/input");
    assert!(!input.body.to_string().contains("display_elapsed_us") && !input.body.to_string().contains("display_outcome"));
    assert_eq!(input.body["history"].as_array().unwrap().iter().filter(|m| m["role"] == "tool").count(), 1);
    h.app.run("ai.stop", json!({})).unwrap();
}

#[test]
fn reused_tool_request_with_different_arguments_does_not_claim_a_cached_execution() {
    let library = NativeLibrary::new();
    let (mut h, requests, events) = consent(library.open());
    let photo = h.app.session.active().unwrap();
    h.app.run("ai.start", json!({"prompt":"Inspect once"})).unwrap();
    let creation = outgoing(&mut h, &requests, "/v1/rounds");
    phase(&events, &creation, "awaiting_input");
    outgoing(&mut h, &requests, "/input");
    phase(&events, &creation, "analyzing_initial");
    let round = format!("native-round-{}", creation.identity.submission_seq);
    events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":round,"stage":"initial","payload":{"message_id":"owner","model_call_id":1,"tool_calls":[{"id":"read","name":"get_photo_state","arguments":{}}]}})}).unwrap();
    let mut request = json!({"round_id":round,"payload":{"tool_call_id":"read","tool_request_id":"same-request","tool_name":"get_photo_state","args":{},"expected_revision":h.app.session.photo_revision(photo)}});
    events.send(network::Event::Server { kind: "tool.request".into(), data: request.clone() }).unwrap();
    let original = outgoing(&mut h, &requests, "/tool-results");
    request["payload"]["args"] = json!({"different_request":true});
    events.send(network::Event::Server { kind: "tool.request".into(), data: request }).unwrap();
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["busy"], false, "conflicting reuse must fail, not respond with an unrelated cached result");
    outgoing(&mut h, &requests, "/stop");
    let stored = h.app.session.companion_messages_page(photo, 0, 10).unwrap().records;
    let results: Vec<_> = stored.iter().filter(|message| message["role"] == "tool").collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["text"], original.body["result"]["text"]);
}

#[test]
fn bounded_history_display_retains_only_local_timing_and_exact_tool_association() {
    use lightcraft_engine::companion::storage::{DisplayRecord, DisplayScan};
    let library = NativeLibrary::new();
    let mut app = library.open();
    let photo = app.session.active().unwrap();
    let message = json!({"role":"tool","text":"private_result","local_round_id":"scoped","local_message_id":"result","tool_call_id":"read","tool_request_id":"request","tool_name":"get_photo_histogram","stage":"initial","model_call_id":4,"display_outcome":"succeeded","display_elapsed_us":2_100_000,"display_write_state":"published","display_delivery_state":"confirmed","display_plan_id":"plan","additive_large_value":"x".repeat(300_000)});
    app.session.save_companion_tail(photo, None, &[message], &[], &[], &json!({"schema_version":2})).unwrap();
    let header = app.session.companion_photo_header(photo).unwrap().unwrap();
    let native = app.session.companion_owned_history_reader(&header).unwrap().unwrap().message_display(0).unwrap();
    let mut scan = DisplayScan::new(header, DisplayRecord::Message(0));
    let incremental = loop {
        if let Some(record) = app.session.companion_display_step(&mut scan).unwrap() {
            break record;
        }
    };
    for projected in [native, incremental] {
        assert_eq!(projected["display_elapsed_us"], 2_100_000);
        assert_eq!(projected["display_outcome"], "succeeded");
        assert_eq!(projected["tool_call_id"], "read");
        assert_eq!(projected["tool_name"], "get_photo_histogram");
        assert_eq!(projected["stage"], "initial");
        assert_eq!(projected["model_call_id"], 4);
        assert_eq!(projected["display_write_state"], "published");
        assert_eq!(projected["display_delivery_state"], "confirmed");
        assert_eq!(projected["display_plan_id"], "plan");
        assert!(projected.get("additive_large_value").is_none());
    }
}

fn saved_progress(messages: &[Value]) -> crate::headless::Headless {
    let library = NativeLibrary::new();
    let mut app = library.open_with(Box::new(MemStore::new()));
    let photo = app.session.active().unwrap();
    app.session
        .save_companion_tail(
            photo,
            None,
            messages,
            &[],
            &[json!({"local_round_id":"timing","local_terminal":"completed","stages":[]})],
            &json!({"schema_version":2}),
        )
        .unwrap();
    let (mut h, _, _) = consent(app);
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.app.run("ai.status", json!({})).unwrap()["historyLoading"] != false {
        h.step();
        assert!(Instant::now() < deadline);
    }
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiProcess:timing"}), Duration::from_secs(10))["ok"], true);
    for _ in 0..4 {
        h.step();
    }
    h
}

#[test]
fn historical_summary_uses_complete_measured_calls_and_rejects_mismatched_facts() {
    for case in [
        "complete",
        "repeated-category",
        "partial",
        "untimed-guidance",
        "wrong-round",
        "wrong-model",
        "wrong-stage",
        "wrong-name",
        "missing-id",
        "empty-id",
    ] {
        let mut messages = vec![
            json!({"role":"assistant","text":"","local_round_id":"timing","local_message_id":"owner","stage":"initial","model_call_id":1,"tool_calls":[{"id":"state","name":"get_photo_state","arguments":{}},{"id":"histogram","name":"get_photo_histogram","arguments":{}}]}),
            json!({"role":"tool","text":"private_state","local_round_id":"timing","local_message_id":"state-result","tool_call_id":"state","tool_name":"get_photo_state","stage":"initial","model_call_id":1,"display_outcome":"succeeded","display_elapsed_us":1_200_000}),
            json!({"role":"tool","text":"private_histogram","local_round_id":"timing","local_message_id":"histogram-result","tool_call_id":"histogram","tool_name":"get_photo_histogram","stage":"initial","model_call_id":1,"display_outcome":"succeeded","display_elapsed_us":900_000}),
        ];
        match case {
            "repeated-category" => {
                messages[0]["tool_calls"][1]["name"] = json!("get_photo_state");
                messages[2]["tool_name"] = json!("get_photo_state");
            }
            "partial" => {
                messages[2].as_object_mut().unwrap().remove("display_elapsed_us");
            }
            "wrong-model" => messages[2]["model_call_id"] = json!(2),
            "wrong-round" => messages[2]["local_round_id"] = json!("other-round"),
            "wrong-stage" => messages[2]["stage"] = json!("review"),
            "wrong-name" => messages[2]["tool_name"] = json!("sample_photo_color"),
            "untimed-guidance" => {
                messages.push(json!({"role":"assistant","text":"","local_round_id":"timing","local_message_id":"guide-owner","stage":"initial","model_call_id":2,"tool_calls":[{"id":"guidance","name":"skill","arguments":{}}]}));
                messages.push(json!({"role":"tool","text":"private_guidance","local_round_id":"timing","local_message_id":"guide-result","tool_call_id":"guidance","tool_name":"skill","stage":"initial","model_call_id":2}));
            }
            "missing-id" => {
                messages[0]["tool_calls"][1].as_object_mut().unwrap().remove("id");
                messages[2].as_object_mut().unwrap().remove("tool_call_id");
            }
            "empty-id" => {
                messages[0]["tool_calls"][1]["id"] = json!("");
                messages[2]["tool_call_id"] = json!("");
            }
            _ => {}
        }
        // A duplicate result is a repeated fact, not another execution interval.
        if case == "complete" {
            messages.push(messages[2].clone());
        }
        let mut h = saved_progress(&messages);
        let painted = h.painted_text().join(" ");
        if case == "complete" {
            assert!(painted.contains("Viewed 2 info types · 2.1s"), "{case}: {painted}");
        } else if case == "repeated-category" {
            assert!(painted.contains("Viewed 1 info type · 2.1s"), "{case}: {painted}");
        } else if matches!(case, "partial" | "untimed-guidance") {
            assert!(h.painted_text().iter().any(|line| line == "Viewed 2 types of photo information"), "{case}: {painted}");
        } else if case == "wrong-round" {
            assert!(h.painted_text().iter().any(|line| line == "Viewed 1 type of photo information"), "{case}: {painted}");
        } else {
            assert!(h.painted_text().iter().any(|line| line == "Photo information activity") && !painted.contains("2.1s"), "{case}: {painted}");
        }
        assert!(!painted.contains("private_"), "{case}: {painted}");
        if case == "complete" {
            for locale in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
                h.app
                    .run(if locale == crate::i18n::Locale::ZhHans { "app.language.simplifiedChinese" } else { "app.language.english" }, json!({}))
                    .unwrap();
                for width in [1000.0, 1400.0] {
                    h.size.x = width;
                    for _ in 0..4 {
                        h.step();
                    }
                    let expected =
                        if locale == crate::i18n::Locale::ZhHans { "查看了 2 项照片信息 · 2.1s" } else { "Viewed 2 info types · 2.1s" };
                    assert!(h.painted_text().join(" ").contains(expected));
                    let pane = h.app.widgets.iter().find(|(id, _)| id == "ai:messages").unwrap().1;
                    let mut measured_caption = false;
                    for shape in h.view.painted_shapes() {
                        if let egui::epaint::Shape::Text(text) = shape
                            && text.galley.text().contains("2.1s")
                        {
                            let visible: String = text.galley.rows.iter().flat_map(|row| row.glyphs.iter().map(|glyph| glyph.chr)).collect();
                            assert!(visible.contains("2.1s"), "actual painted duration must remain visible: {visible}");
                            assert!(text.visual_bounding_rect().right() <= pane.right() + 1.0, "timed caption exceeds pane");
                            measured_caption = true;
                        }
                    }
                    assert!(measured_caption, "expected actual timed summary shape");
                    super::super::redesign_tests::screenshots(&mut h, &format!("tool-timing-{}-{width}", locale.code()));
                    let perf = h.request("ui.inspect", json!({}), Duration::from_secs(10));
                    assert!(perf["result"]["perf"]["frameMs"].is_number());
                    eprintln!("tool-timing {} {width}: {}", locale.code(), perf["result"]["perf"]);
                }
            }
        }
    }
}

#[test]
fn paged_result_fragments_and_photo_switch_do_not_invent_complete_segment_time() {
    let messages = vec![
        json!({"role":"assistant","text":"","local_round_id":"timing","local_message_id":"paged-owner","stage":"initial","model_call_id":1,"tool_calls":[{"id":"state","name":"get_photo_state","arguments":{}},{"id":"histogram","name":"get_photo_histogram","arguments":{}}]}),
        json!({"role":"tool","text":"private_state","local_round_id":"timing","local_message_id":"paged-state","tool_call_id":"state","tool_name":"get_photo_state","stage":"initial","model_call_id":1,"display_outcome":"succeeded","display_elapsed_us":1_200_000}),
        json!({"role":"tool","text":"private_histogram".repeat(20_000),"local_round_id":"timing","local_message_id":"paged-histogram","tool_call_id":"histogram","tool_name":"get_photo_histogram","stage":"initial","model_call_id":1,"display_outcome":"succeeded","display_elapsed_us":900_000}),
        json!({"role":"assistant","text":"Keep the complete original result.","local_round_id":"timing","local_message_id":"paged-answer"}),
    ];
    let mut h = saved_progress(&messages);
    let original = h.app.session.active().unwrap();
    let other = h.app.session.catalog.photos().find(|p| p.id != original).unwrap().id;
    let text = h.painted_text().join(" ");
    assert!(text.contains("Photo information activity"), "{text}");
    assert!(!text.contains("Viewed 2") && !text.contains("2.1s") && !text.contains("private_"), "{text}");
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryNewer"}), Duration::from_secs(10))["ok"], true);
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.app.run("ai.status", json!({})).unwrap()["historyLoading"] != false {
        h.step();
        assert!(Instant::now() < deadline);
    }
    for _ in 0..4 {
        h.step();
    }
    let text = h.painted_text().join(" ");
    assert!(!text.contains("Viewed 2") && !text.contains("2.1s") && !text.contains("private_"), "{text}");
    h.app.run("library.select", json!({"ids":[other.0],"active":other.0})).unwrap();
    for _ in 0..5 {
        h.step();
    }
    assert!(!h.painted_text().join(" ").contains("2.1s"));
    h.app.run("library.select", json!({"ids":[original.0],"active":original.0})).unwrap();
    for _ in 0..8 {
        h.step();
    }
    assert!(!h.painted_text().join(" ").contains("2.1s"));
    assert_eq!(h.app.session.companion_messages_page(original, 0, 4).unwrap().records, messages);
}

#[test]
fn complete_tool_batches_at_page_boundaries_never_claim_the_visible_subtotal_is_the_segment_total() {
    let mut messages = Vec::new();
    for n in 0..17 {
        messages.push(json!({"role":"assistant","text":"","local_round_id":"timing","local_message_id":format!("batch-{n}"),"stage":"initial","model_call_id":n,"tool_calls":[{"id":format!("read-{n}"),"name":"get_photo_state","arguments":{}}]}));
        messages.push(json!({"role":"tool","text":"private_complete_state","local_round_id":"timing","local_message_id":format!("result-{n}"),"stage":"initial","model_call_id":n,"tool_call_id":format!("read-{n}"),"tool_name":"get_photo_state","display_outcome":"succeeded","display_elapsed_us":100_000}));
    }
    let mut h = saved_progress(&messages);
    for button in [None, Some("button:aiHistoryOlder"), Some("button:aiHistoryNewer")] {
        if let Some(button) = button {
            assert_eq!(h.request("ui.clickWidget", json!({"id":button}), Duration::from_secs(10))["ok"], true);
            let deadline = Instant::now() + Duration::from_secs(10);
            while h.app.run("ai.status", json!({})).unwrap()["historyLoading"] != false {
                h.step();
                assert!(Instant::now() < deadline);
            }
            for _ in 0..4 {
                h.step();
            }
        }
        let text = h.painted_text().join(" ");
        assert!(text.contains("Photo information activity"), "unknown continuous segment range stays neutral: {text}");
        assert!(
            !text.contains("1.6s") && !text.contains("0.1s") && !text.contains("private_complete_state"),
            "a complete visible call batch is not a complete segment: {text}"
        );
    }
    let photo = h.app.session.active().unwrap();
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 34).unwrap().records, messages);
}
