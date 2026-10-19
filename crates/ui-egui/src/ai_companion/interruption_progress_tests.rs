//! Real event dispatch and painted UI keep interruption facts truthful.
use super::*;

#[test]
fn attempted_read_failure_expands_inline_without_inventing_success_or_fallback() {
    let (mut h, requests, events, _) = reading_tests::progress_live();
    let photo = h.app.session.active().unwrap();
    events.send(network::Event::Server {kind:"message.completed".into(), data:json!({"round_id":"progress-round","stage":"initial","payload":{"message_id":"bad-sample","model_call_id":1,"tool_calls":[{"id":"sample","name":"sample_photo_color","arguments":{}}]}})}).unwrap();
    events.send(network::Event::Server {kind:"tool.request".into(),data:json!({"round_id":"progress-round","payload":{"tool_call_id":"sample","tool_request_id":"bad-request","tool_name":"sample_photo_color","args":{},"expected_revision":h.app.session.photo_revision(photo)}})}).unwrap();
    for _ in 0..8 {
        h.step();
    }
    let painted = h.painted_text().join(" ");
    assert!(painted.contains("Color sampling failed"), "{painted}");
    assert!(painted.contains("Photo information reading incomplete"), "{painted}");
    assert!(!painted.contains("Preparing to") && !painted.contains("Viewed 0") && !painted.contains("fallback"), "{painted}");
    let records = h.app.session.companion_messages_page(photo, 0, 20).unwrap().records;
    let result = records.iter().find(|message| message["tool_call_id"] == "sample").unwrap();
    assert_eq!(result["display_outcome"], "failed");
    assert!(result["display_elapsed_us"].is_u64());
    let feedback: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
    assert_eq!(feedback["executed"], true);
    native_history_tests::outgoing(&mut h, &requests, "/stop");
    h.app.run("ai.start", json!({"prompt":"Continue from this photo"})).unwrap();
    let create = native_history_tests::outgoing(&mut h, &requests, "/v1/rounds");
    events.send(network::Event::Server {kind:"round.state".into(),data:json!({"round_id":"next-progress-round","payload":{"submission_seq":create.identity.submission_seq,"photo_ref":create.body["photo_ref"],"state":"awaiting_input"}})}).unwrap();
    let next = native_history_tests::outgoing(&mut h, &requests, "/input");
    let tools: Vec<_> = next.body["history"].as_array().unwrap().iter().filter(|message| message["role"] == "tool").collect();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["tool_call_id"], "sample");
    assert_eq!(tools[0]["text"], result["text"]);
    assert!(!next.body.to_string().contains("display_outcome"));
    h.app.run("ai.stop", json!({})).unwrap();
}

#[test]
fn stopped_timed_out_and_disconnected_reads_keep_successes_without_failed_unexecuted_claims() {
    for cause in ["stop", "timeout", "disconnect"] {
        for partial in [false, true] {
            let (mut h, requests, events, round) = reading_tests::progress_live();
            let photo = h.app.session.active().unwrap();
            if partial {
                events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"progress-round","stage":"initial","payload":{"message_id":"state","model_call_id":1,"tool_calls":[{"id":"state","name":"get_photo_state","arguments":{}}]}})}).unwrap();
                events.send(network::Event::Server {kind:"tool.request".into(),data:json!({"round_id":"progress-round","payload":{"tool_call_id":"state","tool_request_id":"read-state","tool_name":"get_photo_state","args":{},"expected_revision":h.app.session.photo_revision(photo)}})}).unwrap();
                native_history_tests::outgoing(&mut h, &requests, "/tool-results");
            }
            events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"progress-round","stage":"initial","payload":{"message_id":"pending","model_call_id":2,"tool_calls":[{"id":"hist","name":"get_photo_histogram","arguments":{}}]}})}).unwrap();
            for _ in 0..3 {
                h.step();
            }
            match cause {
                "stop" => {
                    h.app.run("ai.stop", json!({})).unwrap();
                }
                "timeout" => events
                    .send(network::Event::Server {
                        kind: "round.ended".into(),
                        data: json!({"round_id":"progress-round","payload":{"terminal_state":"timed_out","reason_code":"round_timed_out"}}),
                    })
                    .unwrap(),
                _ => events.send(network::Event::Closed("owned transport closed".into())).unwrap(),
            }
            for _ in 0..5 {
                h.step();
            }
            let before_open = h.painted_text().join(" ");
            assert!(
                before_open.contains(match cause {
                    "stop" => "Stopped generating",
                    "timeout" => "timed out",
                    _ => "connection was interrupted",
                }),
                "{before_open}"
            );
            assert!(!before_open.contains("Preparing to") && !before_open.contains("continuing analysis"));
            assert!(
                before_open.contains("The photo remains at this round's starting settings."),
                "actual photo state survives a folded terminal process: {before_open}"
            );
            assert_eq!(h.request("ui.clickWidget", json!({"id":format!("button:aiProcess:{round}")}), Duration::from_secs(10))["ok"], true);
            for (language, expected_reason) in [
                (
                    crate::i18n::Locale::En,
                    match cause {
                        "stop" => "Stopped",
                        "timeout" => "Timed out",
                        _ => "Connection interrupted",
                    },
                ),
                (
                    crate::i18n::Locale::ZhHans,
                    match cause {
                        "stop" => "已停止",
                        "timeout" => "处理超时",
                        _ => "连接中断",
                    },
                ),
            ] {
                h.app.ui.language = language;
                for _ in 0..4 {
                    h.step();
                }
                let painted = h.painted_text().join(" ");
                assert!(painted.contains(expected_reason), "{cause} {partial}: {painted}");
                let expected = match (language, partial) {
                    (crate::i18n::Locale::En, true) => "Viewed 1 info type",
                    (crate::i18n::Locale::En, false) => "Photo information reading incomplete",
                    (_, true) => "查看了 1 项照片信息",
                    (_, false) => "照片信息读取未完成",
                };
                assert!(painted.contains(expected), "{painted}");
                for forbidden in ["Reading histogram failed", "读取直方图失败", "Preparing to", "正在继续分析", "Viewed 0"] {
                    assert!(!painted.contains(forbidden), "{painted}");
                }
            }
            let records = h.app.session.companion_messages_page(photo, 0, 30).unwrap().records;
            let pending = records.iter().find(|message| message["tool_call_id"] == "hist").unwrap();
            let result: Value = serde_json::from_str(pending["text"].as_str().unwrap()).unwrap();
            assert_eq!(result["executed"], false);
            assert_eq!(pending["display_outcome"], "not_executed");
        }
    }
}

