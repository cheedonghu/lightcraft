//! Public command regressions for photo-local undo and conversation deletion.
use crate::{LightcraftApp, Services};
use lightcraft_engine::Session;
use serde_json::json;

fn archived() -> (LightcraftApp, lightcraft_catalog::PhotoId, lightcraft_develop::DevelopSettings) {
    let mut session = Session::with_demo();
    let photo = session.active().unwrap();
    let expected = session.develop_of(photo).unwrap();
    let mut original = (*expected).clone();
    original.light.exposure = -0.2;
    original.effects.clarity = 17.0;
    session.set_photo_companion(photo,Some(json!({"schema_version":1,"messages":[{"role":"user","text":"仅此照片的对话"}],"rounds":[],"checkpoint":{"original":original,"expected":expected,"eligible":true}}))).unwrap();
    (LightcraftApp::new(session, Services::default()), photo, original)
}
#[test]
fn loaded_archive_has_no_runtime_round_revert_without_verified_certificate() {
    let (mut app, photo, _) = archived();
    let before = app.session.develop_of(photo).unwrap();
    assert!(app.run("ai.undo", json!({})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
    assert!(app.run("ai.undo", json!({"local_round_id":"old-round"})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
}
#[test]
fn unopened_conversation_cannot_undo_after_manual_change_away_and_back() {
    let (mut app, photo, _) = archived();
    let before = app.session.develop_of(photo).unwrap();
    app.run("develop.set", json!({"values":{"light.exposure":0.5}})).unwrap();
    app.run("develop.set", json!({"values":{"light.exposure":before.light.exposure}})).unwrap();
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
    assert!(app.run("ai.undo", json!({"local_round_id":"old-round"})).unwrap_err().contains("History"));
    app.run("ai.clear", json!({})).unwrap();
    assert!(app.session.photo_companion(photo).unwrap().is_none());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
}

#[test]
fn conversation_and_preview_survive_reopen_and_clear_removes_only_its_attachment() {
    let dir = std::env::temp_dir().join(format!("lc-companion-archive-{}", std::process::id()));
    let mut session = Session::new().with_fs();
    session.open_library(&dir, true).unwrap();
    let photo = session.catalog.photos().next().unwrap().id;
    session.execute("library.select", &json!({"ids":[photo.0],"active":photo.0})).unwrap();
    let before = session.develop_of(photo).unwrap();
    let name = session.library.as_mut().unwrap().companion_attachment(photo, b"synthetic preview").unwrap();
    assert!(session.library.as_mut().unwrap().read_companion_attachment("../catalog.snap").is_err());
    session.set_photo_companion(photo, Some(json!({"schema_version":1,"messages":[{"role":"user","text":"恢复对话","images":[{"mime_type":"image/jpeg","attachment":name}]}],"rounds":[]}))).unwrap();
    session.close_library().unwrap();
    session.open_library(&dir, false).unwrap();
    let mut app = LightcraftApp::new(session, Services::default());
    let status = app.run("ai.status", json!({})).unwrap();
    assert!(status["text"].as_str().unwrap().contains("恢复对话"));
    assert_eq!(app.session.library.as_mut().unwrap().read_companion_attachment(&name).unwrap(), b"synthetic preview");
    app.run("ai.clear", json!({})).unwrap();
    assert!(!dir.join(name).exists());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
    app.session.close_library().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

fn published_round() -> (
    LightcraftApp,
    lightcraft_catalog::PhotoId,
    lightcraft_catalog::PhotoId,
    lightcraft_engine::companion::LocalRoundId,
    lightcraft_develop::DevelopSettings,
) {
    let mut session = Session::with_demo();
    let photo = session.active().unwrap();
    let other = session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    let mut start = (*session.develop_of(photo).unwrap()).clone();
    start.light.exposure = -0.2;
    start.effects.clarity = 17.0;
    session.set_develop(photo, start.clone(), "Manual starting edit").unwrap();
    let round = lightcraft_engine::companion::LocalRoundId::new().unwrap();
    let lease = session.acquire_companion_lease(photo, round.clone()).unwrap();
    let mut result = start.clone();
    for value in [0.4, 0.2] {
        result.light.exposure = value;
        result.effects.clarity = 30.0;
        let op = session.develop_op(photo, result.clone(), "AI edit plan").unwrap();
        session.commit_companion(&lease, &round, session.photo_revision(photo), op).unwrap();
    }
    drop(lease);
    session.set_photo_companion(photo, Some(json!({"schema_version":2,"messages":[{"role":"user","text":"Optimize this photo.","local_round_id":round}],"round_records":[],"rounds":[]}))).unwrap();
    (LightcraftApp::new(session, Services::default()), photo, other, round, start)
}
#[test]
fn explicit_live_round_revert_restores_the_whole_start_only_on_its_photo() {
    let (mut app, photo, other, round, start) = published_round();
    let other_start = app.session.develop_of(other).unwrap();
    let before = app.session.develop_of(photo).unwrap();
    assert!(app.run("ai.undo", json!({"local_round_id":"a-different-round"})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
    app.run("library.select", json!({"ids":[photo.0,other.0],"active":photo.0})).unwrap();
    app.session.auto_sync = true;
    assert_eq!(app.run("ai.undo", json!({"photo":photo.0,"local_round_id":round})).unwrap()["undone"], true);
    assert_eq!(*app.session.develop_of(photo).unwrap(), start);
    assert_eq!(*app.session.develop_of(other).unwrap(), *other_start);
    let archive = app.session.photo_companion(photo).unwrap().unwrap();
    assert!(archive["messages"].as_array().unwrap().iter().any(|m| m["display_kind"] == "round_reverted" && m["local_round_id"] == round.as_str()));
    assert!(app.run("ai.undo", json!({"local_round_id":round})).is_err());
}

#[test]
fn newer_identical_round_has_no_later_edit_warning_or_revert_permission() {
    use lightcraft_engine::companion::{LocalRoundId, RoundRevertState};
    let (mut app, photo, _, earlier, start) = published_round();
    let result = app.session.develop_of(photo).unwrap();
    let identical = LocalRoundId::new().unwrap();
    let lease = app.session.acquire_companion_lease(photo, identical.clone()).unwrap();
    let op = app.session.develop_op(photo, (*result).clone(), "AI edit plan").unwrap();
    app.session.commit_companion(&lease, &identical, app.session.photo_revision(photo), op).unwrap();
    drop(lease);
    app.session.set_photo_companion(photo, Some(json!({"schema_version":2,"messages":[],"rounds":[],"round_records":[{
        "local_round_id":identical,"photo_id":photo.0,"recent_complete_result":result,"recent_complete_hash":result.hash64(),
        "stages":[{"stage":"initial","reason":"Identical result; no parameters changed.","locally_published":true,"before_state":result,"applied_state":result,"execution_outcome":"succeeded","render_outcome":"succeeded"}]
    }, {
        "local_round_id":earlier,"photo_id":photo.0,"recent_complete_result":result,"recent_complete_hash":result.hash64(),
        "stages":[{"stage":"correction","locally_published":true,"before_state":start,"applied_state":result,"execution_outcome":"succeeded","render_outcome":"succeeded"}]
    }]}))).unwrap();
    let mut h = crate::headless::Headless::new(app, [1400.0, 1000.0], 1.0);
    let app = &mut h.app;
    let status = app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["resultCards"][0]["currentPhotoHasLaterEdits"], false);
    assert_eq!(status["resultCards"][0]["canUndo"], false);
    assert_eq!(app.session.companion_round_revert_state(photo, &identical), RoundRevertState::Unverified);
    assert!(app.session.verify_round_revert(photo, &earlier).is_ok());
    assert!(app.run("ai.undo", json!({"photo":photo.0,"local_round_id":identical})).is_err());
    identical_card_screens(&mut h);
    let app = &mut h.app;
    app.run("develop.set", json!({"values":{"light.exposure":0.5}})).unwrap();
    app.run("develop.set", json!({"values":{"light.exposure":0.2}})).unwrap();
    assert_eq!(app.session.companion_round_revert_state(photo, &earlier), RoundRevertState::LaterEdits);
    assert!(app.run("ai.undo", json!({"photo":photo.0,"local_round_id":earlier})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *result);
    let newer = LocalRoundId::new().unwrap();
    let lease = app.session.acquire_companion_lease(photo, newer.clone()).unwrap();
    for exposure in [0.7, 0.2] {
        let mut settings = (*result).clone();
        settings.light.exposure = exposure;
        let op = app.session.develop_op(photo, settings, "AI edit plan").unwrap();
        app.session.commit_companion(&lease, &newer, app.session.photo_revision(photo), op).unwrap();
        let status = app.run("ai.status", json!({})).unwrap();
        let old_card = status["resultCards"].as_array().unwrap().iter().find(|card| card["localRoundId"] == earlier.as_str()).unwrap();
        assert_eq!(old_card["canUndo"], false, "a superseded card never regains permission even when its result returns");
        if exposure == 0.7 {
            assert_eq!(old_card["currentPhotoHasLaterEdits"], true, "the saved result still exposes actual current parameter differences");
        }
    }
    drop(lease);
    assert!(app.run("ai.undo", json!({"photo":photo.0,"local_round_id":earlier})).is_err());
}

fn identical_card_screens(h: &mut crate::headless::Headless) {
    let Ok(dir) = std::env::var("UX17_SCREENSHOT_DIR") else { return };
    h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
    h.app.services.png = Some(Box::new(|image| {
        lightcraft_engine::export::encode_image(
            image,
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
        )
        .unwrap()
    }));
    h.app.run("ai.open", json!({})).unwrap();
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        h.app.ui.language = language;
        for _ in 0..4 {
            h.step();
        }
        assert_eq!(
            h.request(
                "ui.screenshot",
                json!({"path":format!("{dir}/identical-normal-{}.png",language.code()),"headless":true}),
                std::time::Duration::from_secs(30)
            )["ok"],
            true
        );
    }
}

#[test]
fn live_round_cannot_regain_revert_after_manual_edit_away_and_back() {
    let (mut app, photo, _, round, _) = published_round();
    app.run("develop.set", json!({"values":{"light.exposure":0.5}})).unwrap();
    app.run("develop.set", json!({"values":{"light.exposure":0.2}})).unwrap();
    let before = app.session.develop_of(photo).unwrap();
    assert_eq!(before.light.exposure, 0.2);
    assert!(app.run("ai.undo", json!({"local_round_id":round})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
}

#[test]
fn ordinary_history_restore_does_not_renew_a_round_even_at_identical_values() {
    let (mut app, photo, _, round, _) = published_round();
    let index = app.session.catalog.photo(photo).unwrap().history.len() - 1;
    app.run("history.restore", json!({"index":index})).unwrap();
    let before = app.session.develop_of(photo).unwrap();
    assert_eq!(before.light.exposure, 0.2);
    assert!(app.run("ai.undo", json!({"local_round_id":round})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
}

#[test]
fn non_editing_activity_keeps_the_explicit_earlier_editing_round_qualified() {
    let (mut app, photo, _, round, start) = published_round();
    app.run("develop.beginInteraction", json!({"label":"Focus exposure"})).unwrap();
    app.run("develop.endInteraction", json!({})).unwrap();
    app.run("develop.set", json!({"values":{"light.exposure":0.2}})).unwrap();
    let answer = lightcraft_engine::companion::LocalRoundId::new().unwrap();
    let lease = app.session.acquire_companion_lease(photo, answer.clone()).unwrap();
    drop(lease); // A pure answer has no edit publication.
    assert!(app.run("ai.undo", json!({"local_round_id":answer})).is_err());
    let no_op = lightcraft_engine::companion::LocalRoundId::new().unwrap();
    let lease = app.session.acquire_companion_lease(photo, no_op.clone()).unwrap();
    let op = app.session.develop_op(photo, (*app.session.develop_of(photo).unwrap()).clone(), "AI edit plan").unwrap();
    app.session.commit_companion(&lease, &no_op, app.session.photo_revision(photo), op).unwrap();
    drop(lease);
    assert!(app.run("ai.undo", json!({"local_round_id":no_op})).is_err());
    app.run("ai.undo", json!({"local_round_id":round})).unwrap();
    assert_eq!(*app.session.develop_of(photo).unwrap(), start);
}

#[test]
fn source_relink_away_and_back_never_renews_runtime_round_permission() {
    let (mut app, photo, other, round, _) = published_round();
    let p = app.session.catalog.photo(photo).unwrap().clone();
    let other_source = app.session.catalog.photo(other).unwrap().source.clone();
    for source in [other_source, p.source.clone()] {
        app.session.commit("Relink", lightcraft_catalog::Op::Relink { id: photo, file_name: p.file_name.clone(), source, format: None }).unwrap();
    }
    let before = app.session.develop_of(photo).unwrap();
    assert_eq!(app.session.catalog.photo(photo).unwrap().source, p.source);
    assert!(app.run("ai.undo", json!({"local_round_id":round})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
}

#[test]
fn file_source_away_and_back_never_renews_runtime_round_permission() {
    let (mut app, photo, other, round, _) = published_round();
    let p = app.session.catalog.photo(photo).unwrap().clone();
    let other_source = app.session.catalog.photo(other).unwrap().source.clone();
    for source in [other_source, p.source.clone()] {
        app.session.commit("File source", lightcraft_catalog::Op::SetFile { id: photo, file_name: p.file_name.clone(), source }).unwrap();
    }
    let before = app.session.develop_of(photo).unwrap();
    assert!(app.run("ai.undo", json!({"local_round_id":round})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
}

#[test]
fn partial_correction_undo_and_redo_never_restore_round_permission() {
    let (mut app, photo, _, round, _) = published_round();
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.develop_of(photo).unwrap().light.exposure, 0.4);
    assert!(app.run("ai.undo", json!({"local_round_id":round})).is_err());
    app.run("edit.redo", json!({})).unwrap();
    let current = app.session.develop_of(photo).unwrap();
    assert_eq!(current.light.exposure, 0.2);
    assert!(app.run("ai.undo", json!({"local_round_id":round})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *current);
}
#[test]
fn other_photo_task_does_not_block_local_round_restore() {
    let (mut app, photo, other, round, start) = published_round();
    let other_before = app.session.develop_of(other).unwrap();
    let _lease = app.session.acquire_companion_lease(other, lightcraft_engine::companion::LocalRoundId::new().unwrap()).unwrap();
    app.run("ai.undo", json!({"photo":photo.0,"local_round_id":round})).unwrap();
    assert_eq!(*app.session.develop_of(photo).unwrap(), start);
    assert_eq!(*app.session.develop_of(other).unwrap(), *other_before);
}

fn visit_twenty_new_photo_conversations(app: &mut LightcraftApp, photo: lightcraft_catalog::PhotoId) {
    app.run("ai.status", json!({"photo":photo.0})).unwrap();
    let template = app.session.catalog.photo(photo).unwrap().as_ref().clone();
    for index in 0..20 {
        let id = app.session.catalog.alloc_photo_id();
        let mut p = template.clone();
        p.id = id;
        app.session.commit("Procedural photo", lightcraft_catalog::Op::AddPhoto { photo: Box::new(p) }).unwrap();
        app.session.set_photo_companion(id,Some(json!({"schema_version":2,"messages":[{"role":"user","text":format!("Photo conversation {index}")}],"rounds":[],"round_records":[]}))).unwrap();
        app.run("library.select", json!({"ids":[id.0],"active":id.0})).unwrap();
        app.run("ai.status", json!({})).unwrap();
    }
    app.run("library.select", json!({"ids":[photo.0],"active":photo.0})).unwrap();
    app.run("ai.status", json!({})).unwrap();
}
#[test]
fn photo_read_cache_retirement_does_not_lose_trusted_runtime_permission() {
    let (mut app, photo, _, round, start) = published_round();
    visit_twenty_new_photo_conversations(&mut app, photo);
    app.run("ai.undo", json!({"local_round_id":round})).unwrap();
    assert_eq!(*app.session.develop_of(photo).unwrap(), start);
}
#[test]
fn photo_read_cache_retirement_never_rehydrates_permission_after_later_edits() {
    let (mut app, photo, _, round, _) = published_round();
    app.run("develop.set", json!({"values":{"light.exposure":0.5}})).unwrap();
    app.run("develop.set", json!({"values":{"light.exposure":0.2}})).unwrap();
    visit_twenty_new_photo_conversations(&mut app, photo);
    let current = app.session.develop_of(photo).unwrap();
    assert!(app.run("ai.undo", json!({"local_round_id":round})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *current);
}

#[test]
fn malformed_explicit_round_photo_cannot_fall_back_to_the_active_photo() {
    let (mut app, photo, _, round, _) = published_round();
    let current = app.session.develop_of(photo).unwrap();
    assert!(app.run("ai.undo", json!({"photo":"invalid-photo","local_round_id":round})).is_err());
    assert_eq!(*app.session.develop_of(photo).unwrap(), *current);
}
