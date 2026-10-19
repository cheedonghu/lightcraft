use super::*;
use crate::{Services, headless::Headless, i18n::Locale};
use lightcraft_catalog::{MemStore, Store};
use lightcraft_engine::{Session, companion::EditorState, library::LibraryStores};

fn open(catalog: &MemStore, files: &MemStore) -> Session {
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "damaged-ui".into(), on_disk: false },
            true,
        )
        .unwrap();
    session
}

#[test]
fn corrupt_owned_message_shows_a_localized_placeholder_keeps_neighbors_and_blocks_submission() {
    for language in [Locale::En, Locale::ZhHans] {
        for background in [false, true] {
            let (catalog, mut files) = (MemStore::new(), MemStore::new());
            files.background = background;
            let mut session = open(&catalog, &files);
            let photo = session.active().unwrap();
            let mut messages = vec![
                json!({"role":"user","text":"INTACT BEFORE"}),
                json!({"role":"assistant","text":"DAMAGED MIDDLE"}),
                json!({"role":"user","text":"INTACT AFTER"}),
            ];
            if background {
                messages.extend((3..40).map(|_| json!({"role":"assistant","text":"LATER VERIFIED"})));
            }
            session.save_companion_conversation(photo, json!({"schema_version":2,"messages":messages,"rounds":[],"round_records":[]})).unwrap();
            session.set_companion_editor(photo, EditorState { draft: "Keep the unsent draft".into(), ..Default::default() }).unwrap();
            let header = session.companion_photo_header(photo).unwrap().unwrap();
            session.close_library().unwrap();
            drop(session);
            let mut index: Value = serde_json::from_slice(&files.get(&header.message_root.as_ref().unwrap().key).unwrap()).unwrap();
            while index["start"].as_u64().unwrap() > 1 {
                index = serde_json::from_slice(&files.get(index["previous"]["key"].as_str().unwrap()).unwrap()).unwrap();
            }
            let entry = &index["entries"][1];
            let key = entry["block"].as_str().unwrap();
            let offset = entry["offset"].as_u64().unwrap() as usize;
            let length = entry["length"].as_u64().unwrap() as usize;
            let mut block = files.get(key).unwrap();
            let relative = block[offset..offset + length].windows(b"DAMAGED MIDDLE".len()).position(|b| b == b"DAMAGED MIDDLE").unwrap();
            block[offset + relative] = b'X';
            files.set(key, block.clone());
            let mut app = LightcraftApp::new(open(&catalog, &files), Services::default());
            app.ui.language = language;
            app.ui.settings.gpu = false;
            app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
            app.services.png = Some(Box::new(|image| {
                lightcraft_engine::export::encode_image(
                    image,
                    &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                )
                .unwrap()
            }));
            let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
            h.app.run("ai.open", json!({})).unwrap();
            h.app.run("ai.history", json!({"photo":photo.0,"record":0})).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                h.step();
                let status = h.app.run("ai.status", json!({})).unwrap();
                if status["historyLoading"] == false {
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "actual page did not retire");
                std::thread::yield_now();
            }
            for _ in 0..5 {
                h.step();
            }
            assert!(h.painted_text().iter().any(|text| text == "INTACT BEFORE"));
            assert!(h.painted_text().iter().any(|text| text == "INTACT AFTER"));
            let placeholder = match language {
                Locale::En => "Message unavailable. Original data is retained.",
                _ => "消息不可用，原始数据已保留。",
            };
            assert!(h.painted_text().iter().any(|text| text == placeholder));
            assert!(!h.painted_text().iter().any(|text| text.contains("MIDDLE")));
            assert!(h.app.run("ai.status", json!({})).unwrap()["historyError"].is_string());
            assert_eq!(
                h.app.run("ai.start", json!({"photo":photo.0})).unwrap_err(),
                tr("The AI history is incomplete or invalid. Local history is retained. Repair attachments or delete the conversation to retry.")
            );
            super::panel_tests::reachable(&mut h, "button:aiCopyStorageDiagnostic", -1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiCopyStorageDiagnostic"}), Duration::from_secs(10))["ok"], true);
            assert!(h.view.clipboard.contains(key));
            assert!(h.view.clipboard.contains("record 1"));
            assert_eq!(h.app.session.companion_editor(photo).unwrap().draft, "Keep the unsent draft");
            assert_eq!(files.get(key).unwrap(), block);
            if let Ok(directory) = std::env::var("UX14_SCREENSHOT_DIR") {
                assert_eq!(h.request("ui.screenshot", json!({"path":format!("{directory}/neighbor-{}-{}.png",if background {"threaded"} else {"sync"},language.code()),"headless":true}), Duration::from_secs(30))["ok"], true);
            }
            if background {
                assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryNewer"}), Duration::from_secs(10))["ok"], true);
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                loop {
                    h.step();
                    let status = h.app.run("ai.status", json!({})).unwrap();
                    if status["historyLoading"] == false && status["text"].as_str().unwrap().starts_with("LATER VERIFIED\n") {
                        break;
                    }
                    assert!(std::time::Instant::now() < deadline, "Newer must reach verified records beyond the damaged page: {status}");
                    std::thread::yield_now();
                }
                assert!(h.app.run("ai.status", json!({})).unwrap()["historyError"].is_string());
                assert_eq!(h.app.session.companion_editor(photo).unwrap().draft, "Keep the unsent draft");
                assert_eq!(files.get(key).unwrap(), block);
            }
        }
    }
}