#[test]
fn explicit_knowledge_failure_expands_without_stopping_round_or_claiming_fallback() {
    for (name, code) in [("skill", "knowledge_unavailable"), ("read_skill_resource", "knowledge_limit")] {
        let (mut h, _requests, events, round) = reading_tests::progress_live();
        let text = json!({"error":code,"message":"Requested knowledge could not be read; continue only with available information."}).to_string();
        events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"progress-round","stage":"initial","payload":{"message_id":"knowledge","model_call_id":1,"tool_calls":[{"id":"guide","name":name,"arguments":{}}]}})}).unwrap();
        events.send(network::Event::Server {kind:"tool.result".into(),data:json!({"round_id":"progress-round","stage":"initial","payload":{"tool_call_id":"guide","tool_name":name,"text":text,"model_call_id":1}})}).unwrap();
        for _ in 0..5 {
            h.step();
        }
        for (language, expected) in [
            (crate::i18n::Locale::En, if name == "skill" { "Photography guidance unavailable" } else { "Reference material unavailable" }),
            (crate::i18n::Locale::ZhHans, if name == "skill" { "摄影指导读取失败" } else { "参考资料读取失败" }),
        ] {
            h.app.ui.language = language;
            for _ in 0..4 {
                h.step();
            }
            let paint = h.painted_text().join(" ");
            assert!(paint.contains(expected), "{paint}");
            assert!(
                !paint.contains("response recorded") && !paint.contains("返回内容已记录") && !paint.contains("fallback") && !paint.contains("已改用"),
                "{paint}"
            );
            assert_eq!(h.app.run("ai.status", json!({})).unwrap()["busy"], true);
        }
        assert_eq!(h.request("ui.clickWidget", json!({"id":format!("button:aiProcess:{round}")}), Duration::from_secs(10))["ok"], true);
        for _ in 0..5 {
            h.step();
        }
        assert!(!h.app.widgets.iter().any(|(id, _)| id.starts_with("button:aiToolSegment:")), "a failed history remains manually foldable");
        let photo = h.app.session.active().unwrap();
        let messages = h.app.session.companion_messages_page(photo, 0, 20).unwrap().records;
        assert_eq!(messages.iter().find(|message| message["tool_call_id"] == "guide").unwrap()["text"], text);
        events
            .send(network::Event::Server {
                kind: "text.delta".into(),
                data: json!({"round_id":"progress-round","payload":{"message_id":"answer","delta":"Use the available photo information."}}),
            })
            .unwrap();
        events
            .send(network::Event::Server {
                kind: "message.completed".into(),
                data: json!({"round_id":"progress-round","stage":"initial","payload":{"message_id":"answer","model_call_id":2,"tool_calls":[]}}),
            })
            .unwrap();
        events
            .send(network::Event::Server {
                kind: "round.ended".into(),
                data: json!({"round_id":"progress-round","payload":{"terminal_state":"completed"}}),
            })
            .unwrap();
        for _ in 0..5 {
            h.step();
        }
        assert!(
            !h.app.widgets.iter().any(|(id, _)| id.starts_with("button:aiToolSegment:")),
            "already observed failure must not override manual folding when the round ends"
        );
        assert!(h.painted_text().join(" ").contains("Use the available photo information."));
    }
}

