use super::*;
use crate::library::LibraryStores;
use lightcraft_catalog::journal::{LOG, SNAPSHOT, VERSION, encode_record};
use lightcraft_catalog::{MemStore, Op};

fn open(catalog: MemStore, files: Box<dyn Store>) -> Session {
    let mut s = Session::new();
    s.open_library_in(LibraryStores { catalog: Box::new(catalog), files, dir: "memory".into(), on_disk: false }, true).unwrap();
    s
}

#[test]
fn standalone_archive_survives_reopen_without_changing_catalog_version_or_journal() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(catalog.clone(), Box::new(files.clone()));
    let id = s.catalog.photos().next().unwrap().id;
    let other = s.catalog.photos().nth(1).unwrap().id;
    let other_value = json!({"messages":["other photo"],"checkpoint":{"eligible":true}});
    s.set_photo_companion(other, Some(other_value.clone())).unwrap();
    let attachment = s.library.as_mut().unwrap().companion_attachment(other, b"other preview").unwrap();
    let before = catalog.files.lock().unwrap().clone();
    let conversation = json!({"schema_version":1,"messages":[{"text":"conversation"}],"checkpoint":{"eligible":true}});
    s.set_photo_companion(id, Some(conversation.clone())).unwrap();
    assert_eq!(*catalog.files.lock().unwrap(), before, "AI saves never append catalog ops or rewrite snapshots");
    let value: Value = serde_json::from_slice(&catalog.get(SNAPSHOT).unwrap()).unwrap();
    assert_eq!(value["version"], VERSION);
    assert!(!value["catalog"]["photos"][id.0.to_string()].as_object().unwrap().contains_key("companion"));
    let saved: Value = serde_json::from_slice(&files.get(STATE_FILE).unwrap()).unwrap();
    assert_eq!(saved["version"], 1);
    s.close_library().unwrap();
    let mut s = open(catalog.clone(), Box::new(files.clone()));
    assert_eq!(s.photo_companion(id).unwrap(), Some(&conversation));
    s.execute("develop.set", &json!({"ids":[id.0],"values":{"light.exposure":0.5}})).unwrap();
    assert_eq!(s.photo_companion(id).unwrap().unwrap()["checkpoint"]["eligible"], false);
    s.close_library().unwrap();
    let s = open(catalog, Box::new(files));
    assert_eq!(s.photo_companion(id).unwrap().unwrap()["checkpoint"]["eligible"], false);
    assert_eq!(s.photo_companion(other).unwrap(), Some(&other_value));
    let mut s = s;
    s.set_photo_companion(id, None).unwrap();
    s.close_library().unwrap();
    assert_eq!(s.photo_companion(other).unwrap(), Some(&other_value));
    assert_eq!(s.library.as_mut().unwrap().read_companion_attachment(&attachment).unwrap(), b"other preview");
}

fn legacy_record(seq: u64, op: &Value) -> String {
    let body = serde_json::to_string(op).unwrap();
    format!("{{\"seq\":{seq},\"crc\":{},\"op\":{body}}}\n", crc32fast::hash(body.as_bytes()))
}

#[test]
fn old_ai_records_are_discarded_while_photo_operations_and_sequences_survive() {
    let (mut catalog, files) = (MemStore::new(), MemStore::new());
    let mut initial = open(catalog.clone(), Box::new(files.clone()));
    let id = initial.catalog.photos().next().unwrap().id;
    initial.close_library().unwrap();
    let snap: Value = serde_json::from_slice(&catalog.get(SNAPSHOT).unwrap()).unwrap();
    let seq = snap["seq"].as_u64().unwrap();
    let legacy = json!({"op":"setPhotoCompanion","id":id.0,"conversation":{"messages":[{"text":"discard old AI"}]}});
    let rating = Op::SetRating { id, rating: 4 };
    catalog.set(LOG, format!("{}{}\n", legacy_record(seq + 1, &legacy), encode_record(seq + 2, &rating)).into_bytes());
    let s = open(catalog.clone(), Box::new(files.clone()));
    assert_eq!(s.catalog.photo(id).unwrap().rating, 4);
    assert!(s.photo_companion(id).unwrap().is_none());
    assert_eq!(s.library.as_ref().unwrap().journal().seq(), seq + 2);
    let cleaned = catalog.get(LOG).unwrap();
    discard_legacy_ops(&mut catalog).unwrap();
    assert_eq!(catalog.get(LOG).unwrap(), cleaned, "cleanup is idempotent after interruption");
    catalog.set(LOG, format!("{}{}", legacy_record(seq + 1, &legacy), legacy_record(seq + 2, &json!({"op":"futureOp"}))).into_bytes());
    let before = catalog.get(LOG).unwrap();
    discard_legacy_ops(&mut catalog).unwrap();
    assert_eq!(catalog.get(LOG).unwrap(), before, "future data is refused untouched");
}

#[test]
fn newer_or_corrupt_ai_storage_does_not_block_photos_and_is_never_overwritten() {
    for bytes in [b"not json".to_vec(), br#"{"format":"lightcraft-companion","version":2,"photos":{}}"#.to_vec()] {
        let files = MemStore::new();
        files.set(STATE_FILE, bytes.clone());
        let mut s = open(MemStore::new(), Box::new(files.clone()));
        let id = s.catalog.photos().next().unwrap().id;
        s.execute("develop.set", &json!({"ids":[id.0],"values":{"light.exposure":0.5}})).unwrap();
        assert!(s.set_photo_companion(id, Some(json!({}))).is_err());
        assert_eq!(files.get(STATE_FILE).unwrap(), bytes);
    }
}

#[test]
fn failed_archive_write_keeps_the_change_and_retries_atomically() {
    struct Fault {
        memory: MemStore,
        fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    impl Store for Fault {
        fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
            self.memory.read(name)
        }
        fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            if name == STATE_FILE && self.fail.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(std::io::Error::other("injected archive failure"));
            }
            self.memory.write_atomic(name, bytes)
        }
        fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            self.memory.append(name, bytes)
        }
        fn truncate(&mut self, name: &str, len: u64) -> std::io::Result<()> {
            self.memory.truncate(name, len)
        }
        fn describe(&self) -> String {
            "fault".into()
        }
    }
    let files = MemStore::new();
    let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut s = open(MemStore::new(), Box::new(Fault { memory: files.clone(), fail: fail.clone() }));
    let id = s.catalog.photos().next().unwrap().id;
    s.set_photo_companion(id, Some(json!({"messages":["saved"]}))).unwrap();
    let before = files.get(STATE_FILE).unwrap();
    fail.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(s.set_photo_companion(id, Some(json!({"messages":["queued"]}))).is_err());
    assert_eq!(files.get(STATE_FILE).unwrap(), before);
    assert!(s.unsaved().unwrap().1.contains("injected archive failure"));
    let identity = s.library_identity.clone();
    assert!(s.close_library().is_err(), "a successful catalog snapshot cannot hide an unsaved AI archive");
    assert!(std::sync::Arc::ptr_eq(&identity, &s.library_identity));
    fail.store(false, std::sync::atomic::Ordering::Relaxed);
    s.persist().unwrap();
    assert!(s.unsaved().is_none());
    let saved: Value = serde_json::from_slice(&files.get(STATE_FILE).unwrap()).unwrap();
    assert_eq!(saved["photos"][id.0.to_string()]["messages"][0], "queued");
}