#[test]
fn protected_storage_shows_localized_repair_guidance_and_copies_an_owning_diagnostic_with_the_draft_kept() {
    for language in [Locale::En, Locale::ZhHans] {
        for (layout, size) in [("normal", [1400.0, 1000.0]), ("narrow", [640.0, 900.0])] {
            let (catalog, mut files) = (MemStore::new(), MemStore::new());
            let mut session = open(&catalog, &files);
            let photo = session.active().unwrap();
            let round = LocalRoundId::new().unwrap();
            let start = session.develop_of(photo).unwrap();
            let lease = session.acquire_companion_lease(photo, round.clone()).unwrap();
            let mut edited = (*start).clone();
            edited.light.exposure = 0.75;
            let op = session.develop_op(photo, edited, "AI edit plan").unwrap();
            session.commit_companion(&lease, &round, session.photo_revision(photo), op).unwrap();
            drop(lease);
            session
                .save_companion_conversation(
                    photo,
                    json!({"schema_version":2,"messages":[{"role":"user","text":"Saved request"}],"rounds":[],"round_records":[{"local_round_id":round.as_str(),"original":start,"recent_complete_result":session.develop_of(photo).unwrap(),"complete_state":"complete"}]}),
                )
                .unwrap();
            session.set_companion_editor(photo, EditorState { draft: "Keep this visible unsent 草稿".into(), ..Default::default() }).unwrap();
            session.close_library().unwrap();
            drop(session);
            let original = b"{broken identity for the actual recovery UI";
            files.set("companion/v2/store-identity.json", original.to_vec());
            let cold = open(&catalog, &files);
            let expected_core = serde_json::to_value(&cold.catalog).unwrap();
            let mut app = LightcraftApp::new(cold, Services::default());
            app.ui.language = language;
            app.ui.settings.gpu = false;
            app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
            app.services.png = Some(Box::new(|image| {
                lightcraft_engine::export::encode_image(
                    image,
                    &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                )
                .unwrap()
            }));
            let mut h = Headless::new(app, size, 1.0);
            h.app.run("ai.open", json!({})).unwrap();
            for _ in 0..5 {
                h.step();
            }
            let guidance = match language {
                Locale::En => {
                    "AI history is protected. Original files are kept. Back up the library, restore a verified companion backup, then reopen it."
                }
                _ => "AI 历史处于保护状态，原文件已保留。请先备份图库，恢复已验证的 companion 备份，再重新打开图库。",
            };
            assert!(h.painted_text().iter().any(|text| text == guidance), "actual localized recovery outlet {layout}/{}", language.code());
            let status = h.app.run("ai.status", json!({})).expect("protected archive diagnostics must remain queryable");
            assert_eq!(status["historyError"], guidance);
            assert_eq!(status["text"], guidance);
            assert!(h.painted_text().iter().any(|text| text == "Keep this visible unsent 草稿"), "verified independent draft is visible");
            super::panel_tests::reachable(&mut h, "button:aiCopyStorageDiagnostic", -1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiCopyStorageDiagnostic"}), Duration::from_secs(10))["ok"], true);
            assert!(h.view.clipboard.contains("companion/v2/store-identity.json"), "copy identifies the actual protected original file");
            assert!(h.view.clipboard.contains("key must be a string"), "copy includes the observed parser failure");
            assert_eq!(
                h.app.run("ai.start", json!({"photo":photo.0})).unwrap_err(),
                tr("This conversation format is protected. Original history is retained.")
            );
            assert!(h.app.session.verify_round_revert(photo, &round).is_err());
            assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), expected_core);
            assert_eq!(files.read("companion/v2/store-identity.json").unwrap().unwrap(), original);
            if let Ok(directory) = std::env::var("UX14_SCREENSHOT_DIR") {
                assert_eq!(
                    h.request(
                        "ui.screenshot",
                        json!({"path":format!("{directory}/protected-{layout}-{}.png",language.code()),"headless":true}),
                        Duration::from_secs(30)
                    )["ok"],
                    true
                );
            }
        }
    }
}

