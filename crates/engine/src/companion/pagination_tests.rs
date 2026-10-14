use super::storage::*;
use crate::{Session, library::LibraryStores};
use lightcraft_catalog::MemStore;
use serde_json::json;

#[test]
fn long_history_append_preserves_pages_without_hydrating_history() {
    let files = MemStore::new();
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(files.clone()), dir: "memory".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let mut header = None;
    for page in 0..20u64 {
        let messages: Vec<_> =
            (0..128).map(|n| json!({"role":"user","text":format!("page {page} entry {n}"),"local_message_id":format!("{page}-{n}")})).collect();
        session.save_companion_tail(photo, header.as_ref(), &messages, &[], &[], &json!({"schema_version":2})).unwrap();
        header = session.companion_photo_header(photo).unwrap();
    }
    assert_eq!(header.as_ref().unwrap().message_count, 2560);
    assert!(session.photo_companion(photo).unwrap().is_none(), "paged save keeps no eager whole-conversation cache");
    for page in [0u64, 10, 19] {
        let result = session.companion_messages_page(photo, page * 128, 128).unwrap();
        assert_eq!(result.records.len(), 128);
        assert_eq!(result.records[0]["text"], format!("page {page} entry 0"));
    }
}

#[test]
fn oversized_record_has_bounded_readable_fragments_without_local_quota() {
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "memory".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let text = "详尽的历史记录".repeat(300_000);
    session.save_companion_tail(photo, None, &[json!({"role":"assistant","text":text})], &[], &[], &json!({"schema_version":2})).unwrap();
    let result = session.companion_history_slice(photo, HistoryCursor::default(), 32 * 1024).unwrap();
    assert!(result.bytes <= 32 * 1024);
    assert!(result.next.is_some());
    assert!(result.fragments[0].contains("详尽的历史记录"));
}

