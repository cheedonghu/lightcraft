use lightcraft_catalog::MemStore;
use lightcraft_engine::{Session, companion::LocalRoundId, library::LibraryStores};
use serde_json::json;

#[test]
fn unavailable_develop_targets_refuse_the_whole_batch_and_leave_other_photos_editable() {
    let mut session = Session::with_demo();
    let deleted = session.active().unwrap();
    let other = session.catalog.photos().find(|p| p.id != deleted).unwrap().id;
    session.execute("photo.delete", &json!({"ids":[deleted.0]})).unwrap();
    let original = serde_json::to_value(&session.catalog).unwrap();
    assert!(session.execute("develop.set", &json!({"ids":[other.0,deleted.0],"values":{"light.exposure":0.7}})).is_err());
    assert_eq!(serde_json::to_value(&session.catalog).unwrap(), original, "no earlier available target or history can change in the refused batch");
    session.execute("develop.set", &json!({"ids":[other.0],"values":{"light.exposure":0.4}})).unwrap();
    assert_eq!(session.develop_of(other).unwrap().light.exposure, 0.4);
    session.execute("photo.restore", &json!({"ids":[deleted.0]})).unwrap();
    session.execute("develop.set", &json!({"ids":[deleted.0],"values":{"light.exposure":0.2}})).unwrap();
    assert_eq!(session.develop_of(deleted).unwrap().light.exposure, 0.2, "explicit restore makes parameter editing available again");
}

#[test]
fn captured_message_projection_keeps_reliable_completion_and_image_binding() {
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(MemStore::new()),
                dir: "comparison-projection".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let round = LocalRoundId::new().unwrap();
    let id = format!("{}-user", round.as_str());
    session.save_companion_conversation(photo,json!({"schema_version":2,"messages":[{
        "role":"user","local_round_id":round,"local_message_id":id,"display_kind":"request","complete_state":"complete","text":"Saved request","opaque_extension":"x".repeat(2*1024*1024)
    }],"rounds":[],"round_records":[]})).unwrap();
    let header = session.companion_photo_header(photo).unwrap().unwrap();
    let mut reader = session.companion_owned_history_reader(&header).unwrap().unwrap();
    let record = reader.message_display(0).unwrap();
    assert_eq!(record["complete_state"], "complete");
    assert_eq!(record["local_message_id"], id);
    assert_eq!(record["local_round_id"], round.as_str());
    assert!(record.get("opaque_extension").is_none());
    assert_eq!(session.companion_messages_page(photo, 0, 1).unwrap().records[0]["opaque_extension"].as_str().unwrap().len(), 2 * 1024 * 1024);
}
