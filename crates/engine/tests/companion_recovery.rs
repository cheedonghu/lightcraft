use lightcraft_catalog::{MemStore, PhotoId, Store};
use lightcraft_engine::{Session, companion::LocalRoundId, library::LibraryStores};
use serde_json::json;

fn open(catalog: &MemStore, files: &MemStore) -> Session {
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "ux14-recovery".into(), on_disk: false },
            true,
        )
        .expect("trustworthy core photos must open under durable protection");
    session
}

fn publish(session: &mut Session, photo: PhotoId) -> LocalRoundId {
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
            json!({"schema_version":2,"messages":[{"role":"user","text":"brighten"}],"rounds":[],"round_records":[{"local_round_id":round.as_str(),"original":start,"recent_complete_result":session.develop_of(photo).unwrap(),"complete_state":"complete"}]}),
        )
        .unwrap();
    round
}

#[test]
fn damaged_storage_identity_keeps_core_photos_and_original_metadata_under_a_durable_fence() {
    let (catalog, mut files) = (MemStore::new(), MemStore::new());
    let mut session = open(&catalog, &files);
    let photo = session.active().unwrap();
    let round = publish(&mut session, photo);
    let expected = serde_json::to_value(&session.catalog).unwrap();
    session.close_library().unwrap();
    drop(session);
    let original = b"{broken storage identity";
    files.set("companion/v2/store-identity.json", original.to_vec());

    let mut cold = open(&catalog, &files);
    assert_eq!(serde_json::to_value(&cold.catalog).unwrap(), expected);
    assert_eq!(files.read("companion/v2/store-identity.json").unwrap().unwrap(), original);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&files.read("companion/v2/load-proof.json").unwrap().unwrap()).unwrap()["pending"], true);
    assert!(cold.companion_archive_protected_reason().is_some());
    assert!(cold.verify_round_revert(photo, &round).is_err());
    assert!(cold.acquire_companion_lease(photo, LocalRoundId::new().unwrap()).is_err());
}

#[test]
fn protected_identity_allows_ordinary_edit_history_save_and_normal_close_without_replacing_bad_metadata() {
    let (catalog, mut files) = (MemStore::new(), MemStore::new());
    let mut session = open(&catalog, &files);
    let photo = session.active().unwrap();
    publish(&mut session, photo);
    session.close_library().unwrap();
    drop(session);
    let original = b"{bad identity retained through normal close";
    files.set("companion/v2/store-identity.json", original.to_vec());

    let mut cold = open(&catalog, &files);
    let before = cold.develop_of(photo).unwrap();
    let mut edited = (*before).clone();
    edited.light.exposure = 1.25;
    let op = cold.develop_op(photo, edited.clone(), "Ordinary edit during protected recovery").unwrap();
    cold.commit("Ordinary edit during protected recovery", op).unwrap();
    cold.undo_step().unwrap();
    assert_eq!(cold.develop_of(photo).unwrap(), before);
    cold.redo_step().unwrap();
    assert_eq!(*cold.develop_of(photo).unwrap(), edited);
    cold.persist().unwrap();
    cold.close_library().expect("durably fenced AI metadata must not block ordinary core closing");
    assert_eq!(files.read("companion/v2/store-identity.json").unwrap().unwrap(), original);
    drop(cold);
    let reopened = open(&catalog, &files);
    assert_eq!(*reopened.develop_of(photo).unwrap(), edited);
    assert!(reopened.companion_archive_protected_reason().is_some());
}

#[test]
fn protected_archive_still_reads_the_verified_independent_unsent_draft() {
    use lightcraft_engine::companion::EditorState;
    let (catalog, mut files) = (MemStore::new(), MemStore::new());
    let mut session = open(&catalog, &files);
    let photo = session.active().unwrap();
    publish(&mut session, photo);
    let draft = EditorState { draft: "Keep this unsent 草稿".into(), scroll_offset: 406.0, follow_tail: false, ..Default::default() };
    session.set_companion_editor(photo, draft.clone()).unwrap();
    session.close_library().unwrap();
    drop(session);
    let original = b"{damaged AI identity, intact independent editor";
    files.set("companion/v2/store-identity.json", original.to_vec());

    let mut cold = open(&catalog, &files);
    assert_eq!(cold.companion_editor(photo).expect("read verified draft without hydrating protected history"), draft);
    assert!(cold.companion_archive_protected_reason().is_some());
    assert_eq!(files.read("companion/v2/store-identity.json").unwrap().unwrap(), original);
}

#[test]
fn duplicate_storage_identity_fields_are_protected_and_cannot_regrant_a_saved_round() {
    let (catalog, mut files) = (MemStore::new(), MemStore::new());
    let mut session = open(&catalog, &files);
    let photo = session.active().unwrap();
    let round = publish(&mut session, photo);
    session.close_library().unwrap();
    drop(session);
    let saved: serde_json::Value = serde_json::from_slice(&files.read("companion/v2/store-identity.json").unwrap().unwrap()).unwrap();
    let identity = serde_json::to_string(&saved["identity"]).unwrap();
    let original = format!("{{\"version\":1,\"identity\":{identity},\"identity\":{identity}}}");
    files.set("companion/v2/store-identity.json", original.as_bytes().to_vec());

    let mut cold = open(&catalog, &files);
    assert!(cold.verify_round_revert(photo, &round).is_err(), "duplicate owning fields are not clean authority even if values match");
    assert!(cold.companion_archive_protected_reason().is_some());
    assert!(cold.acquire_companion_lease(photo, LocalRoundId::new().unwrap()).is_err());
    assert_eq!(files.read("companion/v2/store-identity.json").unwrap().unwrap(), original.as_bytes());
}
