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
    finish_migration(&mut s);
    assert_eq!(s.companion_messages_page(id, 0, 1).unwrap().records, conversation["messages"].as_array().unwrap().clone());
    s.execute("develop.set", &json!({"ids":[id.0],"values":{"light.exposure":0.5}})).unwrap();
    assert_ne!(s.load_companion_conversation(id).unwrap().unwrap()["checkpoint"]["eligible"], true);
    s.close_library().unwrap();
    let mut s = open(catalog, Box::new(files));
    assert_ne!(s.load_companion_conversation(id).unwrap().unwrap()["checkpoint"]["eligible"], true);
    assert_eq!(s.companion_messages_page(other, 0, 1).unwrap().records, other_value["messages"].as_array().unwrap().clone());
    s.set_photo_companion(id, None).unwrap();
    s.close_library().unwrap();
    assert_eq!(s.companion_messages_page(other, 0, 1).unwrap().records, other_value["messages"].as_array().unwrap().clone());
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
        fn source_snapshot(&mut self, name: &str) -> std::io::Result<Option<Box<dyn lightcraft_catalog::SourceSnapshot>>> {
            self.memory.source_snapshot(name)
        }
        fn replace_source_atomic(&mut self, name: &str, expected: &lightcraft_catalog::SourceIdentity, bytes: &[u8]) -> std::io::Result<()> {
            if name == STATE_FILE && self.fail.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(std::io::Error::other("injected archive failure"));
            }
            self.memory.replace_source_atomic(name, expected, bytes)
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
        fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
            self.0.read_range(name, offset, length)
        }
        fn remove(&mut self, name: &str) -> std::io::Result<()> {
            self.0.remove(name)
        }
        fn retention_namespace(&self) -> Option<String> {
            self.0.retention_namespace()
        }
        fn source_snapshot(&mut self, name: &str) -> std::io::Result<Option<Box<dyn lightcraft_catalog::SourceSnapshot>>> {
            self.0.source_snapshot(name)
        }
        fn copy_source_atomic(&mut self, name: &str, source: &mut dyn lightcraft_catalog::SourceSnapshot) -> std::io::Result<()> {
            self.0.copy_source_atomic(name, source)
        }
        fn replace_source_atomic(&mut self, name: &str, expected: &lightcraft_catalog::SourceIdentity, bytes: &[u8]) -> std::io::Result<()> {
            self.0.replace_source_atomic(name, expected, bytes)
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
    finish_migration(&mut session);
    assert_eq!(session.companion_messages_page(id, 0, 1).unwrap().records, conversation["messages"].as_array().unwrap().clone());
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
    finish_migration(&mut session);
    assert_eq!(session.companion_messages_page(id, 0, 1).unwrap().records, value["messages"].as_array().unwrap().clone());
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

fn finish_migration(session: &mut Session) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while session.companion_migration_pending() && std::time::Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!session.companion_migration_pending(), "migration did not complete: {:?}", session.companion_migration_error());
    session.persist().unwrap();
}

