use super::*;
fn history(messages: Vec<Value>) -> Result<Vec<Value>, String> {
    let mut app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), crate::Services::default());
    let photo = app.session.active().unwrap();
    let mut state = State::default();
    state.conversations.insert(photo, Conversation { messages, ..Default::default() });
    state.history(&mut app, photo)
}
fn call(id: &str) -> Value {
    json!({"id":id,"name":"get_photo_state","arguments":{}})
}
#[test]
fn unmatched_duplicate_and_interleaved_tool_history_refuses_without_trimming() {
    for messages in [
        vec![json!({"role":"assistant","text":"","tool_calls":[call("a")]} )],
        vec![json!({"role":"tool","tool_call_id":"orphan","text":"success"})],
        vec![json!({"role":"assistant","text":"","tool_calls":[call("a"),call("a")]}), json!({"role":"tool","tool_call_id":"a","text":"done"})],
        vec![
            json!({"role":"assistant","text":"","tool_calls":[call("a")]}),
            json!({"role":"user","text":"later"}),
            json!({"role":"tool","tool_call_id":"a","text":"done"}),
        ],
    ] {
        assert!(history(messages).is_err());
    }
}
#[test]
fn model_projection_omits_unfinished_assistant_and_only_serializes_protocol_fields() {
    let projected = history(vec![
        json!({"role":"assistant","text":"partial","complete_state":"streaming"}),
        json!({"role":"user","text":"all text","preview_integrity":{"secret":true},"new_local_field":true}),
    ])
    .unwrap();
    assert_eq!(projected, vec![json!({"role":"user","text":"all text"})]);
}
#[test]
fn current_request_reserves_service_message_and_image_capacity() {
    assert!(history((0..1024).map(|n| json!({"role":"user","text":format!("{n}")})).collect()).is_err());
}
#[test]
fn stopped_multi_tool_batch_reopens_and_next_real_input_has_all_true_results() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let make_app = || {
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "memory".into(), on_disk: false },
                true,
            )
            .unwrap();
        LightcraftApp::new(session, crate::Services::default())
    };
    let mut app = make_app();
    let photo = app.session.active().unwrap();
    let (connection, requests, _) = network::test_connection();
    let mut state = State { connection: Some(connection), ..Default::default() };
    state.start(&mut app).unwrap();
    requests.try_recv().unwrap();
    let identity = state.jobs[&photo].identity.clone();
    state.server(&mut app,&egui::Context::default(),"round.state",&json!({"round_id":"round","payload":{"submission_seq":identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":identity.scope.library_generation},"state":"awaiting_input"}})).unwrap();
    let render = state.rx.recv_timeout(Duration::from_secs(30)).unwrap();
    state.prepared(&mut app, &egui::Context::default(), render).unwrap();
    requests.try_recv().unwrap();
    state
        .server(
            &mut app,
            &egui::Context::default(),
            "message.completed",
            &json!({"round_id":"round","payload":{"message_id":"m","tool_calls":[call("z-done"),call("b-not-run"),call("a-not-run")]}}),
        )
        .unwrap();
    let revision = app.session.photo_revision(photo);
    state.server(&mut app,&egui::Context::default(),"tool.request",&json!({"round_id":"round","payload":{"tool_call_id":"z-done","tool_request_id":"req","expected_revision":revision,"tool_name":"get_photo_state"}})).unwrap();
    requests.try_recv().unwrap();
    state
        .server(
            &mut app,
            &egui::Context::default(),
            "text.delta",
            &json!({"round_id":"round","payload":{"message_id":"unfinished","delta":"MUST NOT PROJECT"}}),
        )
        .unwrap();
    state.stop(photo).unwrap();
    state.save(&mut app, photo).unwrap();
    app.session.close_library().unwrap();
    drop(state);
    let mut app = make_app();
    let (connection, requests, events) = network::test_connection();
    app.ai.next_connection = Some(connection);
    let mut h = crate::headless::Headless::new(app, [1100., 750.], 1.);
    h.app.run("ai.open", json!({})).unwrap();
    h.step();
    h.step();
    h.app.run("ai.start", json!({"prompt":"next explicit request"})).unwrap();
    let creation = native_history_tests::outgoing(&mut h, &requests, "/v1/rounds");
    let identity = creation.identity;
    events.send(network::Event::Server { kind:"round.state".into(), data:json!({"round_id":"next","payload":{"submission_seq":identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":identity.scope.library_generation},"state":"awaiting_input"}}) }).unwrap();
    let request = native_history_tests::outgoing(&mut h, &requests, "/input");
    let history = request.body["history"].as_array().unwrap();
    let tools: Vec<_> = history.iter().filter(|m| m["role"] == "tool").collect();
    assert_eq!(tools.len(), 3);
    assert_eq!(tools[0]["tool_call_id"], "z-done");
    assert_eq!(tools[1]["tool_call_id"], "b-not-run");
    assert_eq!(tools[2]["tool_call_id"], "a-not-run");
    for message in &tools[1..] {
        let result: Value = serde_json::from_str(message["text"].as_str().unwrap()).unwrap();
        assert_eq!(result["outcome"], "cancelled");
        assert_eq!(result["executed"], false);
    }
    assert!(!history.iter().any(|m| m["text"].as_str().unwrap_or("").contains("MUST NOT PROJECT")));
    if let Ok(path) = std::env::var("UX11_HISTORY_FIXTURE") {
        std::fs::write(path, serde_json::to_vec_pretty(&request.body).unwrap()).unwrap();
    }

    assert_eq!(request.body["history"].as_array().unwrap().iter().filter(|m| m["role"] == "tool").count(), 3);
}
#[test]
fn current_actual_state_budget_refuses_before_create_and_keeps_every_local_byte() {
    let mut app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), crate::Services::default());
    let photo = app.session.active().unwrap();
    let text = "x".repeat(256 * 1024 - 16);
    let (connection, requests, _) = network::test_connection();
    let mut state = State { prompt: "user request".into(), connection: Some(connection), ..Default::default() };
    state.conversations.insert(photo, Conversation { messages: vec![json!({"role":"user","text":text})], ..Default::default() });
    assert!(state.start(&mut app).is_err());
    assert!(requests.try_recv().is_err());
    assert!(state.jobs.is_empty());
    assert!(app.session.companion_task_photo().is_none());
    assert_eq!(state.conversations[&photo].messages[0]["text"], text);
}
#[test]
fn image_and_argument_limits_are_checked_before_attachment_read() {
    let mut projection = projection::HistoryProjection::new(PhotoId(1));
    let mut reads = 0;
    let bad =
        json!({"role":"user","text":"all","images":(0..256).map(|_|json!({"mime_type":"image/jpeg","attachment":"missing"})).collect::<Vec<_>>()});
    assert!(
        projection
            .push(&bad, |_| {
                reads += 1;
                Err("read".into())
            })
            .is_err()
    );
    assert_eq!(reads, 0);
    let bad = json!({"role":"assistant","text":"","tool_calls":[{"id":"a","name":"n","arguments":{"text":"x".repeat(64*1024)}}]});
    assert!(history(vec![bad]).is_err());
    let bad = json!({"role":"user","text":"","images":[{"mime_type":"image/png","data":Base64::encode_string(b"not PNG")}]});
    assert!(history(vec![bad]).is_err());
    let mut valid = projection::HistoryProjection::new(PhotoId(1));
    for role in [
        json!({"role":"user","text":"full text"}),
        json!({"role":"assistant","text":"plan","tool_calls":[call("a")]}),
        json!({"role":"tool","text":"cancelled","tool_call_id":"a"}),
        json!({"role":"user","text":"Actual execution feedback: failed"}),
    ] {
        valid.push(&role, |_| Err("unexpected attachment".into())).unwrap();
    }
    assert_eq!(valid.finish().unwrap().len(), 4);
}
#[test]
fn invalid_history_rejection_is_visible_and_localized_from_real_start_widget() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let mut app = LightcraftApp::new(
            lightcraft_engine::Session::with_demo(),
            crate::Services {
                write: Some(Box::new(lightcraft_engine::export::write_file)),
                png: Some(Box::new(|image| {
                    lightcraft_engine::export::encode_image(
                        image,
                        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                    )
                    .unwrap()
                })),
                ..Default::default()
            },
        );
        let photo = app.session.active().unwrap();
        app.ui.language = language;
        app.ui.settings.gpu = false;
        let (connection, requests, _) = network::test_connection();
        app.ai = State { generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
        app.ai.conversations.insert(
            photo,
            Conversation {
                scope: Some(PhotoScope { photo, library_generation: app.session.library_generation() }),
                messages: vec![json!({"role":"tool","text":"readable legacy orphan","tool_call_id":"missing-owner"})],
                ..Default::default()
            },
        );
        let mut h = crate::headless::Headless::new(app, [1600.0, 1000.0], 1.0);
        let timeout = Duration::from_secs(10);
        h.app.run("ai.open", json!({})).unwrap();
        h.step();
        h.step();
        h.app.ai.prompt = "Review this photo.".into();
        super::drafts::capture(&mut h.app);
        h.step();
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiStart"}), timeout)["ok"], true);
        h.step();
        assert!(requests.try_recv().is_err());
        assert!(h.app.ai.jobs.is_empty());
        assert!(h.app.session.companion_task_photo().is_none());
        assert_eq!(h.app.ai.conversations[&photo].messages.len(), 1);
        assert!(h.painted_text().iter().any(|text| {
            text.contains(
                language.tr(
                    "The AI history is incomplete or invalid. Local history is retained. Repair attachments or delete the conversation to retry.",
                ),
            )
        }));
        if let Ok(dir) = std::env::var("UX11_SCREENSHOT_DIR") {
            let result = h.request("ui.screenshot", json!({"path":format!("{dir}/invalid-history-{}.png",language.code()),"headless":true}), timeout);
            assert_eq!(result["ok"], true, "{result}");
        }
    }
}
#[test]
fn base64_request_budget_stops_before_loading_all_historical_images() {
    // Opaque PNG exports use RGB; keep the fixture above 100 KiB without relying on an alpha channel.
    let image = lightcraft_raster::Rgba8::from_fn(192, 192, |x, y| {
        let mut n = (x + y * 192) as u64 + 0x9e3779b97f4a7c15;
        n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
        n ^= n >> 31;
        [(n & 255) as u8, ((n >> 8) & 255) as u8, ((n >> 16) & 255) as u8, 255]
    });
    let data = lightcraft_engine::export::encode_image(
        &image,
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    assert!(data.len() > 100 * 1024 && data.len() <= 128 * 1024, "fixture length {}", data.len());
    let pictures: Vec<_> = (0..255).map(|_| json!({"mime_type":"image/png","attachment":format!("companion/preview-1-{:032x}.bin",lightcraft_preview::hash_bytes(&data).0),"length":data.len()})).collect();
    let mut builder = projection::HistoryProjection::new(PhotoId(1));
    let mut reads = 0usize;
    let result = builder.push(&json!({"role":"user","text":"retain every historical image locally","images":pictures}), |_| {
        reads += 1;
        Ok(data.clone())
    });
    assert!(result.unwrap_err().contains("service limit"));
    assert!(reads > 0 && reads < 255, "preflight must stop before full hydration, read {reads}");
}
#[test]
fn legacy_attachment_owner_and_content_identity_must_match_current_photo_history() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(LibraryStores { catalog: Box::new(catalog), files: Box::new(files.clone()), dir: "memory".into(), on_disk: false }, true)
        .unwrap();
    let mut app = LightcraftApp::new(session, crate::Services::default());
    let photo = app.session.active().unwrap();
    let other = app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    let options = lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() };
    let bytes = lightcraft_engine::export::encode_image(&lightcraft_raster::Rgba8::filled(16, 16, [12, 30, 50, 255]), &options).unwrap();
    let library = app.session.library.as_mut().unwrap();
    let correct = library.companion_attachment(photo, &bytes).unwrap();
    let wrong = library.companion_attachment(other, &bytes).unwrap();
    let mut state = State::default();
    state.conversations.insert(
        photo,
        Conversation {
            messages: vec![json!({"role":"user","text":"readable","images":[{"mime_type":"image/png","attachment":wrong}]})],
            ..Default::default()
        },
    );
    assert!(state.history(&mut app, photo).is_err(), "cross-photo reference must not be uploaded");
    state.conversations.get_mut(&photo).unwrap().messages[0]["images"][0]["attachment"] = json!(correct);
    assert!(state.history(&mut app, photo).is_ok());
    let replacement = lightcraft_engine::export::encode_image(&lightcraft_raster::Rgba8::filled(16, 16, [90, 100, 120, 255]), &options).unwrap();
    files.set(&correct, replacement);
    assert!(state.history(&mut app, photo).is_err(), "validly encoded replacement must not impersonate the recorded image");
    assert_eq!(state.conversations[&photo].messages[0]["text"], "readable");
}
#[test]
fn valid_legacy_empty_optional_message_fields_follow_go_wire_semantics() {
    let result = history(vec![
        json!({"role":"assistant","tool_call_id":"","tool_calls":[call("legacy")]}),
        json!({"role":"tool","tool_call_id":"legacy","text":null}),
    ])
    .unwrap();
    assert_eq!(result[0]["text"], "");
    assert!(result[0].get("tool_call_id").is_none());
    assert_eq!(result[1]["text"], "");
    assert_eq!(result.len(), 2);
}

#[test]
fn start_command_refuses_ambiguous_legacy_image_and_allows_next_ordinary_edit() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let stores = || LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "memory".into(), on_disk: false };
    let mut session = lightcraft_engine::Session::new();
    session.open_library_in(stores(), true).unwrap();
    let photo = session.active().unwrap();
    session.close_library().unwrap();
    let bytes = lightcraft_engine::export::encode_image(
        &lightcraft_raster::Rgba8::filled(16, 16, [12, 30, 50, 255]),
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    files.set("companion/state.json", serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"keep readable history","images":[{"mime_type":"image/png","data":Base64::encode_string(&bytes),"attachment":123}]}],"rounds":[]}}})).unwrap());
    let mut session = lightcraft_engine::Session::new();
    session.open_library_in(stores(), false).unwrap();
    let mut app = LightcraftApp::new(session, crate::Services::default());
    let (connection, requests, _events) = network::test_connection();
    app.ai.generation = app.session.library_generation();
    app.ai.connection = Some(connection);
    let mut h = crate::headless::Headless::new(app, [1600.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    h.step();
    h.step();
    let started = h.app.run("ai.start", json!({"prompt":"current request"}));
    let error = match started {
        Err(error) => error,
        Ok(_) => native_history_tests::wait_idle(&mut h, &requests)["text"].as_str().unwrap().to_owned(),
    };
    assert!(error.contains("history is incomplete or invalid"), "{error}");
    assert!(requests.try_recv().is_err(), "no creation request should reach the transport adapter");
    h.app.run("develop.set", json!({"control":"light.exposure","value":0.7})).unwrap();
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.7);
}