#[test]
fn native_multi_photo_history_exceeds_64mib_and_large_photo_pages_remain_independent() {
    let dir = std::env::temp_dir().join(format!("lightcraft-ux08-{}", super::LocalRoundId::new().unwrap().as_str()));
    let catalog = MemStore::new();
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(lightcraft_catalog::FsStore::open(&dir).unwrap()),
                dir: dir.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let ids: Vec<_> = session.catalog.photos().take(2).map(|p| p.id).collect();
    let text = "A".repeat(6 * 1024 * 1024);
    for photo in &ids {
        let mut header = None;
        for n in 0..6 {
            session
                .save_companion_tail(
                    *photo,
                    header.as_ref(),
                    &[json!({"role":"assistant","text":text,"ordinal":n})],
                    &[],
                    &[],
                    &json!({"schema_version":2}),
                )
                .unwrap();
            header = session.companion_photo_header(*photo).unwrap();
        }
    }
    let photo = ids[0];
    let header = session.companion_photo_header(photo).unwrap().unwrap();
    let messages: Vec<_> = (0..300).map(|n| json!({"role":"user","text":format!("native page {n}")})).collect();
    session.save_companion_tail(photo, Some(&header), &messages, &[], &[], &json!({"schema_version":2})).unwrap();
    assert!(session.library.as_mut().unwrap().files.read_limited("companion/state.json", 1024).unwrap().unwrap().len() < 1024);
    session.close_library().unwrap();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog),
                files: Box::new(lightcraft_catalog::FsStore::open(&dir).unwrap()),
                dir: dir.clone(),
                on_disk: false,
            },
            false,
        )
        .unwrap();
    assert_eq!(session.companion_photo_header(photo).unwrap().unwrap().message_count, 306);
    for cursor in [6, 134, 262] {
        let page = session.companion_messages_page(photo, cursor, 32).unwrap();
        assert_eq!(page.records[0]["text"], format!("native page {}", cursor - 6));
    }
    let last = session.companion_history_slice(photo, HistoryCursor { record: 5, offset: 6 * 1024 * 1024 - 512 }, 1024).unwrap();
    assert!(last.fragments[0].contains("text") || last.fragments[0].contains("AAAA"));
    // Damage A's HEAD; B remains writable without loading A or rewriting a global root.
    std::fs::write(dir.join(format!("companion/v2/photos/{}/HEAD.json", photo.0)), b"damaged").unwrap();
    let other = ids[1];
    let header = session.companion_photo_header(other).unwrap().unwrap();
    session
        .save_companion_tail(other, Some(&header), &[json!({"role":"user","text":"independent B"})], &[], &[], &json!({"schema_version":2}))
        .unwrap();
    assert!(session.companion_photo_header(photo).is_err());
    assert_eq!(session.companion_messages_page(other, 6, 1).unwrap().records[0]["text"], "independent B");
    // The visible graphs alone contain 12 distinct 6 MiB logical texts (>72 MiB).
    assert_eq!(session.companion_photo_header(other).unwrap().unwrap().message_count, 7);
    session.close_library().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn failed_tail_and_corrupt_other_photo_do_not_block_independent_saves_or_retry() {
    use lightcraft_catalog::Store;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    struct Fault {
        files: MemStore,
        fail: Arc<AtomicU64>,
    }
    impl Store for Fault {
        fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
            self.files.read(name)
        }
        fn read_limited(&mut self, name: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
            self.files.read_limited(name, max)
        }
        fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
            self.files.read_range(name, offset, length)
        }
        fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            if name == format!("companion/v2/photos/{}/HEAD.json", self.fail.load(Ordering::Relaxed)) {
                return Err(std::io::Error::other("injected HEAD failure"));
            }
            self.files.write_atomic(name, bytes)
        }
        fn remove(&mut self, name: &str) -> std::io::Result<()> {
            self.files.remove(name)
        }
        fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            self.files.append(name, bytes)
        }
        fn truncate(&mut self, name: &str, length: u64) -> std::io::Result<()> {
            self.files.truncate(name, length)
        }
        fn describe(&self) -> String {
            "tail fault".into()
        }
    }
    let files = MemStore::new();
    let fail = Arc::new(AtomicU64::new(u64::MAX));
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(Fault { files: files.clone(), fail: fail.clone() }),
                dir: "fault".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photos: Vec<_> = session.catalog.photos().take(2).map(|p| p.id).collect();
    let a = photos[0];
    let b = photos[1];
    let metadata = json!({"schema_version":2,"checkpoint":{"eligible":true}});
    fail.store(a.0, Ordering::Relaxed);
    assert!(session.save_companion_tail(a, None, &[json!({"text":"A actual unsaved"})], &[], &[], &metadata).is_err());
    let transaction = session.companion_pending(a).unwrap().unwrap().transaction_id;
    session.save_companion_tail(b, None, &[json!({"text":"B saved"})], &[], &[], &metadata).unwrap();
    assert_eq!(session.companion_messages_page(b, 0, 1).unwrap().records[0]["text"], "B saved");
    assert!(session.companion_archive.unsaved().is_some());
    assert!(matches!(session.execute("develop.set", &json!({"ids":[a.0],"values":{"light.exposure":0.5}})), Err(crate::EngineError::NotSaved(_))));
    assert_eq!(session.develop_of(a).unwrap().light.exposure, 0.5);
    fail.store(u64::MAX, Ordering::Relaxed);
    session.flush_companion().unwrap();
    assert_eq!(session.companion_photo_header(a).unwrap().unwrap().transaction_id, transaction, "retry reuses the reserved transaction");
    assert_eq!(session.companion_messages_page(a, 0, 1).unwrap().records[0]["text"], "A actual unsaved");
    assert_eq!(session.companion_metadata(a).unwrap().unwrap()["checkpoint"]["eligible"], false);
    assert!(session.companion_archive.tails.is_empty());
    assert!(session.companion_archive.unsaved().is_none());
}