#[test]
fn opening_legacy_history_migrates_with_exact_backup_and_no_revert_eligibility() {
    let catalog = MemStore::new();
    let files = MemStore::new();
    let mut initial = open(catalog.clone(), Box::new(files.clone()));
    let photo = initial.catalog.photos().next().unwrap().id;
    initial.close_library().unwrap();
    let source = format!(r#"{{"format":"lightcraft-companion","version":1,"photos":{{"{}":{{"schema_version":1,"messages":[{{"role":"user","text":"original retained question"}}],"rounds":[{{"plan_id":"original-plan","operations":[]}}],"checkpoint":{{"eligible":true}}}}}},"additive":{{"keep":"outer metadata"}}}}"#, photo.0).into_bytes();
    files.set(STATE_FILE, source.clone());
    let mut session = open(catalog.clone(), Box::new(files.clone()));
    finish_migration(&mut session);
    let root: Value = serde_json::from_slice(&files.get(STATE_FILE).unwrap()).unwrap();
    assert_eq!(root["version"], 2, "ordinary maintenance must complete legacy migration");
    assert_eq!(files.get("companion/state.v1.backup.json"), Some(source));
    assert_eq!(session.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "original retained question");
    assert_eq!(session.companion_rounds_page(photo, 0, 1).unwrap().records[0]["plan_id"], "original-plan");
    assert_eq!(session.load_companion_conversation(photo).unwrap().unwrap()["checkpoint"]["eligible"], false);
    session.close_library().unwrap();
    let mut reopened = open(catalog, Box::new(files));
    assert_eq!(reopened.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "original retained question");
}

#[test]
fn huge_legacy_message_and_additive_metadata_remain_readable_without_root_materialization() {
    use lightcraft_catalog::FsStore;
    use std::io::Read;
    let directory = std::env::temp_dir().join(format!("lc-ux10-huge-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let catalog = MemStore::new();
    let mut files = FsStore::open(&directory).unwrap();
    let mut initial = open(catalog.clone(), Box::new(FsStore::open(&directory).unwrap()));
    let photo = initial.catalog.photos().next().unwrap().id;
    initial.close_library().unwrap();
    files.write_atomic_with(STATE_FILE,&mut |w| {
        write!(w,r#"{{"photos":{{"{}":{{"schema_version":1,"messages":[{{"role":"user","text":"HEAD readable "# ,photo.0)?;
        let bytes = vec![b'x';256*1024];
        for _ in 0..260 { w.write_all(&bytes)?; }
        w.write_all(br#" TAIL readable"},{"role":"assistant","text":"next complete answer"}],"rounds":[{"plan_id":"known-plan","operations":[]}],"unknown_metadata":""#)?;
        for _ in 0..8 { w.write_all(&bytes)?; }
        w.write_all(br#"","checkpoint":null}},"unknown_outer":{"retained":true},"version":1,"format":"lightcraft-companion"}"#)?;
        Ok(())
    }).unwrap();
    let mut decoder = serde_json::Deserializer::from_reader(std::io::BufReader::new(std::fs::File::open(directory.join(STATE_FILE)).unwrap()));
    serde::Deserialize::deserialize(&mut decoder).map(|_: serde::de::IgnoredAny| ()).unwrap();
    decoder.end().unwrap();
    let expected_path = directory.join("expected-legacy-source.json");
    std::fs::copy(directory.join(STATE_FILE), &expected_path).unwrap();
    let mut session = open(catalog, Box::new(FsStore::open(&directory).unwrap()));
    finish_migration(&mut session);
    let first = session.companion_history_slice(photo, super::super::storage::HistoryCursor::default(), 64 * 1024);
    assert!(first.is_ok(), "huge retained history must have bounded reading access: {first:?}");
    let first = first.unwrap();
    assert!(first.fragments.first().unwrap().starts_with("HEAD readable "));
    let offset = br#"{"role":"user","text":"HEAD readable "#.len() as u64 + 260 * 256 * 1024 - 16;
    let tail = session.companion_history_slice(photo, super::super::storage::HistoryCursor { record: 0, offset }, 64 * 1024).unwrap();
    assert!(tail.fragments.first().unwrap().ends_with("TAIL readable"));
    assert_eq!(session.companion_messages_page(photo, 1, 1).unwrap().records[0]["text"], "next complete answer");
    assert_eq!(session.companion_rounds_page(photo, 0, 1).unwrap().records[0]["plan_id"], "known-plan");
    let root = files.read_limited(STATE_FILE, 32 * 1024).unwrap().unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&root).unwrap()["version"], 2);
    let mut backup = std::fs::File::open(directory.join("companion/state.v1.backup.json")).unwrap();
    assert!(backup.metadata().unwrap().len() > 64 * 1024 * 1024);
    let mut head = [0; 64];
    backup.read_exact(&mut head).unwrap();
    assert!(std::str::from_utf8(&head).unwrap().starts_with("{\"photos\""));
    let mut expected = std::fs::File::open(expected_path).unwrap();
    let mut actual = std::fs::File::open(directory.join("companion/state.v1.backup.json")).unwrap();
    let mut left = [0; 32 * 1024];
    let mut right = [0; 32 * 1024];
    loop {
        let count = expected.read(&mut left).unwrap();
        if count == 0 {
            assert_eq!(actual.read(&mut right).unwrap(), 0);
            break;
        }
        actual.read_exact(&mut right[..count]).unwrap();
        assert_eq!(left[..count], right[..count]);
    }
    session.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn migration_pauses_ai_and_legacy_writers_while_latest_draft_and_photo_edits_survive() {
    use super::super::{EditorState, LocalRoundId};
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut initial = open(catalog.clone(), Box::new(files.clone()));
    let photo = initial.catalog.photos().next().unwrap().id;
    initial.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"exact original"}],"rounds":[],"checkpoint":{"eligible":true}}}})).unwrap();
    files.set(STATE_FILE, source.clone());
    let mut session = open(catalog.clone(), Box::new(files.clone()));
    assert!(
        session.acquire_companion_lease(photo, LocalRoundId::new().unwrap()).is_err(),
        "migration must gate engine AI admission before task ownership"
    );
    session.set_companion_editor(photo, EditorState { draft: "first unsent draft".into(), ..Default::default() }).unwrap();
    session.execute("develop.set", &json!({"ids":[photo.0],"values":{"light.exposure":0.75}})).unwrap();
    assert!(session.set_photo_companion(photo, None).is_err(), "delete must not alter frozen legacy source");
    assert!(session.save_companion_conversation(photo, json!({"messages":[],"rounds":[]})).is_err(), "old writers must be gated at engine seam");
    assert_eq!(files.get(STATE_FILE), Some(source.clone()));
    let latest = EditorState { draft: "latest unsent 草稿".into(), scroll_offset: 54.0, follow_tail: false, ..Default::default() };
    session.set_companion_editor(photo, latest.clone()).unwrap();
    finish_migration(&mut session);
    assert_eq!(session.companion_editor(photo).unwrap(), latest);
    assert!(session.companion_dirty_editors().is_empty());
    assert_eq!(files.get("companion/state.v1.backup.json"), Some(source));
    assert_eq!(session.load_companion_conversation(photo).unwrap().unwrap()["messages"][0]["text"], "exact original");
    assert_eq!(session.load_companion_conversation(photo).unwrap().unwrap()["checkpoint"]["eligible"], false);
    assert_eq!(session.develop_of(photo).unwrap().light.exposure, 0.75);
    session.close_library().unwrap();
    let mut reopened = open(catalog, Box::new(files));
    assert_eq!(reopened.companion_editor(photo).unwrap(), latest);
    assert_eq!(reopened.develop_of(photo).unwrap().light.exposure, 0.75);
}

