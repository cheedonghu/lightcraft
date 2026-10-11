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
fn public_round_undo_restores_complete_settings_and_retains_conversation() {
    let (mut app, photo, original) = archived();
    app.run("ai.undo", json!({})).unwrap();
    assert_eq!(*app.session.develop_of(photo).unwrap(), original);
    let archive = app.session.catalog.photo(photo).unwrap().companion.as_ref().unwrap();
    assert_eq!(archive["messages"].as_array().unwrap().len(), 2);
    assert_eq!(archive["checkpoint"]["eligible"], false);
    assert!(app.run("ai.undo", json!({})).is_err());
}
#[test]
fn unopened_conversation_cannot_undo_after_manual_change_away_and_back() {
    let (mut app, photo, _) = archived();
    let before = app.session.develop_of(photo).unwrap();
    app.run("develop.set", json!({"values":{"light.exposure":0.5}})).unwrap();
    app.run("develop.set", json!({"values":{"light.exposure":before.light.exposure}})).unwrap();
    assert_eq!(*app.session.develop_of(photo).unwrap(), *before);
    assert!(app.run("ai.undo", json!({})).unwrap_err().contains("历史恢复"));
    app.run("ai.clear", json!({})).unwrap();
    assert!(app.session.catalog.photo(photo).unwrap().companion.is_none());
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