#[test]
fn old_page_seek_uses_bounded_index_work_and_never_reads_a_whole_storage_file() {
    use lightcraft_catalog::Store;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    struct Probe {
        files: MemStore,
        measured: Arc<AtomicBool>,
        indexes: Arc<AtomicUsize>,
        largest: Arc<AtomicUsize>,
    }
    impl Store for Probe {
        fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
            assert!(!self.measured.load(Ordering::Relaxed), "unbounded read: {name}");
            self.files.read(name)
        }
        fn read_limited(&mut self, name: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
            if self.measured.load(Ordering::Relaxed) {
                if name.contains("/pages/") {
                    self.indexes.fetch_add(1, Ordering::Relaxed);
                }
                self.largest.fetch_max(max as usize, Ordering::Relaxed);
            }
            self.files.read_limited(name, max)
        }
        fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
            if self.measured.load(Ordering::Relaxed) {
                self.largest.fetch_max(length, Ordering::Relaxed);
            }
            self.files.read_range(name, offset, length)
        }
        fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            self.files.write_atomic(name, bytes)
        }
        fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            self.files.append(name, bytes)
        }
        fn truncate(&mut self, name: &str, length: u64) -> std::io::Result<()> {
            self.files.truncate(name, length)
        }
        fn remove(&mut self, name: &str) -> std::io::Result<()> {
            self.files.remove(name)
        }
        fn describe(&self) -> String {
            "index probe".into()
        }
    }
    let measured = Arc::new(AtomicBool::new(false));
    let indexes = Arc::new(AtomicUsize::new(0));
    let largest = Arc::new(AtomicUsize::new(0));
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(Probe { files: MemStore::new(), measured: measured.clone(), indexes: indexes.clone(), largest: largest.clone() }),
                dir: "probe".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let messages: Vec<_> = (0..10_000).map(|n| json!({"role":"user","text":format!("original record {n}")})).collect();
    session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
    measured.store(true, Ordering::Relaxed);
    let result = session.companion_messages_page(photo, 0, 128).unwrap();
    assert_eq!(result.records[127]["text"], "original record 127");
    assert!(indexes.load(Ordering::Relaxed) < 32, "seek revisited linear history: {}", indexes.load(Ordering::Relaxed));
    assert!(largest.load(Ordering::Relaxed) <= 4 * 1024 * 1024);
}

#[test]
fn large_record_fragment_marks_real_chunk_corruption_unavailable_without_materializing_history() {
    let files = MemStore::new();
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(files.clone()), dir: "hash-fixture".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    session
        .save_companion_tail(photo, None, &[json!({"role":"user","text":"x".repeat(6*1024*1024)})], &[], &[], &json!({"schema_version":2}))
        .unwrap();
    let header = session.companion_photo_header(photo).unwrap().unwrap();
    let index: serde_json::Value = serde_json::from_slice(&files.get(&header.message_root.as_ref().unwrap().key).unwrap()).unwrap();
    let key = index["entries"][0]["block"].as_str().unwrap();
    let mut bytes = files.get(key).unwrap();
    bytes[100] ^= 1;
    files.set(key, bytes.clone());
    let page = session.companion_history_slice(photo, HistoryCursor::default(), 32 * 1024).unwrap();
    assert!(matches!(page.items.as_slice(), [super::storage::HistoryItem::Unavailable { record: 0 }]));
    assert!(page.records.is_empty() && page.fragments.is_empty(), "unverified content is never exposed");
    assert!(page.errors[0].contains("chunk digest mismatch"));
    assert!(page.bytes <= 32 * 1024);
    let failure = session.companion_protocol_messages_bounded(&header, 0, 1, 32 * 1024 * 1024).unwrap_err();
    assert!(failure.to_string().contains("chunk digest mismatch"));
    assert_eq!(files.get(key).unwrap(), bytes);
    assert_eq!(session.companion_photo_header(photo).unwrap().unwrap().message_count, 1);
}