struct MigrationFault {
    native: lightcraft_catalog::FsStore,
    fault: std::sync::Arc<std::sync::Mutex<Option<(String, bool)>>>,
}
impl MigrationFault {
    fn failed(&mut self, name: &str, visible: bool) -> std::io::Result<()> {
        let mut fault = self.fault.lock().unwrap();
        if fault.as_ref().is_some_and(|(pattern, after)| name.contains(pattern) && *after == visible) {
            fault.take();
            return Err(std::io::Error::other("migration filesystem fault"));
        }
        Ok(())
    }
}
impl Store for MigrationFault {
    fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.native.read(name)
    }
    fn read_limited(&mut self, name: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.native.read_limited(name, max)
    }
    fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.native.read_range(name, offset, length)
    }
    fn write_atomic(&mut self, name: &str, data: &[u8]) -> std::io::Result<()> {
        self.failed(name, false)?;
        self.native.write_atomic(name, data)?;
        self.failed(name, true)
    }
    fn remove(&mut self, name: &str) -> std::io::Result<()> {
        let operation = format!("remove:{}", name.rsplit('/').next().unwrap());
        self.failed(&operation, false)?;
        self.native.remove(name)?;
        self.failed(&operation, true)
    }
    fn append(&mut self, name: &str, data: &[u8]) -> std::io::Result<()> {
        self.native.append(name, data)
    }
    fn truncate(&mut self, name: &str, length: u64) -> std::io::Result<()> {
        self.native.truncate(name, length)
    }
    fn describe(&self) -> String {
        self.native.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.native.retention_namespace()
    }
    fn source_snapshot(&mut self, name: &str) -> std::io::Result<Option<Box<dyn lightcraft_catalog::SourceSnapshot>>> {
        self.native.source_snapshot(name)
    }
    fn copy_source_atomic(&mut self, name: &str, source: &mut dyn lightcraft_catalog::SourceSnapshot) -> std::io::Result<()> {
        self.failed(name, false)?;
        self.native.copy_source_atomic(name, source)?;
        self.failed(name, true)
    }
    fn replace_source_atomic(&mut self, name: &str, expected: &lightcraft_catalog::SourceIdentity, data: &[u8]) -> std::io::Result<()> {
        self.failed(name, false)?;
        self.native.replace_source_atomic(name, expected, data)?;
        self.failed(name, true)
    }
}
#[test]
fn interrupted_native_backup_keeps_legacy_authority_and_restart_resumes_same_graph() {
    use lightcraft_catalog::FsStore;
    let directory = std::env::temp_dir().join(format!("lc-ux10-interrupted-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = open(catalog.clone(), Box::new(FsStore::open(&directory).unwrap()));
    let photo = initial.catalog.photos().next().unwrap().id;
    initial.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"legacy authority survives fault"}],"rounds":[]}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic(STATE_FILE, &source).unwrap();
    let fault = std::sync::Arc::new(std::sync::Mutex::new(Some(("state.v1.backup.json".into(), false))));
    let mut session = open(catalog.clone(), Box::new(MigrationFault { native: FsStore::open(&directory).unwrap(), fault: fault.clone() }));
    for _ in 0..20 {
        if session.companion_migration_error().is_some() {
            break;
        }
        let _ = session.persist();
    }
    assert!(session.companion_migration_error().is_some(), "pending readiness alone is not a physical backup failure");
    assert_eq!(files.read_limited(STATE_FILE, 32 * 1024).unwrap(), Some(source.clone()));
    assert_eq!(session.load_companion_conversation(photo).unwrap().unwrap()["messages"][0]["text"], "legacy authority survives fault");
    assert!(session.companion_photo_header(photo).unwrap().is_none(), "unactivated migration HEAD must not enter the v2 selector");
    let staged = files.read_limited(&format!("companion/v2/photos/{}/HEAD.json", photo.0), 32 * 1024).unwrap().unwrap();
    drop(session);
    let mut reopened = open(catalog, Box::new(MigrationFault { native: FsStore::open(&directory).unwrap(), fault }));
    finish_migration(&mut reopened);
    assert_eq!(
        files.read_limited(&format!("companion/v2/photos/{}/HEAD.json", photo.0), 32 * 1024).unwrap().unwrap(),
        staged,
        "restart reuses the exact committed migration graph"
    );
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 32 * 1024).unwrap(), Some(source));
    assert_eq!(reopened.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "legacy authority survives fault");
    reopened.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn visible_pending_removal_error_reuses_migrated_transaction_after_restart() {
    use lightcraft_catalog::FsStore;
    let directory = std::env::temp_dir().join(format!("lc-ux10-remove-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = open(catalog.clone(), Box::new(FsStore::open(&directory).unwrap()));
    let photo = initial.catalog.photos().next().unwrap().id;
    initial.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"one original record"}],"rounds":[]}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic(STATE_FILE, &source).unwrap();
    let fault = std::sync::Arc::new(std::sync::Mutex::new(Some(("remove:pending.json".into(), true))));
    let session = open(catalog.clone(), Box::new(MigrationFault { native: FsStore::open(&directory).unwrap(), fault: fault.clone() }));
    assert!(session.companion_migration_error().is_some(), "real open maintenance must retain the injected cleanup failure");
    let staged = files.read_limited(&format!("companion/v2/photos/{}/HEAD.json", photo.0), 32 * 1024).unwrap().unwrap();
    assert_eq!(files.read_limited(STATE_FILE, 32 * 1024).unwrap(), Some(source.clone()));
    drop(session);
    let mut reopened = open(catalog, Box::new(MigrationFault { native: FsStore::open(&directory).unwrap(), fault }));
    finish_migration(&mut reopened);
    assert_eq!(
        files.read_limited(&format!("companion/v2/photos/{}/HEAD.json", photo.0), 32 * 1024).unwrap().unwrap(),
        staged,
        "a completed source photo must keep its migration transaction after ambiguous cleanup"
    );
    assert_eq!(reopened.companion_messages_page(photo, 0, 2).unwrap().records.len(), 1);
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 32 * 1024).unwrap(), Some(source));
    reopened.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn ambiguous_outer_version_stays_protected_despite_known_last_value() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut initial = open(catalog.clone(), Box::new(files.clone()));
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let source = br#"{"format":"lightcraft-companion","version":99,"version":2,"layout":"photo-head-v2","photos":{}}"#.to_vec();
    files.set(STATE_FILE, source.clone());
    let mut session = open(catalog, Box::new(files.clone()));
    assert!(
        session.acquire_companion_lease(photo, crate::companion::LocalRoundId::new().unwrap()).is_err(),
        "duplicate outer version must not bypass protected-source admission"
    );
    session.execute("develop.set", &json!({"ids":[photo.0],"values":{"light.exposure":0.4}})).unwrap();
    session.persist().unwrap();
    assert_eq!(files.get(STATE_FILE), Some(source));
    assert!(files.get("companion/state.v1.backup.json").is_none());
    assert!(
        session.collect_companion(photo, 8).is_err(),
        "an unsupported source cannot authorize reclamation through a known last duplicate version"
    );
}

