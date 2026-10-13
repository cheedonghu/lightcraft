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