#[test]
fn retrying_a_staged_late_receipt_cannot_restore_eligibility_after_a_manual_edit() {
    use lightcraft_catalog::Store;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Fault {
        files: MemStore,
        fail: Arc<AtomicBool>,
    }
    impl Store for Fault {
        fn read(&mut self, n: &str) -> std::io::Result<Option<Vec<u8>>> {
            self.files.read(n)
        }
        fn read_limited(&mut self, n: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
            self.files.read_limited(n, max)
        }
        fn read_range(&mut self, n: &str, o: u64, l: usize) -> std::io::Result<Vec<u8>> {
            self.files.read_range(n, o, l)
        }
        fn write_atomic(&mut self, n: &str, b: &[u8]) -> std::io::Result<()> {
            if self.fail.load(Ordering::Relaxed) && n.ends_with("/HEAD.json") {
                return Err(std::io::Error::other("receipt HEAD failure"));
            }
            self.files.write_atomic(n, b)
        }
        fn append(&mut self, n: &str, b: &[u8]) -> std::io::Result<()> {
            self.files.append(n, b)
        }
        fn truncate(&mut self, n: &str, l: u64) -> std::io::Result<()> {
            self.files.truncate(n, l)
        }
        fn remove(&mut self, n: &str) -> std::io::Result<()> {
            self.files.remove(n)
        }
        fn describe(&self) -> String {
            "receipt retry fixture".into()
        }
    }
    let files = MemStore::new();
    let fail = Arc::new(AtomicBool::new(false));
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(Fault { files: files.clone(), fail: fail.clone() }),
                dir: "receipt-fixture".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let identity = super::RoundIdentity {
        scope: super::PhotoScope { library_generation: session.library_generation(), photo },
        local_round_id: super::LocalRoundId::new().unwrap(),
        submission_seq: 9,
        connection_generation: 3,
    };
    let record = json!({"local_round_id":identity.local_round_id,"submission_seq":9,"connection_generation":3,"runtime_library_generation":identity.scope.library_generation,"stages":[{"plan_id":"plan","execution_outcome":"succeeded","delivery_state":"pending"}]});
    session
        .save_companion_tail(
            photo,
            None,
            &[],
            &[],
            &[record],
            &json!({"schema_version":2,"checkpoint":{"eligible":true,"local_round_id":identity.local_round_id}}),
        )
        .unwrap();
    fail.store(true, Ordering::Relaxed);
    assert!(session.confirm_companion_execution(&identity, "plan").is_err());
    let pending = session.companion_pending(photo).unwrap().unwrap();
    assert_ne!(session.companion_round(photo, identity.local_round_id.as_str()).unwrap().unwrap()["backend_confirmed_plan"], "plan");
    assert!(
        matches!(session.execute("develop.set", &json!({"ids":[photo.0],"values":{"light.exposure":0.5}})), Err(crate::EngineError::NotSaved(_))),
        "receipt storage debt must remain visible after the ordinary edit"
    );
    assert_eq!(session.develop_of(photo).unwrap().light.exposure, 0.5);
    fail.store(false, Ordering::Relaxed);
    assert!(session.confirm_companion_execution(&identity, "plan").unwrap());
    assert_eq!(session.companion_photo_header(photo).unwrap().unwrap().transaction_id, pending.transaction_id);
    assert_eq!(session.companion_round(photo, identity.local_round_id.as_str()).unwrap().unwrap()["backend_confirmed_plan"], "plan");
    assert_eq!(session.companion_metadata(photo).unwrap().unwrap()["checkpoint"]["eligible"], false);
    assert_eq!(session.develop_of(photo).unwrap().light.exposure, 0.5);
    assert!(session.confirm_companion_execution(&identity, "plan").unwrap());
    assert_eq!(session.companion_metadata(photo).unwrap().unwrap()["checkpoint"]["eligible"], false);
}

#[test]
fn oversized_text_fragments_decode_escapes_without_exposing_record_json_or_losing_text() {
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "escaped-text".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let text = "自然\n\"\\🙂\u{0001}".repeat(300_000);
    session
        .save_companion_tail(
            photo,
            None,
            &[json!({"role":"assistant","text":text,"local_round_id":"historical"})],
            &[],
            &[],
            &json!({"schema_version":2}),
        )
        .unwrap();
    let mut cursor = HistoryCursor::default();
    let mut visible = String::new();
    let mut slices = 0;
    loop {
        let page = session.companion_history_slice(photo, cursor, 32 * 1024).unwrap();
        assert!(page.bytes <= 32 * 1024);
        assert!(page.records.is_empty());
        for fragment in page.fragments {
            assert!(!fragment.contains("local_round_id"));
            visible.push_str(&fragment);
        }
        slices += 1;
        if let Some(next) = page.next {
            cursor = next;
        } else {
            break;
        }
    }
    assert!(slices > 128);
    assert_eq!(visible, text);
}

#[test]
fn runtime_prefix_roots_have_a_byte_bound_and_release_capacity() {
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "root-cache".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let header = session.save_companion_tail(photo, None, &[], &[], &[], &json!({"schema_version":2})).unwrap();
    let mut oversized = header.clone();
    oversized.metadata = "x".repeat(40 * 1024);
    assert!(session.pin_companion_prefix(photo, Some(oversized)).is_err());
    for n in 0..32 {
        let mut h = header.clone();
        h.photo_id = lightcraft_catalog::PhotoId(10_000 + n);
        session.pin_companion_prefix(h.photo_id, Some(h)).unwrap();
    }
    let mut extra = header;
    extra.photo_id = lightcraft_catalog::PhotoId(20_000);
    assert!(session.pin_companion_prefix(extra.photo_id, Some(extra.clone())).is_err());
    session.pin_companion_prefix(lightcraft_catalog::PhotoId(10_000), None).unwrap();
    session.pin_companion_prefix(extra.photo_id, Some(extra)).unwrap();
}