#[test]
fn deep_additive_legacy_metadata_has_no_parser_retention_quota() {
    use lightcraft_catalog::FsStore;
    let directory = std::env::temp_dir().join(format!("lc-ux10-deep-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = open(catalog.clone(), Box::new(FsStore::open(&directory).unwrap()));
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let nested = format!("{}true{}", "[".repeat(300), "]".repeat(300));
    let source=format!(r#"{{"photos":{{"{}":{{"schema_version":1,"messages":[{{"role":"user","text":"deep metadata keeps this text","unknown":{},"padding":"{}"}},{{"role":"assistant","text":"following legal answer"}}],"rounds":[],"checkpoint":null,"deep":{}}}}},"version":1,"format":"lightcraft-companion"}}"#,photo.0,nested,"x".repeat(300*1024),nested).into_bytes();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic(STATE_FILE, &source).unwrap();
    let mut session = open(catalog, Box::new(FsStore::open(&directory).unwrap()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while session.companion_migration_pending() && std::time::Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!session.companion_migration_pending(), "valid deep additive JSON must migrate: {:?}", session.companion_migration_error());
    let page = session.companion_history_slice(photo, super::super::storage::HistoryCursor::default(), 64 * 1024).unwrap();
    assert!(page.fragments.first().is_some_and(|text| text.starts_with("deep metadata keeps this text")));
    assert_eq!(session.companion_messages_page(photo, 1, 1).unwrap().records[0]["text"], "following legal answer");
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 4 * 1024 * 1024).unwrap(), Some(source));
    session.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn stale_legacy_writer_cannot_replace_an_activated_conversation() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut old = open(catalog.clone(), Box::new(files.clone()));
    let photo = old.active().unwrap();
    old.set_photo_companion(
        photo,
        Some(json!({"schema_version":1,"messages":[{"role":"user","text":"retained before activation"}],"rounds":[],"checkpoint":null})),
    )
    .unwrap();
    let mut owner = open(catalog, Box::new(files.clone()));
    finish_migration(&mut owner);
    let activated = files.get(STATE_FILE).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&activated).unwrap()["version"], 2);
    let _ = old.execute("develop.set", &json!({"ids":[photo.0],"values":{"light.exposure":0.3}}));
    assert_eq!(old.develop_of(photo).unwrap().light.exposure, 0.3);
    assert!(old.set_photo_companion(photo, None).is_err(), "an old v1 writer must reject the actual activated root");
    assert_eq!(files.get(STATE_FILE), Some(activated), "old invalidate/persist/setter routes must retain v2 authority");
}