#[test]
fn start_refuses_truncated_historical_jpeg_and_keeps_valid_jpeg_usable() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    let valid = lightcraft_engine::export::encode_image(
        &lightcraft_raster::Rgba8::from_fn(32, 32, |x, y| [(x * 7) as u8, (y * 7) as u8, 80, 255]),
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Jpeg, ..Default::default() },
    )
    .unwrap();
    let truncated = valid[..valid.len() - 2].to_vec();
    if let Ok(dir) = std::env::var("UX11_STRICT_JPEG_FIXTURES") {
        std::fs::write(format!("{dir}/caller-valid.jpg"), &valid).unwrap();
        std::fs::write(format!("{dir}/caller-no-eoi.jpg"), &truncated).unwrap();
    }
    for (bytes, refused) in [(&truncated, true), (&valid, false)] {
        let (catalog, files) = (MemStore::new(), MemStore::new());
        let stores = || LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "memory".into(), on_disk: false };
        let mut session = lightcraft_engine::Session::new();
        session.open_library_in(stores(), true).unwrap();
        let photo = session.active().unwrap();
        session.close_library().unwrap();
        let saved = json!({"schema_version":1,"messages":[{"role":"user","text":"keep JPEG history","images":[{"mime_type":"image/jpeg","data":Base64::encode_string(bytes)}]}],"rounds":[]});
        files.set(
            "companion/state.json",
            serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():saved}})).unwrap(),
        );
        let mut session = lightcraft_engine::Session::new();
        session.open_library_in(stores(), false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while session.companion_migration_pending() && Instant::now() < deadline {
            let _ = session.persist();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!session.companion_migration_pending(), "migration must finish before testing image-specific admission");
        let mut app = LightcraftApp::new(session, crate::Services::default());
        let (connection, requests, _events) = network::test_connection();
        app.ai.generation = app.session.library_generation();
        app.ai.connection = Some(connection);
        let mut h = crate::headless::Headless::new(app, [1600.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        h.step();
        h.step();
        let result = h.app.run("ai.start", json!({"prompt":"current request"}));
        if refused {
            let error = match result {
                Err(error) => error,
                Ok(_) => native_history_tests::wait_idle(&mut h, &requests)["text"].as_str().unwrap().to_owned(),
            };
            assert!(error.contains("history is incomplete or invalid"), "{error}");
            assert!(requests.try_recv().is_err(), "corrupt JPEG must not create/upload a round");
            assert_eq!(
                h.app.session.companion_messages_page(photo, 0, 1).unwrap().records,
                saved["messages"].as_array().unwrap().clone(),
                "complete original messages remain readable"
            );
            assert_eq!(
                files.get("companion/state.v1.backup.json"),
                Some(serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():saved}})).unwrap()),
                "the exact original archive remains recoverable"
            );
            h.app.run("develop.set", json!({"control":"light.exposure","value":0.7})).unwrap();
            assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.7);
        } else {
            assert_eq!(result.unwrap()["started"], true);
            assert_eq!(native_history_tests::outgoing(&mut h, &requests, "/v1/rounds").path, "/v1/rounds");
        }
    }
}

