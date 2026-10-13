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
        s.close_library().unwrap();
        assert_eq!(files.get(STATE_FILE).unwrap(), bytes, "a protected AI root does not trap ordinary photos in the library");
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
        fn read_limited(&mut self, name: &str, max_bytes: u64) -> std::io::Result<Option<Vec<u8>>> {
            self.memory.read_limited(name, max_bytes)
        }
        fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
            self.memory.read_range(name, offset, length)
        }
        fn remove(&mut self, name: &str) -> std::io::Result<()> {
            if self.fail.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(std::io::Error::other("injected attachment deletion failure"));
            }
            self.memory.remove(name)
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
    let name = s.library.as_mut().unwrap().companion_attachment(id, b"preview").unwrap();
    assert_eq!(s.library.as_mut().unwrap().files.read_range(&name, 1, 3).unwrap(), b"rev");
    fail.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(s.set_photo_companion(id, Some(json!({"messages":["queued"]}))).is_err());
    assert_eq!(files.get(STATE_FILE).unwrap(), before);
    assert!(s.library.as_mut().unwrap().remove_companion_attachment(&name).is_err());
    assert_eq!(s.library.as_mut().unwrap().read_companion_attachment(&name).unwrap(), b"preview");
    assert!(s.unsaved().unwrap().1.contains("injected archive failure"));
    let identity = s.library_identity.clone();
    assert!(s.close_library().is_err(), "a successful catalog snapshot cannot hide an unsaved AI archive");
    assert!(std::sync::Arc::ptr_eq(&identity, &s.library_identity));
    fail.store(false, std::sync::atomic::Ordering::Relaxed);
    s.persist().unwrap();
    s.library.as_mut().unwrap().remove_companion_attachment(&name).unwrap();
    assert!(s.library.as_mut().unwrap().read_companion_attachment(&name).is_err());
    assert!(s.unsaved().is_none());
    let saved: Value = serde_json::from_slice(&files.get(STATE_FILE).unwrap()).unwrap();
    assert_eq!(saved["photos"][id.0.to_string()]["messages"][0], "queued");
}

#[test]
fn temporary_library_attachment_removal_survives_reopen_and_is_idempotent() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut session = open(catalog.clone(), Box::new(files.clone()));
    let id = session.catalog.photos().next().unwrap().id;
    let library = session.library.as_mut().unwrap();
    let name = library.companion_attachment(id, b"preview").unwrap();
    library.remove_companion_attachment(&name).unwrap();
    library.remove_companion_attachment(&name).unwrap();
    assert!(library.read_companion_attachment(&name).is_err(), "removed attachments must no longer hydrate in temporary libraries");
    assert!(files.get(&name).is_none());
    session.close_library().unwrap();
    let mut session = open(catalog, Box::new(files));
    assert!(session.library.as_mut().unwrap().read_companion_attachment(&name).is_err());
}

#[test]
fn companion_hydration_uses_only_the_bounded_store_capability() {
    struct BoundedOnly(MemStore);
    impl Store for BoundedOnly {
        fn read(&mut self, _: &str) -> std::io::Result<Option<Vec<u8>>> {
            Err(std::io::Error::other("unbounded reads forbidden"))
        }
        fn read_limited(&mut self, name: &str, limit: u64) -> std::io::Result<Option<Vec<u8>>> {
            self.0.read_limited(name, limit)
        }
        fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            self.0.write_atomic(name, bytes)
        }
        fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            self.0.append(name, bytes)
        }
        fn truncate(&mut self, name: &str, len: u64) -> std::io::Result<()> {
            self.0.truncate(name, len)
        }
        fn describe(&self) -> String {
            "bounded-only".into()
        }
    }
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut session = open(catalog.clone(), Box::new(files.clone()));
    let id = session.catalog.photos().next().unwrap().id;
    let conversation = json!({"messages":["saved"]});
    session.set_photo_companion(id, Some(conversation.clone())).unwrap();
    let name = session.library.as_mut().unwrap().companion_attachment(id, b"preview").unwrap();
    session.close_library().unwrap();
    let mut session = open(catalog, Box::new(BoundedOnly(files.clone())));
    assert_eq!(session.photo_companion(id).unwrap(), Some(&conversation));
    assert_eq!(session.library.as_mut().unwrap().read_companion_attachment(&name).unwrap(), b"preview");
    files.set(&name, vec![0; 128 * 1024 + 1]);
    assert!(session.library.as_mut().unwrap().read_companion_attachment(&name).unwrap_err().to_string().contains("read limit"));
}