#[test]
fn editor_save_preserves_the_verified_legacy_backup_descriptor() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut initial = open(catalog.clone(), Box::new(files.clone()));
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"complete recoverable source"}],"rounds":[],"checkpoint":null}},"unknown_outer":{"preserved":"in the exact backup"}})).unwrap();
    files.set(STATE_FILE, source.clone());
    let mut session = open(catalog, Box::new(files.clone()));
    finish_migration(&mut session);
    let before: Value = serde_json::from_slice(&files.get(STATE_FILE).unwrap()).unwrap();
    assert_eq!(before["legacy_backup"]["path"], "companion/state.v1.backup.json");
    assert_eq!(before["legacy_backup"]["length"], source.len());
    session.set_companion_editor(photo, crate::companion::EditorState { draft: "latest independent 草稿".into(), ..Default::default() }).unwrap();
    session.persist().unwrap();
    let after: Value = serde_json::from_slice(&files.get(STATE_FILE).unwrap()).unwrap();
    assert_eq!(after["legacy_backup"], before["legacy_backup"], "ordinary editor/root activation must retain the verified recoverability descriptor");
    assert_eq!(files.get("companion/state.v1.backup.json"), Some(source));
    assert_eq!(session.companion_editor(photo).unwrap().draft, "latest independent 草稿");
}