#[test]
fn start_refuses_service_unsupported_lossless_jpeg_and_keeps_normal_jpeg_usable() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    let valid = lightcraft_engine::export::encode_image(
        &lightcraft_raster::Rgba8::from_fn(32, 32, |x, y| [(x * 7) as u8, (y * 7) as u8, 80, 255]),
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Jpeg, ..Default::default() },
    )
    .unwrap();
    // Procedural 1x1, 8-bit Huffman lossless SOF3: complete scan and EOI.
    // Actual service Go image.Decode rejects this frame type (review fixture).
    let lossless = vec![
        0xff, 0xd8, 0xff, 0xc4, 0x00, 0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xc3, 0x00, 0x0b, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x01, 0x00,
        0x00, 0x7f, 0xff, 0xd9,
    ];
    for (bytes, refused) in [(&lossless, true), (&valid, false)] {
        let (catalog, files) = (MemStore::new(), MemStore::new());
        let stores = || LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "memory".into(), on_disk: false };
        let mut session = lightcraft_engine::Session::new();
        session.open_library_in(stores(), true).unwrap();
        let photo = session.active().unwrap();
        session.close_library().unwrap();
        let saved = json!({"schema_version":1,"messages":[{"role":"user","text":"keep JPEG history","images":[{"mime_type":"image/jpeg","data":Base64::encode_string(bytes)}]}],"rounds":[]});
        files.set(
            "companion/state.json",
            serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():saved}})).unwrap(),
        );
        let mut session = lightcraft_engine::Session::new();
        session.open_library_in(stores(), false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while session.companion_migration_pending() && Instant::now() < deadline {
            let _ = session.persist();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!session.companion_migration_pending(), "migration must finish before testing image-specific admission");
        let mut app = LightcraftApp::new(session, crate::Services::default());
        let (connection, requests, _events) = network::test_connection();
        app.ai.generation = app.session.library_generation();
        app.ai.connection = Some(connection);
        let mut h = crate::headless::Headless::new(app, [1600.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        h.step();
        h.step();
        let result = h.app.run("ai.start", json!({"prompt":"current request"}));
        if refused {
            let error = match result {
                Err(error) => error,
                Ok(_) => native_history_tests::wait_idle(&mut h, &requests)["text"].as_str().unwrap().to_owned(),
            };
            assert!(error.contains("history is incomplete or invalid"), "{error}");
            assert!(requests.try_recv().is_err(), "corrupt JPEG must not create/upload a round");
            assert_eq!(
                h.app.session.companion_messages_page(photo, 0, 1).unwrap().records,
                saved["messages"].as_array().unwrap().clone(),
                "complete original messages remain readable"
            );
            assert_eq!(
                files.get("companion/state.v1.backup.json"),
                Some(serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():saved}})).unwrap()),
                "the exact original archive remains recoverable"
            );
            h.app.run("develop.set", json!({"control":"light.exposure","value":0.7})).unwrap();
            assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.7);
        } else {
            assert_eq!(result.unwrap()["started"], true);
            assert_eq!(native_history_tests::outgoing(&mut h, &requests, "/v1/rounds").path, "/v1/rounds");
        }
    }
}