#[test]
fn native_archive_and_attachments_save_reopen_and_delete_through_store() {
    let dir = std::env::temp_dir().join(format!("lc-ux02-archive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut session = Session::new();
    session.open_library(&dir, true).unwrap();
    let id = session.catalog.photos().next().unwrap().id;
    let before = std::fs::read(dir.join(SNAPSHOT)).unwrap();
    let value = json!({"messages":["native conversation"]});
    session.set_photo_companion(id, Some(value.clone())).unwrap();
    let name = session.library.as_mut().unwrap().companion_attachment(id, b"preview").unwrap();
    assert_eq!(std::fs::read(dir.join(SNAPSHOT)).unwrap(), before);
    session.close_library().unwrap();
    drop(session);
    let mut session = Session::new();
    session.open_library(&dir, false).unwrap();
    assert_eq!(session.photo_companion(id).unwrap(), Some(&value));
    let library = session.library.as_mut().unwrap();
    assert_eq!(library.read_companion_attachment(&name).unwrap(), b"preview");
    library.remove_companion_attachment(&name).unwrap();
    library.remove_companion_attachment(&name).unwrap();
    assert!(library.read_companion_attachment(&name).is_err());
    session.close_library().unwrap();
    drop(session);
    assert!(!dir.join(&name).exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fresh_v2_rounds_are_sharded_and_reopen_without_touching_catalog() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(catalog.clone(), Box::new(files.clone()));
    let id = s.catalog.photos().next().unwrap().id;
    let before = catalog.files.lock().unwrap().clone();
    let messages: Vec<Value> = (0..260).map(|n| json!({"local_message_id":format!("m-{n}"),"role":"user","text":format!("request {n}")})).collect();
    let value = json!({"schema_version":2,"messages":messages,"rounds":[{"local_round_id":"r-1","request_text":"explain","local_terminal":"completed"}],"draft":"saved draft"});
    s.save_companion_conversation(id, value.clone()).unwrap();
    assert_eq!(*catalog.files.lock().unwrap(), before);
    let root: Value = serde_json::from_slice(&files.get(STATE_FILE).unwrap()).unwrap();
    assert_eq!(root["version"], 2);
    assert!(root.get("photos").is_none());
    s.close_library().unwrap();
    let mut s = open(catalog, Box::new(files.clone()));
    assert_eq!(s.load_companion_conversation(id).unwrap(), Some(value));
    let page = s.companion_messages_page(id, 128, 64).unwrap();
    assert_eq!(page.records.len(), 64);
    assert_eq!(page.records[0]["text"], "request 128");
    assert!(files.files.lock().unwrap().iter().filter(|(k, _)| k.contains("manifest-")).all(|(_, v)| v.len() < 4096));
}

#[test]
fn v2_failed_photo_commit_preserves_old_head_memory_and_other_photo_then_retries() {
    struct Fault {
        memory: MemStore,
        fail_photo: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }
    impl Store for Fault {
        fn read(&mut self, key: &str) -> std::io::Result<Option<Vec<u8>>> {
            self.memory.read(key)
        }
        fn read_limited(&mut self, key: &str, n: u64) -> std::io::Result<Option<Vec<u8>>> {
            self.memory.read_limited(key, n)
        }
        fn read_range(&mut self, key: &str, o: u64, n: usize) -> std::io::Result<Vec<u8>> {
            self.memory.read_range(key, o, n)
        }
        fn remove(&mut self, key: &str) -> std::io::Result<()> {
            self.memory.remove(key)
        }
        fn write_atomic(&mut self, key: &str, b: &[u8]) -> std::io::Result<()> {
            let photo = self.fail_photo.load(std::sync::atomic::Ordering::Relaxed);
            if photo != 0 && key == format!("companion/v2/photos/{photo}/HEAD.json") {
                return Err(std::io::Error::other("photo HEAD failure"));
            }
            self.memory.write_atomic(key, b)
        }
        fn append(&mut self, key: &str, b: &[u8]) -> std::io::Result<()> {
            self.memory.append(key, b)
        }
        fn truncate(&mut self, key: &str, n: u64) -> std::io::Result<()> {
            self.memory.truncate(key, n)
        }
        fn describe(&self) -> String {
            "v2 fault".into()
        }
    }
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let fail = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut s = open(catalog.clone(), Box::new(Fault { memory: files.clone(), fail_photo: fail.clone() }));
    let ids: Vec<_> = s.catalog.photos().take(2).map(|p| p.id).collect();
    let id = ids[0];
    let other = ids[1];
    let saved = json!({"schema_version":2,"messages":[{"role":"user","text":"saved"}],"rounds":[]});
    s.save_companion_conversation(id, saved.clone()).unwrap();
    let old_head = files.get(&format!("companion/v2/photos/{}/HEAD.json", id.0)).unwrap();
    fail.store(id.0, std::sync::atomic::Ordering::Relaxed);
    let queued = json!({"schema_version":2,"messages":[{"role":"user","text":"actual result kept"}],"rounds":[]});
    assert!(s.save_companion_conversation(id, queued.clone()).is_err());
    assert_eq!(s.photo_companion(id).unwrap(), Some(&queued));
    assert_eq!(s.load_companion_conversation(id).unwrap(), Some(queued.clone()), "reading must not overwrite queued actual result with old HEAD");
    assert_eq!(files.get(&format!("companion/v2/photos/{}/HEAD.json", id.0)).unwrap(), old_head);
    s.save_companion_conversation(other, saved.clone()).unwrap();
    assert!(s.unsaved().is_some(), "other-photo success must not hide queued archive failure");
    let mut reopened = open(catalog, Box::new(files.clone()));
    assert_eq!(reopened.load_companion_conversation(id).unwrap(), Some(saved.clone()));
    assert_eq!(reopened.load_companion_conversation(other).unwrap(), Some(saved));
    fail.store(0, std::sync::atomic::Ordering::Relaxed);
    s.persist().unwrap();
    assert!(s.unsaved().is_none());
    assert_eq!(reopened.load_companion_conversation(id).unwrap(), Some(queued));
}

#[test]
fn v2_cold_edit_invalidates_separate_revert_state_without_opening_conversation() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(catalog.clone(), Box::new(files.clone()));
    let id = s.catalog.photos().next().unwrap().id;
    s.save_companion_conversation(id, json!({"schema_version":2,"messages":[],"rounds":[],"checkpoint":{"eligible":true}})).unwrap();
    s.close_library().unwrap();
    let mut s = open(catalog.clone(), Box::new(files.clone()));
    assert!(s.photo_companion(id).unwrap().is_none(), "v2 library open must not hydrate global history");
    s.execute("develop.set", &json!({"ids":[id.0],"values":{"light.exposure":0.6}})).unwrap();
    s.close_library().unwrap();
    let mut s = open(catalog, Box::new(files));
    assert_eq!(s.load_companion_conversation(id).unwrap().unwrap()["checkpoint"]["eligible"], false);
}

#[test]
fn v2_corrupt_staged_block_cannot_replace_committed_head() {
    let files = MemStore::new();
    let mut s = open(MemStore::new(), Box::new(files.clone()));
    let id = s.catalog.photos().next().unwrap().id;
    let old = json!({"schema_version":2,"messages":[{"role":"user","text":"old"}],"rounds":[]});
    s.save_companion_conversation(id, old.clone()).unwrap();
    let head_key = format!("companion/v2/photos/{}/HEAD.json", id.0);
    let head = files.get(&head_key).unwrap();
    let pending = s.companion_stage(id, 0, &json!({"schema_version":2,"messages":[{"role":"user","text":"new"}],"rounds":[]})).unwrap();
    let key = files.files.lock().unwrap().keys().find(|k| k.contains(&pending.transaction_id) && k.contains("blocks/")).cloned().unwrap();
    files.set(&key, b"corrupt block".to_vec());
    assert!(s.companion_commit(&pending).is_err());
    assert_eq!(files.get(&head_key).unwrap(), head);
    assert_eq!(s.load_companion_conversation(id).unwrap(), Some(old));
}

#[test]
fn v2_head_error_after_visible_rename_keeps_valid_result_and_retry_is_safe() {
    struct AfterRename {
        memory: MemStore,
        fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    impl Store for AfterRename {
        fn read(&mut self, k: &str) -> std::io::Result<Option<Vec<u8>>> {
            self.memory.read(k)
        }
        fn read_limited(&mut self, k: &str, n: u64) -> std::io::Result<Option<Vec<u8>>> {
            self.memory.read_limited(k, n)
        }
        fn read_range(&mut self, k: &str, o: u64, n: usize) -> std::io::Result<Vec<u8>> {
            self.memory.read_range(k, o, n)
        }
        fn remove(&mut self, k: &str) -> std::io::Result<()> {
            self.memory.remove(k)
        }
        fn write_atomic(&mut self, k: &str, b: &[u8]) -> std::io::Result<()> {
            self.memory.write_atomic(k, b)?;
            if k.ends_with("/HEAD.json") && self.fail.swap(false, std::sync::atomic::Ordering::Relaxed) {
                return Err(std::io::Error::other("sync failed after visible rename"));
            }
            Ok(())
        }
        fn append(&mut self, k: &str, b: &[u8]) -> std::io::Result<()> {
            self.memory.append(k, b)
        }
        fn truncate(&mut self, k: &str, n: u64) -> std::io::Result<()> {
            self.memory.truncate(k, n)
        }
        fn describe(&self) -> String {
            "after rename fault".into()
        }
    }
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut s = open(catalog.clone(), Box::new(AfterRename { memory: files.clone(), fail: fail.clone() }));
    let id = s.catalog.photos().next().unwrap().id;
    s.save_companion_conversation(id, json!({"schema_version":2,"messages":[],"rounds":[]})).unwrap();
    fail.store(true, std::sync::atomic::Ordering::Relaxed);
    let actual = json!({"schema_version":2,"messages":[{"role":"user","text":"complete actual result"}],"rounds":[]});
    assert!(s.save_companion_conversation(id, actual.clone()).is_err());
    assert_eq!(s.photo_companion(id).unwrap(), Some(&actual));
    let mut reopened = open(catalog, Box::new(files));
    assert_eq!(reopened.load_companion_conversation(id).unwrap(), Some(actual.clone()));
    let transaction = s.companion_pending(id).unwrap().unwrap().transaction_id;
    fail.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(s.persist().is_err(), "visible HEAD is not a durable confirmation after sync failed");
    assert_eq!(s.companion_pending(id).unwrap().unwrap().transaction_id, transaction);
    s.persist().unwrap();
    assert_eq!(s.companion_photo_header(id).unwrap().unwrap().transaction_id, transaction);
    assert!(s.unsaved().is_none());
    assert_eq!(reopened.load_companion_conversation(id).unwrap(), Some(actual));
}

#[test]
fn v2_large_record_splits_by_bytes_instead_of_imposing_a_local_history_quota() {
    let files = MemStore::new();
    let mut s = open(MemStore::new(), Box::new(files.clone()));
    let id = s.catalog.photos().next().unwrap().id;
    let value = json!({"schema_version":2,"messages":[{"role":"user","text":"x".repeat(6*1024*1024)}],"rounds":[]});
    s.save_companion_conversation(id, value.clone()).unwrap();
    assert!(files.files.lock().unwrap().iter().filter(|(k, _)| k.contains("blocks/")).all(|(_, b)| b.len() <= 4 * 1024 * 1024));
    assert_eq!(s.load_companion_conversation(id).unwrap(), Some(value));
}

#[test]
fn v2_tampered_pending_cannot_overwrite_an_old_committed_manifest() {
    let files = MemStore::new();
    let mut s = open(MemStore::new(), Box::new(files.clone()));
    let id = s.catalog.photos().next().unwrap().id;
    let old = json!({"schema_version":2,"messages":[{"role":"user","text":"protected old"}],"rounds":[]});
    s.save_companion_conversation(id, old.clone()).unwrap();
    let base = format!("companion/v2/photos/{}", id.0);
    let head: Value = serde_json::from_slice(&files.get(&format!("{base}/HEAD.json")).unwrap()).unwrap();
    s.companion_stage(id, 0, &json!({"schema_version":2,"messages":[],"rounds":[]})).unwrap();
    let mut pending: Value = serde_json::from_slice(&files.get(&format!("{base}/pending.json")).unwrap()).unwrap();
    pending["head"]["manifest"] = head["manifest"].clone();
    files.set(&format!("{base}/pending.json"), serde_json::to_vec(&pending).unwrap());
    assert!(s.companion_stage(id, 0, &json!({"schema_version":2,"messages":[],"rounds":[]})).is_err());
    assert_eq!(s.load_companion_conversation(id).unwrap(), Some(old));
}