#[test]
fn small_deep_additive_message_migrates_and_keeps_selected_text() {
    let catalog = MemStore::new();
    let files = MemStore::new();
    let mut initial = open(catalog.clone(), Box::new(files.clone()));
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let nested = format!("{}0{}", "[".repeat(300), "]".repeat(300));
    let source=format!(r#"{{"format":"lightcraft-companion","version":1,"photos":{{"{}":{{"schema_version":1,"messages":[{{"role":"user","text":"Small original text","additive":{}}}],"rounds":[]}}}}}}"#,photo.0,nested).into_bytes();
    let mut decoder = serde_json::Deserializer::from_slice(&source);
    serde::Deserialize::deserialize(&mut decoder).map(|_: serde::de::IgnoredAny| ()).unwrap();
    decoder.end().unwrap();
    files.set(STATE_FILE, source.clone());
    let mut session = open(catalog, Box::new(files.clone()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while session.companion_migration_pending() && session.companion_migration_error().is_none() && std::time::Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!session.companion_migration_pending(), "small additive nesting must not prevent migration: {:?}", session.companion_migration_error());
    let header = session.companion_photo_header(photo).unwrap().unwrap();
    let mut scan = crate::companion::storage::DisplayScan::new(header, crate::companion::storage::DisplayRecord::Message(0));
    let value = loop {
        if let Some(value) = session.companion_display_step(&mut scan).unwrap() {
            break value;
        }
    };
    assert_eq!(value["text"], "Small original text");
    assert_eq!(files.get("companion/state.v1.backup.json"), Some(source));
}

#[test]
fn legacy_attachment_durability_failure_keeps_source_and_restart_preserves_valid_png() {
    use lightcraft_catalog::FsStore;
    let directory = std::env::temp_dir().join(format!("lc-ux10-preview-proof-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = open(catalog.clone(), Box::new(FsStore::open(&directory).unwrap()));
    let photo = initial.active().unwrap();
    let png = crate::export::encode_image(
        &lightcraft_raster::Rgba8::from_fn(32, 32, |_, _| [24, 196, 212, 255]),
        &crate::export::ExportOptions { format: crate::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    let attachment = initial.library.as_mut().unwrap().companion_attachment(photo, &png).unwrap();
    initial.close_library().unwrap();
    let image = json!({"mime_type":"image/png","attachment":attachment});
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"Original durable preview","images":[image]}],"rounds":[]}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic(STATE_FILE, &source).unwrap();
    let fault = std::sync::Arc::new(std::sync::Mutex::new(Some((attachment.clone(), false))));
    let mut session = open(catalog.clone(), Box::new(MigrationFault { native: FsStore::open(&directory).unwrap(), fault: fault.clone() }));
    for _ in 0..12 {
        if session.companion_migration_error().is_some() || session.persist().is_err() {
            break;
        }
    }
    assert!(session.companion_migration_error().is_some(), "attachment durability must be proved before replacing the source root");
    assert_eq!(files.read_limited(STATE_FILE, 32 * 1024).unwrap(), Some(source.clone()));
    assert_eq!(session.read_companion_historical_preview(photo, &image).unwrap(), png);
    drop(session);
    let mut reopened = open(catalog, Box::new(MigrationFault { native: FsStore::open(&directory).unwrap(), fault }));
    finish_migration(&mut reopened);
    assert_eq!(reopened.companion_messages_page(photo, 0, 1).unwrap().records[0]["images"][0], image);
    assert_eq!(reopened.read_companion_historical_preview(photo, &image).unwrap(), png);
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 32 * 1024).unwrap(), Some(source));
    reopened.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn visible_activation_error_keeps_legacy_reader_and_restart_reconfirms_durability() {
    use lightcraft_catalog::FsStore;
    let directory = std::env::temp_dir().join(format!("lc-ux10-activation-proof-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = open(catalog.clone(), Box::new(FsStore::open(&directory).unwrap()));
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"Readable original after visible activation error"}],"rounds":[{"plan_id":"original-activation-plan","operations":[]}]}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic(STATE_FILE, &source).unwrap();
    let fault = std::sync::Arc::new(std::sync::Mutex::new(Some((STATE_FILE.into(), true))));
    let mut session = open(catalog.clone(), Box::new(MigrationFault { native: FsStore::open(&directory).unwrap(), fault }));
    for _ in 0..20 {
        if session.companion_migration_error().is_some() {
            break;
        }
        let _ = session.persist();
    }
    assert!(session.companion_migration_error().is_some());
    let root: Value = serde_json::from_slice(&files.read_limited(STATE_FILE, 32 * 1024).unwrap().unwrap()).unwrap();
    assert_eq!(root["version"], 2);
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 32 * 1024).unwrap(), Some(source.clone()));
    let legacy =
        session.companion_legacy_photo_header(photo).unwrap().expect("visible but unconfirmed activation must retain original read-only graph");
    assert!(
        session.companion_history_slice_at(&legacy, crate::companion::storage::HistoryCursor::default(), 1024).unwrap().records[0]["text"]
            .as_str()
            .unwrap()
            .contains("Readable original")
    );
    drop(session);
    let fault = std::sync::Arc::new(std::sync::Mutex::new(Some((STATE_FILE.into(), false))));
    let mut reopened = open(catalog.clone(), Box::new(MigrationFault { native: FsStore::open(&directory).unwrap(), fault }));
    for _ in 0..20 {
        if reopened.companion_migration_error().is_some() {
            break;
        }
        let _ = reopened.persist();
    }
    assert!(reopened.companion_migration_pending(), "restart must keep unconfirmed activation debt rather than admit new writers");
    assert!(reopened.companion_migration_error().is_some(), "restart must perform and report the failed durable root repetition");
    assert!(reopened.acquire_companion_lease(photo, crate::companion::LocalRoundId::new().unwrap()).is_err());
    reopened.retry_companion_migration().unwrap();
    finish_migration(&mut reopened);
    assert_eq!(reopened.companion_rounds_page(photo, 0, 1).unwrap().records[0]["plan_id"], "original-activation-plan");
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 32 * 1024).unwrap(), Some(source));
    reopened.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn additive_image_metadata_cannot_skip_original_attachment_durability() {
    use lightcraft_catalog::FsStore;
    let directory = std::env::temp_dir().join(format!("lc-ux10-additive-preview-proof-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = open(catalog.clone(), Box::new(FsStore::open(&directory).unwrap()));
    let photo = initial.active().unwrap();
    let png = crate::export::encode_image(
        &lightcraft_raster::Rgba8::from_fn(32, 32, |_, _| [24, 196, 212, 255]),
        &crate::export::ExportOptions { format: crate::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    let attachment = initial.library.as_mut().unwrap().companion_attachment(photo, &png).unwrap();
    initial.close_library().unwrap();
    let image = json!({"mime_type":"image/png","attachment":attachment});
    let encoded=serde_json::to_string(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"Original image with additive metadata","images":[image]}],"rounds":[]}}})).unwrap();
    let nested = format!("{}true{}", "[".repeat(300), "]".repeat(300));
    let source = encoded.replace("\"mime_type\":\"image/png\"", &format!("\"mime_type\":\"image/png\",\"additive\":{}", nested)).into_bytes();
    let mut decoder = serde_json::Deserializer::from_slice(&source);
    serde::Deserialize::deserialize(&mut decoder).map(|_: serde::de::IgnoredAny| ()).unwrap();
    decoder.end().unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic(STATE_FILE, &source).unwrap();
    let fault = std::sync::Arc::new(std::sync::Mutex::new(Some((attachment, false))));
    let mut session = open(catalog.clone(), Box::new(MigrationFault { native: FsStore::open(&directory).unwrap(), fault: fault.clone() }));
    for _ in 0..20 {
        if session.companion_migration_error().is_some() {
            break;
        }
        let _ = session.persist();
    }
    assert!(session.companion_migration_error().is_some(), "valid references must be durability-proved even with deep additive metadata");
    assert_eq!(files.read_limited(STATE_FILE, 32 * 1024).unwrap(), Some(source.clone()));
    assert_eq!(session.read_companion_historical_preview(photo, &image).unwrap(), png);
    drop(session);
    let mut reopened = open(catalog, Box::new(MigrationFault { native: FsStore::open(&directory).unwrap(), fault }));
    finish_migration(&mut reopened);
    assert_eq!(reopened.read_companion_historical_preview(photo, &image).unwrap(), png);
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 32 * 1024).unwrap(), Some(source));
    reopened.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn additive_v1_layout_before_version_does_not_block_complete_migration() {
    let catalog = MemStore::new();
    let files = MemStore::new();
    let mut initial = open(catalog.clone(), Box::new(files.clone()));
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let nested = format!("{}true{}", "[".repeat(300), "]".repeat(300));
    let source=format!(r#"{{"layout":{},"photos":{{"{}":{{"schema_version":1,"messages":[{{"role":"user","text":"Original with additive layout"}}],"rounds":[]}}}},"format":"lightcraft-companion","version":1}}"#,nested,photo.0).into_bytes();
    let mut decoder = serde_json::Deserializer::from_slice(&source);
    serde::Deserialize::deserialize(&mut decoder).map(|_: serde::de::IgnoredAny| ()).unwrap();
    decoder.end().unwrap();
    files.set(STATE_FILE, source.clone());
    let mut session = open(catalog, Box::new(files.clone()));
    finish_migration(&mut session);
    assert_eq!(session.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "Original with additive layout");
    assert_eq!(files.get("companion/state.v1.backup.json"), Some(source));
}

#[test]
fn migrated_small_messages_share_bounded_blocks_and_index_pages() {
    use lightcraft_catalog::FsStore;
    let directory = std::env::temp_dir().join(format!("lc-ux10-packed-source-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = open(catalog.clone(), Box::new(FsStore::open(&directory).unwrap()));
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let messages: Vec<Value> = (0..100).map(|i| json!({"role":"user","text":format!("Original message {i}")})).collect();
    let source = serde_json::to_vec(
        &json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":messages,"rounds":[]}}}),
    )
    .unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic(STATE_FILE, &source).unwrap();
    let mut session = open(catalog, Box::new(FsStore::open(&directory).unwrap()));
    finish_migration(&mut session);
    assert_eq!(session.companion_messages_page(photo, 0, 100).unwrap().records, messages);
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 32 * 1024).unwrap(), Some(source));
    let base = directory.join(format!("companion/v2/photos/{}", photo.0));
    assert!(std::fs::read_dir(base.join("blocks")).unwrap().count() <= 5, "small source records must share blocks rather than allocate a file each");
    assert!(std::fs::read_dir(base.join("pages")).unwrap().count() <= 4, "small source records must share bounded index pages");
    session.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn incomplete_or_unsupported_legacy_history_blocks_new_context_but_keeps_edits_and_drafts() {
    for legacy in [
        json!({"schema_version":99,"messages":[{"role":"user","text":"Known original text"}],"rounds":[{"plan_id":"known-readonly-plan","operations":[]}]}),
        json!({"schema_version":1,"messages":{},"rounds":[{"plan_id":"known-readonly-plan","operations":[]}]}),
        json!({"schema_version":1,"rounds":[{"plan_id":"known-readonly-plan","operations":[]}]}),
    ] {
        let catalog = MemStore::new();
        let files = MemStore::new();
        let mut initial = open(catalog.clone(), Box::new(files.clone()));
        let photo = initial.active().unwrap();
        initial.close_library().unwrap();
        let source = serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():legacy}})).unwrap();
        files.set(STATE_FILE, source.clone());
        let mut session = open(catalog.clone(), Box::new(files.clone()));
        finish_migration(&mut session);
        assert_eq!(session.companion_rounds_page(photo, 0, 1).unwrap().records[0]["plan_id"], "known-readonly-plan");
        if legacy["schema_version"] == 99 {
            assert_eq!(session.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "Known original text");
        }
        assert!(
            session.acquire_companion_lease(photo, crate::companion::LocalRoundId::new().unwrap()).is_err(),
            "unsupported/incomplete source cannot become a usable empty context"
        );
        session.execute("develop.set", &json!({"ids":[photo.0],"values":{"light.exposure":0.6}})).unwrap();
        session
            .set_companion_editor(photo, crate::companion::EditorState { draft: "Latest retained draft 草稿".into(), ..Default::default() })
            .unwrap();
        session.close_library().unwrap();
        let mut reopened = open(catalog, Box::new(files.clone()));
        assert_eq!(reopened.develop_of(photo).unwrap().light.exposure, 0.6);
        assert_eq!(reopened.companion_editor(photo).unwrap().draft, "Latest retained draft 草稿");
        assert!(reopened.acquire_companion_lease(photo, crate::companion::LocalRoundId::new().unwrap()).is_err());
        assert_eq!(files.get("companion/state.v1.backup.json"), Some(source));
    }
}