#[test]
fn protected_outer_storage_refuses_single_photo_delete_confirmation_from_every_command_entry() {
    for broken in [
        br#"{"format":"lightcraft-companion","version":99,"layout":"photo-head-v2"}"#.as_slice(),
        br#"{"format":"lightcraft-companion","version":2,"version":2,"layout":"photo-head-v2"}"#.as_slice(),
        b"{torn root".as_slice(),
    ] {
        let (catalog, mut files) = (MemStore::new(), MemStore::new());
        let mut session = open(&catalog, &files);
        let photos = session.catalog.photos().take(2).map(|photo| photo.id).collect::<Vec<_>>();
        let a = photos[0];
        let b = photos[1];
        for photo in [a, b] {
            session
                .save_companion_conversation(
                    photo,
                    json!({"schema_version":2,"messages":[{"role":"user","text":format!("Original photo {}",photo.0)}],"rounds":[],"round_records":[]}),
                )
                .unwrap();
        }
        session.set_companion_editor(a, EditorState { draft: "Keep A visible draft".into(), ..Default::default() }).unwrap();
        session.set_companion_editor(b, EditorState { draft: "Keep B independent draft".into(), ..Default::default() }).unwrap();
        session.close_library().unwrap();
        drop(session);
        files.set("companion/state.json", broken.to_vec());
        let cold = open(&catalog, &files);
        let expected_core = serde_json::to_value(&cold.catalog).unwrap();
        let mut app = LightcraftApp::new(cold, Services::default());
        app.ui.settings.gpu = false;
        let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        for _ in 0..5 {
            h.step();
        }
        for parameters in [json!({}), json!({"photo":a.0}), json!({"photo":b.0})] {
            assert!(h.app.run("ai.clear", parameters).is_err(), "unknown outer storage is not a single-photo confirmation");
            h.step();
            assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiClearConfirm"));
        }
        let generation = h.app.session.library_generation();
        assert!(h.app.run("ai.clear", json!({"photo":a.0,"confirm":true,"expected_epoch":0,"library_generation":generation})).is_err());
        assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), expected_core);
        assert_eq!(h.app.session.companion_editor(a).unwrap().draft, "Keep A visible draft");
        assert_eq!(h.app.session.companion_editor(b).unwrap().draft, "Keep B independent draft");
        assert_eq!(files.read("companion/state.json").unwrap().unwrap(), broken);
    }
}

