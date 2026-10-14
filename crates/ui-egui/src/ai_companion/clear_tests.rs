//! Clear through the same external commands and widgets as the application.
use crate::{LightcraftApp, Services};
use lightcraft_engine::Session;
use serde_json::json;

#[test]
fn unconfirmed_clear_keeps_the_conversation_and_returns_its_scope() {
    let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
    let photo = app.session.active().unwrap();
    app.session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"Keep this old request"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
    let reply = app.run("ai.clear", json!({})).unwrap();
    assert_eq!(reply["confirmationRequired"], true);
    assert_eq!(reply["photo"], photo.0);
    assert_eq!(reply["libraryGeneration"], app.session.library_generation());
    assert_eq!(app.session.load_companion_conversation(photo).unwrap().unwrap()["messages"][0]["text"], "Keep this old request");
}

#[test]
fn confirmed_clear_from_another_library_refuses_before_touching_equal_photo_id() {
    use lightcraft_catalog::MemStore;
    use lightcraft_engine::library::LibraryStores;
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "clear-A".into(), on_disk: false },
            true,
        )
        .unwrap();
    let mut app = LightcraftApp::new(session, Services::default());
    let photo = app.session.active().unwrap();
    app.session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"A history"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
    let confirmation = app.run("ai.clear", json!({})).unwrap();
    app.session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "clear-B".into(), on_disk: false },
            true,
        )
        .unwrap();
    assert_eq!(app.session.active(), Some(photo));
    app.session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"B history must remain"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
    let before = serde_json::to_value(&app.session.catalog).unwrap();
    let undo = app.session.undo.iter().map(|entry| entry.label.clone()).collect::<Vec<_>>();
    let result = app.run("ai.clear",json!({"confirm":true,"photo":confirmation["photo"],"library_generation":confirmation["libraryGeneration"],"expected_epoch":confirmation["expectedEpoch"]}));
    assert!(result.is_err(), "old-library confirmation is not authority to clear the new library");
    assert_eq!(app.session.load_companion_conversation(photo).unwrap().unwrap()["messages"][0]["text"], "B history must remain");
    assert_eq!(serde_json::to_value(&app.session.catalog).unwrap(), before);
    assert_eq!(app.session.undo.iter().map(|entry| entry.label.clone()).collect::<Vec<_>>(), undo);
}
