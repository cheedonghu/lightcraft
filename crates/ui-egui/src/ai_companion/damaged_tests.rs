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
            assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), Duration::from_secs(10))["ok"], true);
            assert_eq!(
                h.app.run("ai.start", json!({"photo":a.0})).unwrap_err(),
                tr("The AI history is incomplete or invalid. Local history is retained. Repair attachments or delete the conversation to retry.")
            );
            assert_eq!(h.app.run("ai.status", json!({})).unwrap()["busy"], false);
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