#[test]
fn cold_damaged_inner_history_keeps_verified_text_blocks_submission_and_confirms_clear_without_full_hydration() {
    for language in [Locale::En, Locale::ZhHans] {
        for bad_metadata in [false, true] {
            let (catalog, files) = (MemStore::new(), MemStore::new());
            let mut session = open(&catalog, &files);
            let a = session.active().unwrap();
            let b = session.catalog.photos().find(|photo| photo.id != a).unwrap().id;
            session
                .save_companion_conversation(
                    a,
                    json!({"schema_version":if bad_metadata {2}else{999},"messages":[{"role":"user","text":"Readable request from damaged history"},{"role":"assistant","text":"Readable response from damaged history"}],"rounds":[],"round_records":[],"checkpoint":{"original":"broken checkpoint"}}),
                )
                .unwrap();
            session
                .save_companion_conversation(
                    b,
                    json!({"schema_version":2,"messages":[{"role":"user","text":"B independent history"}],"rounds":[],"round_records":[]}),
                )
                .unwrap();
            session.set_companion_editor(a, EditorState { draft: "Keep A unsent request".into(), ..Default::default() }).unwrap();
            session.set_companion_editor(b, EditorState { draft: "Keep B unsent request".into(), ..Default::default() }).unwrap();
            let header = session.companion_photo_header(a).unwrap().unwrap();
            session.close_library().unwrap();
            drop(session);
            if bad_metadata {
                files.set(&header.metadata, b"{broken checkpoint metadata".to_vec());
            }
            let cold = open(&catalog, &files);
            let expected_core = serde_json::to_value(&cold.catalog).unwrap();
            let mut app = LightcraftApp::new(cold, Services::default());
            app.ui.language = language;
            app.ui.settings.gpu = false;
            app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
            app.services.png = Some(Box::new(|image| {
                lightcraft_engine::export::encode_image(
                    image,
                    &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                )
                .unwrap()
            }));
            let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
            h.app.run("ai.open", json!({})).unwrap();
            for _ in 0..8 {
                h.step();
            }
            assert_eq!(h.app.session.active(), Some(a));
            assert_eq!(h.app.run("ai.status", json!({})).unwrap()["viewedPhoto"], a.0);
            assert!(h.painted_text().iter().any(|text| text == "Readable request from damaged history"));
            assert!(h.painted_text().iter().any(|text| text == "Readable response from damaged history"));
            assert!(h.painted_text().iter().any(|text| text == "Keep A unsent request"));
            assert_eq!(
                h.app.run("ai.start", json!({"photo":a.0})).unwrap_err(),
                tr("The AI history is incomplete or invalid. Local history is retained. Repair attachments or delete the conversation to retry.")
            );
            assert_eq!(h.app.run("ai.status", json!({})).unwrap()["busy"], false);
            if let Ok(directory) = std::env::var("UX14_SCREENSHOT_DIR") {
                assert_eq!(
                    h.request(
                        "ui.screenshot",
                        json!({"path":format!("{directory}/inner-{}-{}.png",if bad_metadata {"metadata"}else{"schema"},language.code()),"headless":true}),
                        Duration::from_secs(30)
                    )["ok"],
                    true
                );
            }
            super::panel_tests::reachable(&mut h, "button:aiClear", -1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClear"}), Duration::from_secs(10))["ok"], true);
            assert!(h.painted_text().iter().any(|text| {
                text == tr(
                    "Plans and comparisons in this conversation will also be deleted. Photo effects, edit history and the unsent draft are kept.",
                )
            }));
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiClearConfirm"}), Duration::from_secs(10))["ok"], true);
            for _ in 0..8 {
                h.step();
            }
            let status = h.app.run("ai.status", json!({})).unwrap();
            assert!(!status["text"].as_str().unwrap().contains("Readable request from damaged history"));
            assert_eq!(h.app.session.companion_photo_header(a).unwrap().unwrap().conversation_epoch, header.conversation_epoch + 1);
            assert!(h.app.session.companion_messages_page(a, 0, 32).unwrap().records.is_empty());
            assert_eq!(h.app.session.companion_messages_page(b, 0, 32).unwrap().records[0]["text"], "B independent history");
            assert_eq!(h.app.session.companion_editor(a).unwrap().draft, "Keep A unsent request");
            assert_eq!(h.app.session.companion_editor(b).unwrap().draft, "Keep B unsent request");
            assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), expected_core);
            assert!(h.app.session.acquire_companion_lease(a, LocalRoundId::new().unwrap()).is_ok());
        }
    }
}