#[test]
fn native_migration_keeps_actual_library_lock_until_cancelled_producer_retires() {
    use lightcraft_catalog::{FsStore, LibraryLock};
    let root = std::env::temp_dir().join(format!("lc-ux10-real-lock-{}", crate::companion::LocalRoundId::new().unwrap().as_str()));
    let a = root.join("A");
    let b = root.join("B");
    let mut initial = Session::new().with_fs();
    initial.open_library(&a, true).unwrap();
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    drop(initial);
    let prefix = format!(
        r#"{{"format":"lightcraft-companion","version":1,"photos":{{"{}":{{"schema_version":1,"messages":[{{"role":"user","text":"Original locked source","additive":""#,
        photo.0
    );
    let mut files = FsStore::open(&a).unwrap();
    let source_length = files
        .write_atomic_with(STATE_FILE, &mut |writer| {
            writer.write_all(prefix.as_bytes())?;
            let buffer = vec![b'x'; 256 * 1024];
            for _ in 0..4096 {
                writer.write_all(&buffer)?;
            }
            writer.write_all(br#""}],"rounds":[]}}}"#)?;
            Ok(())
        })
        .unwrap();
    let mut decoder = serde_json::Deserializer::from_reader(std::io::BufReader::new(std::fs::File::open(a.join(STATE_FILE)).unwrap()));
    serde::Deserialize::deserialize(&mut decoder).map(|_: serde::de::IgnoredAny| ()).unwrap();
    decoder.end().unwrap();
    let mut session = Session::new().with_fs();
    session.open_library(&a, false).unwrap();
    assert!(session.companion_migration_busy());
    std::thread::sleep(std::time::Duration::from_millis(20));
    let retirement = session.cancel_companion_migration().unwrap();
    session.open_library(&b, true).unwrap();
    let b_photo = session.active().unwrap();
    session.execute("develop.set", &json!({"ids":[b_photo.0],"values":{"light.exposure":0.7}})).unwrap();
    let mut protected_live = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !retirement.retired() && std::time::Instant::now() < deadline {
        match LibraryLock::acquire(&a, "UX10 actual lock probe") {
            Err(_) => protected_live = true,
            Ok(lock) => drop(lock),
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(protected_live, "test must observe a real producer interval protected by the actual native library lock");
    assert!(retirement.retired(), "receipt is complete only after actual producer retirement");
    let reacquired = LibraryLock::acquire(&a, "UX10 retired lock probe").unwrap();
    assert!(reacquired.held());
    drop(reacquired);
    assert_eq!(std::fs::metadata(a.join(STATE_FILE)).unwrap().len(), source_length);
    assert_eq!(files.read_range(STATE_FILE, 0, prefix.len()).unwrap(), prefix.as_bytes());
    assert!(!a.join("companion/state.v1.backup.json").exists());
    assert_eq!(session.develop_of(b_photo).unwrap().light.exposure, 0.7);
    session.close_library().unwrap();
    drop(session);
    std::fs::remove_dir_all(root).unwrap();
}