#[test]
fn partial_read_failure_retains_information_and_reason_outside_manually_folded_process() {
    let (mut h, requests, events, round) = reading_tests::progress_live();
    let photo = h.app.session.active().unwrap();
    events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"progress-round","stage":"initial","payload":{"message_id":"state","model_call_id":1,"tool_calls":[{"id":"state","name":"get_photo_state","arguments":{}}]}})}).unwrap();
    events.send(network::Event::Server {kind:"tool.request".into(),data:json!({"round_id":"progress-round","payload":{"tool_call_id":"state","tool_request_id":"read-state","tool_name":"get_photo_state","args":{},"expected_revision":h.app.session.photo_revision(photo)}})}).unwrap();
    native_history_tests::outgoing(&mut h, &requests, "/tool-results");
    events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"progress-round","stage":"initial","payload":{"message_id":"bad-sample","model_call_id":2,"tool_calls":[{"id":"sample","name":"sample_photo_color","arguments":{}}]}})}).unwrap();
    events.send(network::Event::Server {kind:"tool.request".into(),data:json!({"round_id":"progress-round","payload":{"tool_call_id":"sample","tool_request_id":"bad-request","tool_name":"sample_photo_color","args":{},"expected_revision":h.app.session.photo_revision(photo)}})}).unwrap();
    for _ in 0..6 {
        h.step();
    }
    for (language, count, reason) in [
        (crate::i18n::Locale::En, "Viewed 1 info type", "Color sampling failed"),
        (crate::i18n::Locale::ZhHans, "查看了 1 项照片信息", "颜色取样失败"),
    ] {
        h.app.ui.language = language;
        for width in [1000.0, 1400.0] {
            h.size.x = width;
            for _ in 0..4 {
                h.step();
            }
            let paint = h.painted_text().join(" ");
            assert!(paint.contains(count) && paint.contains(reason), "{paint}");
            assert!(!paint.contains("reading incomplete") && !paint.contains("读取未完成") && !paint.contains("继续分析"), "{paint}");
            redesign_tests::screenshots(&mut h, &format!("interruption-partial-{}-{width}", language.code()));
            let inspect = h.request("ui.inspect", json!({}), Duration::from_secs(10));
            assert!(inspect["result"]["perf"]["frameMs"].is_number());
            eprintln!("interruption partial {} {width}: {}", language.code(), inspect["result"]["perf"]);
        }
    }
    assert_eq!(h.request("ui.clickWidget", json!({"id":format!("button:aiProcess:{round}")}), Duration::from_secs(10))["ok"], true);
    for _ in 0..5 {
        h.step();
    }
    assert!(!h.app.widgets.iter().any(|(id, _)| id.starts_with("button:aiToolSegment:")));
    let paint = h.painted_text().join(" ");
    assert!(paint.contains("颜色取样失败"), "factual failure remains visible outside the folded process: {paint}");
    assert!(paint.contains("照片保持本轮起点") || paint.contains("照片保持在本轮开始时"), "photo state remains visible: {paint}");
}

#[test]
fn rejected_read_guard_is_not_an_attempted_tool_failure() {
    for (name, stale) in [("get_photo_histogram", true), ("unavailable_photo_tool", false)] {
        let (mut h, _requests, events, round) = reading_tests::progress_live();
        let photo = h.app.session.active().unwrap();
        events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"progress-round","stage":"initial","payload":{"message_id":"stale","model_call_id":1,"tool_calls":[{"id":"hist","name":name,"arguments":{}}]}})}).unwrap();
        events.send(network::Event::Server {kind:"tool.request".into(),data:json!({"round_id":"progress-round","payload":{"tool_call_id":"hist","tool_request_id":"stale-request","tool_name":name,"args":{},"expected_revision":h.app.session.photo_revision(photo).saturating_add(u64::from(stale))}})}).unwrap();
        for _ in 0..6 {
            h.step();
        }
        assert!(
            !h.app.widgets.iter().any(|(id, _)| id.starts_with("button:aiToolSegment:")),
            "pre-execution rejection does not claim an actual tool failure"
        );
        assert_eq!(h.request("ui.clickWidget", json!({"id":format!("button:aiProcess:{round}")}), Duration::from_secs(10))["ok"], true);
        for _ in 0..4 {
            h.step();
        }
        let paint = h.painted_text().join(" ");
        assert!(paint.contains(if stale { "Photo information reading incomplete" } else { "Working on the photo" }), "{paint}");
        assert!(!paint.contains("Reading histogram failed") && !paint.contains("Preparing to") && !paint.contains("Viewed 1"), "{paint}");
        let messages = h.app.session.companion_messages_page(photo, 0, 20).unwrap().records;
        let result = messages.iter().find(|message| message["tool_call_id"] == "hist").unwrap();
        assert_eq!(result["display_outcome"], "not_executed");
        assert!(result.get("display_elapsed_us").is_none());
        let text: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["executed"], false);
    }
}