#[test]
fn unverified_photo_head_has_owning_repair_diagnostics_and_never_reads_or_clears_another_photos_history() {
    for language in [Locale::En, Locale::ZhHans] {
        let (catalog, mut files) = (MemStore::new(), MemStore::new());
        let mut session = open(&catalog, &files);
        let a = session.active().unwrap();
        let b = session.catalog.photos().find(|photo| photo.id != a).unwrap().id;
        for photo in [a, b] {
            session
                .save_companion_conversation(
                    photo,
                    json!({"schema_version":2,"messages":[{"role":"user","text":if photo==a {"A original history"}else{"B original history"}}],"rounds":[],"round_records":[]}),
                )
                .unwrap();
        }
        session.set_companion_editor(a, EditorState { draft: "A owned unsent draft".into(), ..Default::default() }).unwrap();
        session.close_library().unwrap();
        drop(session);
        let a_head = format!("companion/v2/photos/{}/HEAD.json", a.0);
        let b_head = format!("companion/v2/photos/{}/HEAD.json", b.0);
        let unrelated = files.read(&b_head).unwrap().unwrap();
        files.set(&a_head, unrelated.clone());
        let cold = open(&catalog, &files);
        let expected_core = serde_json::to_value(&cold.catalog).unwrap();
        let mut app = LightcraftApp::new(cold, Services::default());
        app.ui.language = language;
        app.ui.settings.gpu = false;
        app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        for _ in 0..5 {
            h.step();
        }
        assert_eq!(h.app.session.active(), Some(a));
        let guidance = match language {
            Locale::En => {
                "This photo's AI history cannot be verified. Original files are kept. Repair it from this library's verified backup, then reopen the library."
            }
            _ => "此照片的 AI 历史无法验证，原文件已保留。请从该图库已验证的备份修复，再重新打开图库。",
        };
        assert!(h.painted_text().iter().any(|text| text == guidance));
        assert_eq!(h.app.run("ai.status", json!({})).unwrap()["historyError"], guidance);
        assert!(!h.painted_text().iter().any(|text| text == "B original history"));
        super::panel_tests::reachable(&mut h, "button:aiCopyStorageDiagnostic", -1.0);
        assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiCopyStorageDiagnostic"}), Duration::from_secs(10))["ok"], true);
        assert!(h.view.clipboard.contains(&a_head));
        assert!(h.app.run("ai.clear", json!({"photo":a.0})).is_err());
        assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiClearConfirm"));
        let generation = h.app.session.library_generation();
        assert!(h.app.run("ai.clear", json!({"photo":a.0,"confirm":true,"expected_epoch":0,"library_generation":generation})).is_err());
        assert_eq!(files.read(&a_head).unwrap().unwrap(), unrelated);
        assert_eq!(files.read(&b_head).unwrap().unwrap(), unrelated);
        assert_eq!(h.app.session.companion_messages_page(b, 0, 32).unwrap().records[0]["text"], "B original history");
        assert_eq!(h.app.session.companion_editor(a).unwrap().draft, "A owned unsent draft");
        assert_eq!(serde_json::to_value(&h.app.session.catalog).unwrap(), expected_core);
        if let Ok(directory) = std::env::var("UX14_SCREENSHOT_DIR") {
            assert_eq!(
                h.request(
                    "ui.screenshot",
                    json!({"path":format!("{directory}/ownership-{}.png",language.code()),"headless":true}),
                    Duration::from_secs(30)
                )["ok"],
                true
            );
        }
    }
}
