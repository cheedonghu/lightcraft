use lightcraft_catalog::{MemStore, PhotoId};
use lightcraft_engine::{Session, companion::LocalRoundId, library::LibraryStores};
use serde_json::json;

fn open(catalog: &MemStore, files: &MemStore) -> Session {
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "memory".into(), on_disk: false },
        true,
    )
    .unwrap();
    s
}
fn publish(s: &mut Session, id: PhotoId) -> LocalRoundId {
    let round = LocalRoundId::new().unwrap();
    let start = s.develop_of(id).unwrap();
    let token = s.acquire_companion_lease(id, round.clone()).unwrap();
    let mut edited = (*s.develop_of(id).unwrap()).clone();
    edited.light.exposure = 0.75;
    let op = s.develop_op(id, edited, "AI edit plan").unwrap();
    s.commit_companion(&token, &round, s.photo_revision(id), op).unwrap();
    drop(token);
    s.save_companion_conversation(id, json!({"schema_version":2,"messages":[{"role":"user","text":"brighten"}],"rounds":[],"round_records":[{"local_round_id":round.as_str(),"original":start,"recent_complete_result":s.develop_of(id).unwrap(),"complete_state":"complete"}]})).unwrap();
    round
}
#[test]
fn normally_saved_round_restores_its_complete_start_after_reopen() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files);
    let id = s.active().unwrap();
    let start = s.develop_of(id).unwrap();
    let round = publish(&mut s, id);
    s.close_library().unwrap();
    let mut s = open(&catalog, &files);
    let verified = s.verify_round_revert(id, &round).unwrap();
    s.commit_verified_round_revert(verified).unwrap();
    assert_eq!(s.develop_of(id).unwrap(), start);
    assert!(s.companion_round(id, round.as_str()).unwrap().is_some());
}

use lightcraft_catalog::Store;
use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
struct FaultStore<S: Store> {
    memory: S,
    fail: Arc<AtomicBool>,
    visible: bool,
}
impl<S: Store> Store for FaultStore<S> {
    fn read(&mut self, n: &str) -> io::Result<Option<Vec<u8>>> {
        self.memory.read(n)
    }
    fn read_limited(&mut self, n: &str, limit: u64) -> io::Result<Option<Vec<u8>>> {
        self.memory.read_limited(n, limit)
    }
    fn read_range(&mut self, n: &str, o: u64, l: usize) -> io::Result<Vec<u8>> {
        self.memory.read_range(n, o, l)
    }
    fn remove(&mut self, n: &str) -> io::Result<()> {
        self.memory.remove(n)
    }
    fn write_atomic(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        if n.ends_with("/revert.json") && self.fail.load(Ordering::Relaxed) {
            if self.visible {
                self.memory.write_atomic(n, b)?;
            }
            return Err(io::Error::other("injected guard sync failure"));
        }
        self.memory.write_atomic(n, b)
    }
    fn append(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        self.memory.append(n, b)
    }
    fn truncate(&mut self, n: &str, l: u64) -> io::Result<()> {
        self.memory.truncate(n, l)
    }
    fn describe(&self) -> String {
        self.memory.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
}
#[test]
fn failed_guard_save_refuses_first_preview_and_explicit_retry_rechecks_disk() {
    for visible in [false, true] {
        let (catalog, files) = (MemStore::new(), MemStore::new());
        let fail = Arc::new(AtomicBool::new(false));
        let mut s = Session::new();
        s.open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FaultStore { memory: files.clone(), fail: fail.clone(), visible }),
                dir: "memory".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
        let id = s.active().unwrap();
        let other = s.catalog.photos().find(|p| p.id != id).unwrap().id;
        publish(&mut s, id);
        s.persist().unwrap();
        fail.store(true, Ordering::Relaxed);
        let before = s.develop_of(id).unwrap();
        let undo: Vec<_> = s.undo.iter().map(|e| e.label.clone()).collect();
        let redo: Vec<_> = s.redo.iter().map(|e| e.label.clone()).collect();
        s.begin_interaction("Exposure").unwrap();
        let mut edit = (*before).clone();
        edit.light.exposure = 1.0;
        assert!(s.set_develop(id, edit.clone(), "Exposure").is_err(), "first preview requires durable guard");
        assert_eq!(s.develop_of(id).unwrap(), before);
        assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), undo);
        assert_eq!(s.redo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), redo);
        assert!(s.set_develop(id, edit.clone(), "Exposure").is_err(), "memory invalidation is not a receipt");
        s.cancel_interaction().unwrap();
        let mut independent = (*s.develop_of(other).unwrap()).clone();
        independent.light.exposure = -0.25;
        s.set_develop(other, independent.clone(), "Independent").unwrap();
        assert_eq!(s.develop_of(other).unwrap().light.exposure, -0.25);
        fail.store(false, Ordering::Relaxed);
        s.persist().unwrap();
        assert_eq!(s.develop_of(id).unwrap(), before, "storage retry never replays edit");
        s.set_develop(id, edit, "Explicit retry").unwrap();
        assert_eq!(s.develop_of(id).unwrap().light.exposure, 1.0);
    }
}
#[test]
fn held_failed_gesture_cannot_replay_after_storage_retry() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let fail = Arc::new(AtomicBool::new(false));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog),
            files: Box::new(FaultStore { memory: files, fail: fail.clone(), visible: true }),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let id = s.active().unwrap();
    publish(&mut s, id);
    s.persist().unwrap();
    let before = s.develop_of(id).unwrap();
    s.begin_interaction("Exposure").unwrap();
    fail.store(true, Ordering::Relaxed);
    let mut target = (*before).clone();
    target.light.exposure = 1.1;
    assert!(s.set_develop(id, target.clone(), "Exposure").is_err());
    fail.store(false, Ordering::Relaxed);
    s.persist().unwrap();
    assert!(s.set_develop(id, target.clone(), "Exposure").is_err(), "held gesture must not replay rejected action");
    assert_eq!(s.develop_of(id).unwrap(), before);
    s.end_interaction().unwrap();
    s.begin_interaction("Explicit retry").unwrap();
    s.set_develop(id, target, "Explicit retry").unwrap();
    s.end_interaction().unwrap();
    assert_eq!(s.develop_of(id).unwrap().light.exposure, 1.1);
}
#[test]
fn actual_source_replacement_is_detected_despite_unchanged_cached_content_key() {
    let root = std::env::temp_dir().join(format!("lc-ux18-source-{}", LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("original.png");
    let image = lightcraft_raster::Rgba8::from_fn(8, 8, |x, y| [(x * 20) as u8, (y * 20) as u8, 60, 255]);
    let mut bytes = lightcraft_engine::export::encode_image(
        &image,
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    bytes.extend_from_slice(b"AAAA");
    std::fs::write(&path, &bytes).unwrap();
    let mut s = Session::with_demo().with_fs();
    let id = s.active().unwrap();
    s.commit(
        "Native source",
        lightcraft_catalog::Op::SetFile {
            id,
            file_name: "original.png".into(),
            source: lightcraft_catalog::Source::File { path: path.to_string_lossy().into() },
        },
    )
    .unwrap();
    s.commit(
        "Import hash",
        lightcraft_catalog::Op::SetContent {
            id,
            width: 8,
            height: 8,
            file_size: bytes.len() as u64,
            content_hash: Some(lightcraft_preview::hash_bytes(&bytes).to_string()),
            preview_only: None,
            preview_checked: String::new(),
        },
    )
    .unwrap();
    let round = publish(&mut s, id);
    let permit = s.verify_round_revert(id, &round).unwrap();
    let cache_key = lightcraft_engine::media::content_key(s.catalog.photo(id).unwrap());
    let end = bytes.len();
    bytes[end - 4..].copy_from_slice(b"BBBB");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(lightcraft_engine::media::content_key(s.catalog.photo(id).unwrap()), cache_key);
    assert!(s.commit_verified_round_revert(permit).is_err(), "action must recheck source after issuing permission");
    assert!(s.verify_round_revert(id, &round).is_err(), "fresh source evidence must reject replacement");
    assert_eq!(s.develop_of(id).unwrap().light.exposure, 0.75);
    std::fs::remove_dir_all(root).unwrap();
}
#[cfg(unix)]
#[test]
fn cold_physical_alias_is_protected_before_another_photos_file_move() {
    let root = std::env::temp_dir().join(format!("lc-ux18-alias-{}", LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir_all(&root).unwrap();
    let original = root.join("original.png");
    let alias = root.join("alias.png");
    let image = lightcraft_raster::Rgba8::from_fn(8, 8, |x, y| [(x * 20) as u8, (y * 20) as u8, 60, 255]);
    let bytes = lightcraft_engine::export::encode_image(
        &image,
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    std::fs::write(&original, &bytes).unwrap();
    std::os::unix::fs::symlink(&original, &alias).unwrap();
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files).with_fs();
    let a = s.active().unwrap();
    let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
    for (id, path, name) in [(a, &alias, "alias.png"), (b, &original, "original.png")] {
        s.commit(
            "Native source",
            lightcraft_catalog::Op::SetFile {
                id,
                file_name: name.into(),
                source: lightcraft_catalog::Source::File { path: path.to_string_lossy().into() },
            },
        )
        .unwrap();
    }
    let round = publish(&mut s, a);
    s.close_library().unwrap();
    let fail = Arc::new(AtomicBool::new(true));
    let mut s = Session::new().with_fs();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog.clone()),
            files: Box::new(FaultStore { memory: files.clone(), fail: fail.clone(), visible: false }),
            dir: "memory".into(),
            on_disk: false,
        },
        false,
    )
    .unwrap();
    let before = serde_json::to_value(&s.catalog).unwrap();
    let plans = s.plan_rename(&[b], "moved", 1);
    assert!(s.apply_rename(&plans).is_err(), "unopened alias must gate actual physical move");
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert!(s.undo.is_empty());
    assert!(s.redo.is_empty());
    assert_eq!(std::fs::read(&original).unwrap(), bytes);
    assert_eq!(std::fs::read(&alias).unwrap(), bytes);
    assert!(!root.join("moved.png").exists());
    fail.store(false, Ordering::Relaxed);
    s.persist().unwrap();
    assert!(s.verify_round_revert(a, &round).is_err());
    s.apply_rename(&plans).unwrap();
    assert!(root.join("moved.png").exists());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn damaged_revert_guard_never_panics_or_applies_an_edit() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files);
    let id = s.active().unwrap();
    publish(&mut s, id);
    s.persist().unwrap();
    let key = format!("companion/v2/photos/{}/revert.json", id.0);
    let bytes = br#"{"eligible":true,"mutation_epoch":0,"certificate":7}"#.to_vec();
    files.set(&key, bytes.clone());
    let before = s.develop_of(id).unwrap();
    let mut edit = (*before).clone();
    edit.light.exposure = 1.2;
    assert!(s.set_develop(id, edit, "Exposure").is_err());
    assert_eq!(s.develop_of(id).unwrap(), before);
    assert_eq!(files.get(&key).unwrap(), bytes);
}
#[test]
fn another_ai_publication_cannot_bypass_failed_previous_guard_save() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let fail = Arc::new(AtomicBool::new(false));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog),
            files: Box::new(FaultStore { memory: files, fail: fail.clone(), visible: false }),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let id = s.active().unwrap();
    publish(&mut s, id);
    s.persist().unwrap();
    let before = s.develop_of(id).unwrap();
    let round = LocalRoundId::new().unwrap();
    let lease = s.acquire_companion_lease(id, round.clone()).unwrap();
    let mut edit = (*before).clone();
    edit.light.exposure = 1.3;
    let op = s.develop_op(id, edit, "AI edit plan").unwrap();
    let labels: Vec<_> = s.undo.iter().map(|e| e.label.clone()).collect();
    fail.store(true, Ordering::Relaxed);
    assert!(s.commit_companion(&lease, &round, s.photo_revision(id), op).is_err(), "AI publication must relinquish old durable authority first");
    assert_eq!(s.develop_of(id).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
}
use std::sync::atomic::AtomicU8;
struct CoreFault {
    memory: MemStore,
    mode: Arc<AtomicU8>,
}
impl Store for CoreFault {
    fn read(&mut self, n: &str) -> io::Result<Option<Vec<u8>>> {
        self.memory.read(n)
    }
    fn read_limited(&mut self, n: &str, l: u64) -> io::Result<Option<Vec<u8>>> {
        self.memory.read_limited(n, l)
    }
    fn read_range(&mut self, n: &str, o: u64, l: usize) -> io::Result<Vec<u8>> {
        self.memory.read_range(n, o, l)
    }
    fn remove(&mut self, n: &str) -> io::Result<()> {
        self.memory.remove(n)
    }
    fn write_atomic(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        if n == lightcraft_catalog::journal::SNAPSHOT && self.mode.load(Ordering::Relaxed) == 2 {
            return Err(io::Error::other("injected closing snapshot failure"));
        }
        if n == lightcraft_catalog::journal::LOG && self.mode.load(Ordering::Relaxed) == 3 {
            return Err(io::Error::other("injected closing log reset failure"));
        }
        self.memory.write_atomic(n, b)
    }
    fn append(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        if matches!(self.mode.load(Ordering::Relaxed), 1 | 3) {
            return Err(io::Error::other("injected append failure"));
        }
        self.memory.append(n, b)
    }
    fn truncate(&mut self, n: &str, l: u64) -> io::Result<()> {
        self.memory.truncate(n, l)
    }
    fn describe(&self) -> String {
        self.memory.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
}
#[test]
fn failed_final_close_snapshot_cannot_leave_a_reopen_certificate() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mode = Arc::new(AtomicU8::new(0));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(CoreFault { memory: catalog.clone(), mode: mode.clone() }),
            files: Box::new(files.clone()),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let id = s.active().unwrap();
    let round = publish(&mut s, id);
    s.persist().unwrap();
    mode.store(2, Ordering::Relaxed);
    assert!(s.close_library().is_err());
    let mut reopened = open(&catalog, &files);
    assert!(reopened.verify_round_revert(id, &round).is_err(), "failed final snapshot must not leave durable qualification");
    assert!(reopened.companion_round(id, round.as_str()).unwrap().is_some());
}

#[test]
fn visible_failed_native_certificate_write_is_not_a_reopen_permission() {
    let root = std::env::temp_dir().join(format!("lc-ux18-seal-{}", LocalRoundId::new().unwrap().as_str()));
    let fail = Arc::new(AtomicBool::new(false));
    let mut s = Session::new().with_fs();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(lightcraft_catalog::FsStore::open(&root).unwrap()),
            files: Box::new(FaultStore { memory: lightcraft_catalog::FsStore::open(&root).unwrap(), fail: fail.clone(), visible: true }),
            dir: root.clone(),
            on_disk: true,
        },
        true,
    )
    .unwrap();
    let id = s.active().unwrap();
    let round = publish(&mut s, id);
    let before = s.develop_of(id).unwrap();
    fail.store(true, Ordering::Relaxed);
    assert!(s.persist().is_err());
    let mut reopened = Session::new().with_fs();
    reopened.open_library(&root, false).unwrap();
    assert!(reopened.verify_round_revert(id, &round).is_err(), "visible failed seal is not durable authority");
    assert!(reopened.companion_round(id, round.as_str()).unwrap().is_some());
    drop(reopened);
    fail.store(false, Ordering::Relaxed);
    s.persist().unwrap();
    assert_eq!(s.develop_of(id).unwrap(), before);
    drop(s);
    let mut reopened = Session::new().with_fs();
    reopened.open_library(&root, false).unwrap();
    let permit = reopened.verify_round_revert(id, &round).unwrap();
    reopened.commit_verified_round_revert(permit).unwrap();
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn earlier_edit_seals_after_many_pure_answers_advance_head_before_core_save() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files);
    let id = s.active().unwrap();
    let round = publish(&mut s, id);
    let first = s.companion_round(id, round.as_str()).unwrap().unwrap();
    let current = s.develop_of(id).unwrap();
    let noop = LocalRoundId::new().unwrap();
    let lease = s.acquire_companion_lease(id, noop.clone()).unwrap();
    let op = s.develop_op(id, (*current).clone(), "AI edit plan").unwrap();
    s.commit_companion(&lease, &noop, s.photo_revision(id), op).unwrap();
    drop(lease);
    let mut records = vec![first];
    for i in 0..140 {
        records.push(json!({"local_round_id":format!("answer-{i}"),"phase":"completed","stages":[]}));
    }
    s.save_companion_conversation(
        id,
        json!({"schema_version":2,"messages":[{"role":"user","text":"many answers"}],"rounds":[],"round_records":records}),
    )
    .unwrap();
    assert!(s.verify_round_revert(id, &round).is_ok());
    s.close_library().unwrap();
    let mut reopened = open(&catalog, &files);
    let permit = reopened.verify_round_revert(id, &round).unwrap();
    reopened.commit_verified_round_revert(permit).unwrap();
    assert_eq!(reopened.develop_of(id).unwrap().light.exposure, 0.0);
}
#[test]
fn batch_deletion_refuses_all_parameter_and_history_effects_if_guard_save_fails() {
    let fail = Arc::new(AtomicBool::new(false));
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog),
            files: Box::new(FaultStore { memory: files, fail: fail.clone(), visible: false }),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let a = s.active().unwrap();
    let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
    publish(&mut s, a);
    s.persist().unwrap();
    let before = serde_json::to_value(&s.catalog).unwrap();
    let mut other = (*s.develop_of(b).unwrap()).clone();
    other.light.exposure = -0.4;
    let op = lightcraft_catalog::Op::Batch {
        ops: vec![s.develop_op(b, other, "Batch").unwrap(), lightcraft_catalog::Op::SetDeleted { id: a, deleted: true }],
    };
    let undo: Vec<_> = s.undo.iter().map(|e| e.label.clone()).collect();
    fail.store(true, Ordering::Relaxed);
    assert!(s.commit("Batch deletion", op).is_err(), "logical deletion must gate whole batch");
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), undo);
    assert!(s.redo.is_empty());
}
#[test]
fn repaired_torn_replay_does_not_regain_permission_on_the_next_clean_reopen() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files);
    let id = s.active().unwrap();
    let round = publish(&mut s, id);
    s.close_library().unwrap();
    drop(s);
    catalog.set(lightcraft_catalog::journal::LOG, b"{torn".to_vec());
    let mut recovered = open(&catalog, &files);
    assert!(recovered.library.as_ref().unwrap().report.torn_bytes > 0);
    assert!(recovered.verify_round_revert(id, &round).is_err());
    recovered.close_library().unwrap();
    drop(recovered);
    let mut clean = open(&catalog, &files);
    assert_eq!(clean.library.as_ref().unwrap().report.torn_bytes, 0);
    assert!(clean.verify_round_revert(id, &round).is_err(), "repair must not resurrect a certificate at equal params and seq");
    assert!(clean.companion_round(id, round.as_str()).unwrap().is_some());
}
#[test]
fn failed_retirement_during_replay_repair_blocks_later_clean_reopen() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files);
    let id = s.active().unwrap();
    let round = publish(&mut s, id);
    s.close_library().unwrap();
    drop(s);
    catalog.set(lightcraft_catalog::journal::LOG, b"{torn".to_vec());
    let fail = Arc::new(AtomicBool::new(true));
    let mut recovered = Session::new();
    recovered
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FaultStore { memory: files.clone(), fail, visible: false }),
                dir: "memory".into(),
                on_disk: false,
            },
            false,
        )
        .unwrap();
    assert!(recovered.verify_round_revert(id, &round).is_err());
    drop(recovered);
    let mut clean = open(&catalog, &files);
    assert_eq!(clean.library.as_ref().unwrap().report.torn_bytes, 0);
    assert!(clean.verify_round_revert(id, &round).is_err(), "failed retirement must survive erased replay warning");
    assert!(clean.companion_round(id, round.as_str()).unwrap().is_some());
}
#[test]
fn detected_source_replacement_cannot_regain_permission_when_bytes_are_restored() {
    let root = std::env::temp_dir().join(format!("lc-ux18-source-{}", LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("original.png");
    let image = lightcraft_raster::Rgba8::from_fn(8, 8, |x, y| [(x * 20) as u8, (y * 20) as u8, 60, 255]);
    let mut bytes = lightcraft_engine::export::encode_image(
        &image,
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    bytes.extend_from_slice(b"AAAA");
    std::fs::write(&path, &bytes).unwrap();
    let mut s = Session::with_demo().with_fs();
    let id = s.active().unwrap();
    s.commit(
        "Native source",
        lightcraft_catalog::Op::SetFile {
            id,
            file_name: "original.png".into(),
            source: lightcraft_catalog::Source::File { path: path.to_string_lossy().into() },
        },
    )
    .unwrap();
    s.commit(
        "Import hash",
        lightcraft_catalog::Op::SetContent {
            id,
            width: 8,
            height: 8,
            file_size: bytes.len() as u64,
            content_hash: Some(lightcraft_preview::hash_bytes(&bytes).to_string()),
            preview_only: None,
            preview_checked: String::new(),
        },
    )
    .unwrap();
    let round = publish(&mut s, id);
    assert!(s.verify_round_revert(id, &round).is_ok());
    let cache_key = lightcraft_engine::media::content_key(s.catalog.photo(id).unwrap());
    let original = bytes.clone();
    let end = bytes.len();
    bytes[end - 4..].copy_from_slice(b"BBBB");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(lightcraft_engine::media::content_key(s.catalog.photo(id).unwrap()), cache_key);
    assert!(s.verify_round_revert(id, &round).is_err(), "fresh source evidence must reject replacement");
    std::fs::write(&path, &original).unwrap();
    assert!(s.verify_round_revert(id, &round).is_err(), "detected source replacement is a permanent relinquishment");
    assert_eq!(s.develop_of(id).unwrap().light.exposure, 0.75);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn catalog_removal_refuses_whole_batch_if_guard_save_fails() {
    let fail = Arc::new(AtomicBool::new(false));
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog),
            files: Box::new(FaultStore { memory: files, fail: fail.clone(), visible: false }),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let a = s.active().unwrap();
    let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
    publish(&mut s, a);
    s.persist().unwrap();
    let before = serde_json::to_value(&s.catalog).unwrap();
    let mut other = (*s.develop_of(b).unwrap()).clone();
    other.light.exposure = -0.4;
    let op = lightcraft_catalog::Op::Batch { ops: vec![s.develop_op(b, other, "Batch").unwrap(), lightcraft_catalog::Op::RemovePhoto { id: a }] };
    let undo: Vec<_> = s.undo.iter().map(|e| e.label.clone()).collect();
    fail.store(true, Ordering::Relaxed);
    assert!(s.commit("Batch deletion", op).is_err(), "catalog removal must gate whole batch");
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), undo);
    assert!(s.redo.is_empty());
}
#[test]
fn failed_close_certificate_never_revives_after_another_clean_close() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mode = Arc::new(AtomicU8::new(0));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(CoreFault { memory: catalog.clone(), mode: mode.clone() }),
            files: Box::new(files.clone()),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let id = s.active().unwrap();
    let round = publish(&mut s, id);
    s.persist().unwrap();
    mode.store(2, Ordering::Relaxed);
    assert!(s.close_library().is_err());
    let mut reopened = open(&catalog, &files);
    assert!(reopened.verify_round_revert(id, &round).is_err(), "failed final snapshot must not leave durable qualification");
    assert!(reopened.companion_round(id, round.as_str()).unwrap().is_some());
    reopened.close_library().unwrap();
    drop(reopened);
    let mut clean = open(&catalog, &files);
    assert!(clean.verify_round_revert(id, &round).is_err(), "closing a readonly reopen must not revive failed-close authority");
}

#[test]
fn source_mismatch_during_anchor_refresh_permanently_relinquishes_permission() {
    let root = std::env::temp_dir().join(format!("lc-ux18-source-{}", LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("original.png");
    let image = lightcraft_raster::Rgba8::from_fn(8, 8, |x, y| [(x * 20) as u8, (y * 20) as u8, 60, 255]);
    let mut bytes = lightcraft_engine::export::encode_image(
        &image,
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    bytes.extend_from_slice(b"AAAA");
    std::fs::write(&path, &bytes).unwrap();
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files).with_fs();
    let id = s.active().unwrap();
    s.commit(
        "Native source",
        lightcraft_catalog::Op::SetFile {
            id,
            file_name: "original.png".into(),
            source: lightcraft_catalog::Source::File { path: path.to_string_lossy().into() },
        },
    )
    .unwrap();
    s.commit(
        "Import hash",
        lightcraft_catalog::Op::SetContent {
            id,
            width: 8,
            height: 8,
            file_size: bytes.len() as u64,
            content_hash: Some(lightcraft_preview::hash_bytes(&bytes).to_string()),
            preview_only: None,
            preview_checked: String::new(),
        },
    )
    .unwrap();
    let round = publish(&mut s, id);
    s.persist().unwrap();
    assert!(s.verify_round_revert(id, &round).is_ok());
    let cache_key = lightcraft_engine::media::content_key(s.catalog.photo(id).unwrap());
    let original = bytes.clone();
    let end = bytes.len();
    bytes[end - 4..].copy_from_slice(b"BBBB");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(lightcraft_engine::media::content_key(s.catalog.photo(id).unwrap()), cache_key);
    let other = s.catalog.photos().find(|p| p.id != id).unwrap().id;
    s.execute("photo.rate", &json!({"ids":[other.0],"rating":3})).unwrap();
    std::fs::write(&path, &original).unwrap();
    assert!(s.verify_round_revert(id, &round).is_err(), "detected source replacement is a permanent relinquishment");
    assert_eq!(s.develop_of(id).unwrap().light.exposure, 0.75);
    std::fs::remove_dir_all(root).unwrap();
}

struct HeadFault {
    memory: MemStore,
    fail: Arc<AtomicBool>,
}
impl Store for HeadFault {
    fn read(&mut self, n: &str) -> io::Result<Option<Vec<u8>>> {
        self.memory.read(n)
    }
    fn read_limited(&mut self, n: &str, l: u64) -> io::Result<Option<Vec<u8>>> {
        self.memory.read_limited(n, l)
    }
    fn read_range(&mut self, n: &str, o: u64, l: usize) -> io::Result<Vec<u8>> {
        self.memory.read_range(n, o, l)
    }
    fn remove(&mut self, n: &str) -> io::Result<()> {
        self.memory.remove(n)
    }
    fn write_atomic(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        if n.ends_with("/HEAD.json") && self.fail.load(Ordering::Relaxed) {
            return Err(io::Error::other("injected conversation HEAD failure"));
        }
        self.memory.write_atomic(n, b)
    }
    fn append(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        self.memory.append(n, b)
    }
    fn truncate(&mut self, n: &str, l: u64) -> io::Result<()> {
        self.memory.truncate(n, l)
    }
    fn describe(&self) -> String {
        self.memory.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
}
#[test]
fn recovered_conversation_save_seals_the_existing_trusted_candidate() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let fail = Arc::new(AtomicBool::new(false));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog.clone()),
            files: Box::new(HeadFault { memory: files.clone(), fail: fail.clone() }),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let id = s.active().unwrap();
    let start = s.develop_of(id).unwrap();
    let round = LocalRoundId::new().unwrap();
    let lease = s.acquire_companion_lease(id, round.clone()).unwrap();
    let mut edit = (*start).clone();
    edit.light.exposure = 0.75;
    let op = s.develop_op(id, edit, "AI edit plan").unwrap();
    s.commit_companion(&lease, &round, s.photo_revision(id), op).unwrap();
    drop(lease);
    fail.store(true, Ordering::Relaxed);
    assert!(s.save_companion_conversation(id,json!({"schema_version":2,"messages":[{"role":"user","text":"brighten"}],"rounds":[],"round_records":[{"local_round_id":round.as_str(),"original":start,"recent_complete_result":s.develop_of(id).unwrap(),"complete_state":"complete"}]})).is_err());
    let result = s.develop_of(id).unwrap();
    fail.store(false, Ordering::Relaxed);
    s.persist().unwrap();
    assert_eq!(s.develop_of(id).unwrap(), result, "storage retry never replays publication");
    s.close_library().unwrap();
    drop(s);
    let mut reopened = open(&catalog, &files);
    let permit = reopened.verify_round_revert(id, &round).unwrap();
    reopened.commit_verified_round_revert(permit).unwrap();
    assert_eq!(reopened.develop_of(id).unwrap(), start);
}

#[test]
fn unsealed_candidate_keeps_invalidation_debt_when_a_later_batch_guard_is_damaged() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let fail = Arc::new(AtomicBool::new(false));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog),
            files: Box::new(FaultStore { memory: files.clone(), fail: fail.clone(), visible: false }),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let mut ids: Vec<_> = s.catalog.photos().map(|p| p.id).collect();
    ids.sort();
    let (a, b) = (ids[0], ids[1]);
    publish(&mut s, a);
    files.set(&format!("companion/v2/photos/{}/revert.json", b.0), b"broken".to_vec());
    let before = serde_json::to_value(&s.catalog).unwrap();
    let labels: Vec<_> = s.undo.iter().map(|e| e.label.clone()).collect();
    let mut edit = (*s.develop_of(a).unwrap()).clone();
    edit.light.exposure = 1.5;
    let op = lightcraft_catalog::Op::Batch {
        ops: vec![s.develop_op(a, edit.clone(), "Batch").unwrap(), lightcraft_catalog::Op::SetDeleted { id: b, deleted: true }],
    };
    assert!(s.commit("Batch", op).is_err());
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    fail.store(true, Ordering::Relaxed);
    assert!(s.set_develop(a, edit, "Explicit retry").is_err(), "retired memory proof must retain its durable invalidation debt");
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
    assert!(s.redo.is_empty());
}

#[test]
fn convert_to_dng_refuses_before_creating_any_native_file_when_guard_save_fails() {
    let root = std::env::temp_dir().join(format!("lc-ux18-convert-{}", LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir_all(&root).unwrap();
    let raw = lightcraft_raw::RawImage {
        format: lightcraft_raw::RawFormat::Dng,
        width: 16,
        height: 16,
        cpp: 1,
        data: lightcraft_raw::RawData::U16((0..256).map(|i| 800 + i * 8).collect()),
        cfa: Some(lightcraft_raw::Cfa::bayer("RGGB").unwrap()),
        bits: 12,
        black: lightcraft_raw::BlackLevel::uniform(528.0),
        white: vec![4095.0],
        active_area: lightcraft_raw::Rect::new(0, 0, 16, 16),
        crop: lightcraft_raw::Rect::new(0, 0, 16, 16),
        orientation: lightcraft_raw::Orientation::Normal,
        color: Default::default(),
        wb_multipliers: None,
        linearized: false,
        opcodes: Default::default(),
        metadata: Default::default(),
        lens_metadata: Default::default(),
        pixel_aspect: 1.0,
    };
    let bytes = lightcraft_raw::write_dng(&raw, &Default::default()).unwrap();
    let path = root.join("original.nef");
    std::fs::write(&path, &bytes).unwrap();
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let fail = Arc::new(AtomicBool::new(false));
    let mut s = Session::new().with_fs();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog),
            files: Box::new(FaultStore { memory: files, fail: fail.clone(), visible: false }),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let id = PhotoId(999);
    let mut photo = (**s.catalog.photo(s.active().unwrap()).unwrap()).clone();
    photo.id = id;
    photo.kind = lightcraft_catalog::MediaKind::Raw;
    photo.format = "NEF".into();
    photo.file_name = "original.nef".into();
    photo.source = lightcraft_catalog::Source::File { path: path.to_string_lossy().into() };
    s.commit("Raw source", lightcraft_catalog::Op::AddPhoto { photo: Box::new(photo) }).unwrap();
    publish(&mut s, id);
    s.persist().unwrap();
    let before = serde_json::to_value(&s.catalog).unwrap();
    let labels: Vec<_> = s.undo.iter().map(|e| e.label.clone()).collect();
    fail.store(true, Ordering::Relaxed);
    assert!(s.execute("photo.convertToDng", &json!({"ids":[id.0]})).is_err());
    assert!(!root.join("original.dng").exists(), "pre-effect protection includes DNG creation before Relink");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
    assert!(s.redo.is_empty());
    fail.store(false, Ordering::Relaxed);
    s.persist().unwrap();
    assert!(!root.join("original.dng").exists(), "storage retry must not replay conversion");
    s.execute("photo.convertToDng", &json!({"ids":[id.0]})).unwrap();
    assert!(root.join("original.dng").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn frozen_effect_qualifies_without_materializing_a_large_round_record() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files);
    let id = s.active().unwrap();
    let start = s.develop_of(id).unwrap();
    let round = publish(&mut s, id);
    let mut stored = s.load_companion_conversation(id).unwrap().unwrap();
    stored["round_records"][0]["diagnostic_padding"] = json!("x".repeat(33 * 1024 * 1024));
    s.save_companion_conversation(id, stored).unwrap();
    s.close_library().unwrap();
    drop(s);
    let mut reopened = open(&catalog, &files);
    let permit = reopened.verify_round_revert(id, &round).unwrap();
    reopened.commit_verified_round_revert(permit).unwrap();
    assert_eq!(reopened.develop_of(id).unwrap(), start);
}

#[test]
fn undo_refusal_preserves_stacks_and_redo_equality_never_regrants_old_authority() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let fail = Arc::new(AtomicBool::new(false));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog.clone()),
            files: Box::new(FaultStore { memory: files.clone(), fail: fail.clone(), visible: false }),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let id = s.active().unwrap();
    let round = publish(&mut s, id);
    s.persist().unwrap();
    let before = serde_json::to_value(&s.catalog).unwrap();
    let undo: Vec<_> = s.undo.iter().map(|e| e.label.clone()).collect();
    let redo: Vec<_> = s.redo.iter().map(|e| e.label.clone()).collect();
    fail.store(true, Ordering::Relaxed);
    assert!(s.undo_step().is_err());
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), undo);
    assert_eq!(s.redo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), redo);
    fail.store(false, Ordering::Relaxed);
    s.persist().unwrap();
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    s.undo_step().unwrap();
    s.redo_step().unwrap();
    assert_eq!(s.develop_of(id).unwrap().light.exposure, 0.75);
    assert!(s.verify_round_revert(id, &round).is_err());
    s.close_library().unwrap();
    drop(s);
    let mut reopened = open(&catalog, &files);
    assert!(reopened.verify_round_revert(id, &round).is_err());
    assert!(reopened.companion_round(id, round.as_str()).unwrap().is_some());
}

#[test]
fn context_retirement_retires_runtime_and_future_certificate_without_changing_photo_or_history() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let fail = Arc::new(AtomicBool::new(false));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog.clone()),
            files: Box::new(FaultStore { memory: files.clone(), fail: fail.clone(), visible: false }),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let id = s.active().unwrap();
    let round = publish(&mut s, id);
    s.persist().unwrap();
    let before = serde_json::to_value(&s.catalog).unwrap();
    let labels: Vec<_> = s.undo.iter().map(|e| e.label.clone()).collect();
    let permit = s.verify_round_revert(id, &round).unwrap();
    fail.store(true, Ordering::Relaxed);
    assert!(s.retire_companion_revert(id).is_err());
    assert!(s.commit_verified_round_revert(permit).is_err());
    assert!(s.verify_round_revert(id, &round).is_err());
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
    assert!(s.companion_round(id, round.as_str()).unwrap().is_some());
    fail.store(false, Ordering::Relaxed);
    s.retire_companion_revert(id).unwrap();
    s.close_library().unwrap();
    drop(s);
    let mut reopened = open(&catalog, &files);
    assert!(reopened.verify_round_revert(id, &round).is_err());
    assert!(reopened.companion_round(id, round.as_str()).unwrap().is_some());
    assert_eq!(serde_json::to_value(&reopened.catalog).unwrap(), before);
}

#[test]
fn auto_sync_first_preview_protects_the_complete_expanded_batch() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let fail = Arc::new(AtomicBool::new(false));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog),
            files: Box::new(FaultStore { memory: files, fail: fail.clone(), visible: false }),
            dir: "memory".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let a = s.active().unwrap();
    let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
    publish(&mut s, a);
    s.persist().unwrap();
    s.execute("library.select", &json!({"ids":[b.0,a.0],"active":b.0})).unwrap();
    s.execute("develop.autoSync", &json!({"on":true})).unwrap();
    let before = serde_json::to_value(&s.catalog).unwrap();
    let undo: Vec<_> = s.undo.iter().map(|e| e.label.clone()).collect();
    let redo: Vec<_> = s.redo.iter().map(|e| e.label.clone()).collect();
    s.begin_interaction("Exposure").unwrap();
    fail.store(true, Ordering::Relaxed);
    let mut edit = (*s.develop_of(b).unwrap()).clone();
    edit.light.exposure = 1.25;
    assert!(s.set_develop(b, edit.clone(), "Exposure").is_err());
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), undo);
    assert_eq!(s.redo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), redo);
    fail.store(false, Ordering::Relaxed);
    s.persist().unwrap();
    assert!(s.set_develop(b, edit.clone(), "Exposure").is_err());
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    s.end_interaction().unwrap();
    s.begin_interaction("Explicit retry").unwrap();
    s.set_develop(b, edit, "Exposure").unwrap();
    s.end_interaction().unwrap();
    assert_eq!(s.develop_of(a).unwrap().light.exposure, 1.25);
    assert_eq!(s.develop_of(b).unwrap().light.exposure, 1.25);
}

#[test]
fn normal_compaction_and_other_photo_saves_preserve_a_valid_candidate() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files);
    let a = s.active().unwrap();
    let start = s.develop_of(a).unwrap();
    let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
    let round = publish(&mut s, a);
    s.persist().unwrap();
    s.execute("develop.set", &json!({"ids":[b.0],"values":{"light.exposure":-0.4}})).unwrap();
    s.execute("photo.rate", &json!({"ids":[b.0],"rating":3})).unwrap();
    s.compact_library().unwrap();
    assert!(s.verify_round_revert(a, &round).is_ok());
    s.close_library().unwrap();
    drop(s);
    let mut reopened = open(&catalog, &files);
    let permit = reopened.verify_round_revert(a, &round).unwrap();
    reopened.commit_verified_round_revert(permit).unwrap();
    assert_eq!(reopened.develop_of(a).unwrap(), start);
    assert_eq!(reopened.develop_of(b).unwrap().light.exposure, -0.4);
    assert_eq!(reopened.catalog.photo(b).unwrap().rating, 3);
}

#[test]
fn unavailable_original_evidence_keeps_history_readable_without_permission() {
    for mode in ["missing-smart", "missing-hooks", "unknown-demo"] {
        let root = std::env::temp_dir().join(format!("lc-ux18-unverified-{}", LocalRoundId::new().unwrap().as_str()));
        let path = root.join("original.png");
        let mut s = Session::new().with_fs();
        s.open_library(&root, true).unwrap();
        let id = s.active().unwrap();
        if mode == "unknown-demo" {
            s.commit(
                "Unknown source",
                lightcraft_catalog::Op::SetFile { id, file_name: "unknown".into(), source: lightcraft_catalog::Source::Demo { scene: u32::MAX } },
            )
            .unwrap();
        } else {
            let image = lightcraft_raster::Rgba8::from_fn(8, 8, |x, y| [(x * 20) as u8, (y * 20) as u8, 60, 255]);
            let bytes = lightcraft_engine::export::encode_image(
                &image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap();
            std::fs::write(&path, bytes).unwrap();
            s.commit(
                "Native source",
                lightcraft_catalog::Op::SetFile {
                    id,
                    file_name: "original.png".into(),
                    source: lightcraft_catalog::Source::File { path: path.to_string_lossy().into() },
                },
            )
            .unwrap();
            if mode == "missing-smart" {
                assert_eq!(s.execute("library.smartPreviews", &json!({"ids":[id.0]})).unwrap()["built"], 1);
            }
        }
        let round = publish(&mut s, id);
        let expected = s.develop_of(id).unwrap();
        s.close_library().unwrap();
        drop(s);
        if mode == "missing-smart" {
            std::fs::remove_file(&path).unwrap();
        }
        let mut reopened = if mode == "missing-hooks" { Session::new() } else { Session::new().with_fs() };
        reopened.open_library(&root, false).unwrap();
        if mode == "missing-smart" {
            let photo = reopened.catalog.photo(id).unwrap().clone();
            let source = reopened.media.source_ref(&photo, lightcraft_engine::media::SourceLevel::Preview);
            assert!(matches!(source, lightcraft_engine::media::SourceRef::Smart { .. }));
            assert_eq!(source.load_source().unwrap().image.width, 8);
        }
        assert!(reopened.verify_round_revert(id, &round).is_err(), "{mode}");
        assert_eq!(reopened.develop_of(id).unwrap(), expected);
        assert!(reopened.companion_round(id, round.as_str()).unwrap().is_some(), "{mode} history remains readable");
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn failed_damaged_and_upgraded_loads_retire_matching_sequence_certificates() {
    use lightcraft_catalog::journal::{Journal, LOG, SNAPSHOT, VERSION};
    for mode in ["failed", "damaged", "upgraded"] {
        let (catalog, files) = (MemStore::new(), MemStore::new());
        let mut s = open(&catalog, &files);
        let a = s.active().unwrap();
        let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
        let round = publish(&mut s, a);
        s.compact_library().unwrap();
        let snapshot = catalog.get(SNAPSHOT).unwrap();
        s.execute("photo.rate", &json!({"ids":[b.0],"rating":3})).unwrap();
        let seq = s.library.as_ref().unwrap().journal().seq();
        let expected = s.develop_of(a).unwrap();
        assert!(s.verify_round_revert(a, &round).is_ok());
        if mode == "upgraded" {
            s.close_library().unwrap();
        }
        drop(s);
        match mode {
            "failed" => {
                catalog.set(SNAPSHOT, snapshot);
                catalog.set(LOG, Vec::new());
                let (mut journal, _, _) = Journal::open(Box::new(catalog.clone())).unwrap();
                journal.append(&[lightcraft_catalog::Op::RemovePhoto { id: PhotoId(u64::MAX) }]).unwrap();
                assert_eq!(journal.seq(), seq);
            }
            "damaged" => {
                let log = catalog.get(LOG).unwrap();
                let mut damaged = log.clone();
                damaged.extend_from_slice(b"{damaged}\n");
                damaged.extend_from_slice(&log);
                catalog.set(LOG, damaged);
            }
            "upgraded" => {
                let mut snap: serde_json::Value = serde_json::from_slice(&catalog.get(SNAPSHOT).unwrap()).unwrap();
                snap["version"] = json!(VERSION - 1);
                catalog.set(SNAPSHOT, serde_json::to_vec(&snap).unwrap());
            }
            _ => unreachable!(),
        }
        let mut recovered = open(&catalog, &files);
        let lib = recovered.library.as_ref().unwrap();
        assert_eq!(lib.journal().seq(), seq, "{mode} must exercise a matching anchor");
        match mode {
            "failed" => assert_eq!(lib.report.failed, 1),
            "damaged" => assert!(lib.report.damaged.is_some()),
            "upgraded" => assert_eq!(lib.report.upgraded_from, Some(VERSION - 1)),
            _ => unreachable!(),
        }
        assert_eq!(recovered.develop_of(a).unwrap(), expected);
        assert!(recovered.verify_round_revert(a, &round).is_err(), "{mode}");
        assert!(recovered.companion_round(a, round.as_str()).unwrap().is_some());
        recovered.close_library().unwrap();
        drop(recovered);
        let mut clean = open(&catalog, &files);
        let report = &clean.library.as_ref().unwrap().report;
        assert_eq!(report.failed, 0);
        assert!(report.damaged.is_none());
        assert_eq!(report.upgraded_from, None);
        assert_eq!(clean.library.as_ref().unwrap().journal().seq(), seq);
        assert!(clean.verify_round_revert(a, &round).is_err(), "{mode} must not revive after erased warnings");
        assert_eq!(clean.develop_of(a).unwrap(), expected);
    }
}

#[test]
fn final_snapshot_rescues_failed_append_and_failed_log_reset_never_duplicates_pending_ops() {
    for reset_fails in [false, true] {
        let (catalog, files) = (MemStore::new(), MemStore::new());
        let mode = Arc::new(AtomicU8::new(0));
        let mut s = Session::new();
        s.open_library_in(
            LibraryStores {
                catalog: Box::new(CoreFault { memory: catalog.clone(), mode: mode.clone() }),
                files: Box::new(files.clone()),
                dir: "memory".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
        let id = s.active().unwrap();
        let start = s.develop_of(id).unwrap();
        let before = s.library.as_ref().unwrap().journal().seq();
        let round = publish(&mut s, id);
        mode.store(if reset_fails { 3 } else { 1 }, Ordering::Relaxed);
        let close = s.close_library();
        assert_eq!(close.is_err(), reset_fails);
        let final_seq = s.library.as_ref().unwrap().journal().seq();
        assert_eq!(final_seq, before + 1, "closing snapshot saves the unlogged publication exactly once");
        assert!(s.unsaved().is_none(), "snapshot already contains the pending publication");
        mode.store(0, Ordering::Relaxed);
        s.persist().unwrap();
        assert_eq!(s.library.as_ref().unwrap().journal().seq(), final_seq, "retry must not duplicate snapshot-saved pending ops");
        drop(s);
        let mut reopened = open(&catalog, &files);
        assert_eq!(reopened.library.as_ref().unwrap().journal().seq(), final_seq);
        if reset_fails {
            assert!(reopened.verify_round_revert(id, &round).is_err());
            reopened.close_library().unwrap();
            drop(reopened);
            let mut clean = open(&catalog, &files);
            assert!(clean.verify_round_revert(id, &round).is_err());
            assert_eq!(clean.develop_of(id).unwrap().light.exposure, 0.75);
        } else {
            let permit = reopened.verify_round_revert(id, &round).unwrap();
            reopened.commit_verified_round_revert(permit).unwrap();
            assert_eq!(reopened.develop_of(id).unwrap(), start);
        }
    }
}

#[test]
fn unopened_cold_round_survives_another_photos_normal_save_and_close() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files);
    let a = s.active().unwrap();
    let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
    let start = s.develop_of(a).unwrap();
    let round = publish(&mut s, a);
    s.close_library().unwrap();
    drop(s);
    let mut control = open(&catalog, &files);
    assert!(control.verify_round_revert(a, &round).is_ok());
    drop(control);
    let mut s = open(&catalog, &files);
    let result = s.develop_of(a).unwrap();
    let source = s.catalog.photo(a).unwrap().source.clone();
    // Never load A's AI history or prepare its revert in this session.
    s.execute("photo.rate", &json!({"ids":[b.0],"rating":3})).unwrap();
    s.compact_library().unwrap();
    s.close_library().unwrap();
    assert_eq!(s.develop_of(a).unwrap(), result);
    assert_eq!(s.catalog.photo(a).unwrap().source, source);
    drop(s);
    let mut s = open(&catalog, &files);
    let permit = s.verify_round_revert(a, &round).unwrap();
    s.commit_verified_round_revert(permit).unwrap();
    assert_eq!(s.develop_of(a).unwrap(), start);
    assert_eq!(s.catalog.photo(b).unwrap().rating, 3);
    assert!(s.companion_round(a, round.as_str()).unwrap().is_some());
}

struct ReadTrace {
    memory: MemStore,
    reads: Arc<std::sync::Mutex<Vec<String>>>,
}
impl Store for ReadTrace {
    fn read(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        self.reads.lock().unwrap().push(name.into());
        self.memory.read(name)
    }
    fn read_limited(&mut self, name: &str, limit: u64) -> io::Result<Option<Vec<u8>>> {
        self.reads.lock().unwrap().push(name.into());
        self.memory.read_limited(name, limit)
    }
    fn read_range(&mut self, name: &str, offset: u64, limit: usize) -> io::Result<Vec<u8>> {
        self.reads.lock().unwrap().push(name.into());
        self.memory.read_range(name, offset, limit)
    }
    fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.memory.write_atomic(name, bytes)
    }
    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.memory.append(name, bytes)
    }
    fn truncate(&mut self, name: &str, len: u64) -> io::Result<()> {
        self.memory.truncate(name, len)
    }
    fn remove(&mut self, name: &str) -> io::Result<()> {
        self.memory.remove(name)
    }
    fn describe(&self) -> String {
        self.memory.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
}
#[test]
fn unrelated_save_does_not_read_cold_authority_or_its_native_original() {
    let root = std::env::temp_dir().join(format!("lc-ux18-cold-source-{}", LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("original.png");
    let held = root.join("held.png");
    let image = lightcraft_raster::Rgba8::from_fn(8, 8, |x, y| [(x * 20) as u8, (y * 20) as u8, 60, 255]);
    let bytes = lightcraft_engine::export::encode_image(
        &image,
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files).with_fs();
    let a = s.active().unwrap();
    let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
    s.commit(
        "Native source",
        lightcraft_catalog::Op::SetFile {
            id: a,
            file_name: "original.png".into(),
            source: lightcraft_catalog::Source::File { path: path.to_string_lossy().into() },
        },
    )
    .unwrap();
    let start = s.develop_of(a).unwrap();
    let round = publish(&mut s, a);
    s.close_library().unwrap();
    drop(s);
    let reads = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut s = Session::new().with_fs();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog.clone()),
            files: Box::new(ReadTrace { memory: files.clone(), reads: reads.clone() }),
            dir: "trace".into(),
            on_disk: false,
        },
        false,
    )
    .unwrap();
    reads.lock().unwrap().clear();
    std::fs::rename(&path, &held).unwrap();
    s.execute("photo.rate", &json!({"ids":[b.0],"rating":3})).unwrap();
    s.close_library().unwrap();
    assert!(
        reads.lock().unwrap().iter().all(|name| !name.ends_with("/revert.json") && !name.contains("/revert-associations/")),
        "B saves must not scan or hydrate cold candidates"
    );
    drop(s);
    std::fs::rename(&held, &path).unwrap();
    let mut s = open(&catalog, &files).with_fs();
    let permit = s.verify_round_revert(a, &round).unwrap();
    s.commit_verified_round_revert(permit).unwrap();
    assert_eq!(s.develop_of(a).unwrap(), start);
    assert_eq!(s.catalog.photo(b).unwrap().rating, 3);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

struct RemoveFault<S: Store> {
    memory: S,
    fail: Arc<AtomicBool>,
    tripped: Arc<AtomicBool>,
    suffix: &'static str,
}
impl<S: Store> Store for RemoveFault<S> {
    fn read(&mut self, n: &str) -> io::Result<Option<Vec<u8>>> {
        self.memory.read(n)
    }
    fn read_limited(&mut self, n: &str, l: u64) -> io::Result<Option<Vec<u8>>> {
        self.memory.read_limited(n, l)
    }
    fn read_range(&mut self, n: &str, o: u64, l: usize) -> io::Result<Vec<u8>> {
        self.memory.read_range(n, o, l)
    }
    fn write_atomic(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        self.memory.write_atomic(n, b)
    }
    fn append(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        self.memory.append(n, b)
    }
    fn truncate(&mut self, n: &str, l: u64) -> io::Result<()> {
        self.memory.truncate(n, l)
    }
    fn remove(&mut self, n: &str) -> io::Result<()> {
        self.memory.remove(n)?;
        if n.ends_with(self.suffix) && self.fail.load(Ordering::Relaxed) {
            self.tripped.store(true, Ordering::Relaxed);
            return Err(io::Error::other("directory sync failed after visible marker removal"));
        }
        Ok(())
    }
    fn describe(&self) -> String {
        self.memory.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
}
#[test]
fn acknowledged_certificate_survives_visible_seal_cleanup_failure_as_saved_with_cleanup_debt() {
    let root = std::env::temp_dir().join(format!("lc-ux18-remove-seal-{}", LocalRoundId::new().unwrap().as_str()));
    let fail = Arc::new(AtomicBool::new(false));
    let tripped = Arc::new(AtomicBool::new(false));
    let mut s = Session::new().with_fs();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(lightcraft_catalog::FsStore::open(&root).unwrap()),
            files: Box::new(RemoveFault {
                memory: lightcraft_catalog::FsStore::open(&root).unwrap(),
                fail: fail.clone(),
                tripped: tripped.clone(),
                suffix: "/seal-pending.json",
            }),
            dir: root.clone(),
            on_disk: true,
        },
        true,
    )
    .unwrap();
    let a = s.active().unwrap();
    let round = publish(&mut s, a);
    let before = serde_json::to_value(&s.catalog).unwrap();
    let labels = s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>();
    fail.store(true, Ordering::Relaxed);
    let saved = s.persist();
    assert!(tripped.load(Ordering::Relaxed), "exercise the actual native post-removal error");
    assert!(saved.is_ok(), "substantive certificate commit is acknowledged; only cleanup failed: {saved:?}");
    assert!(s.unsaved().is_none());
    assert!(!s.companion_photo_unsaved(a));
    assert!(s.companion_photo_cleanup_pending(a));
    assert!(s.companion_cleanup_error(a).is_some());
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
    let mut cold = Session::new().with_fs();
    cold.open_library(&root, false).unwrap();
    assert!(cold.verify_round_revert(a, &round).is_ok());
    assert!(cold.companion_round(a, round.as_str()).unwrap().is_some());
    drop(cold);
    fail.store(false, Ordering::Relaxed);
    s.persist().unwrap();
    assert!(s.companion_cleanup_error(a).is_none());
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn acknowledged_final_close_survives_visible_fence_cleanup_failure_as_saved_with_cleanup_debt() {
    let root = std::env::temp_dir().join(format!("lc-ux18-remove-close-{}", LocalRoundId::new().unwrap().as_str()));
    let fail = Arc::new(AtomicBool::new(false));
    let tripped = Arc::new(AtomicBool::new(false));
    let mut s = Session::new().with_fs();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(lightcraft_catalog::FsStore::open(&root).unwrap()),
            files: Box::new(RemoveFault {
                memory: lightcraft_catalog::FsStore::open(&root).unwrap(),
                fail: fail.clone(),
                tripped: tripped.clone(),
                suffix: "companion/v2/close-proof.json",
            }),
            dir: root.clone(),
            on_disk: true,
        },
        true,
    )
    .unwrap();
    let a = s.active().unwrap();
    let round = publish(&mut s, a);
    s.persist().unwrap();
    let before = serde_json::to_value(&s.catalog).unwrap();
    let labels = s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>();
    fail.store(true, Ordering::Relaxed);
    let closed = s.close_library();
    assert!(tripped.load(Ordering::Relaxed), "exercise native returned failure after visible fence removal");
    assert!(closed.is_ok(), "final snapshot/data/certificate commits succeeded; only cleanup failed: {closed:?}");
    assert!(s.unsaved().is_none());
    assert!(!s.companion_photo_unsaved(a));
    assert!(s.companion_photo_cleanup_pending(a));
    assert!(s.companion_cleanup_error(a).is_some());
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
    drop(s);
    let mut cold = Session::new().with_fs();
    cold.open_library(&root, false).unwrap();
    assert!(cold.verify_round_revert(a, &round).is_ok());
    assert!(cold.companion_round(a, round.as_str()).unwrap().is_some());
    assert!(cold.companion_cleanup_pending(), "owning namespace retains cleanup across archive drop");
    cold.persist().unwrap();
    assert!(cold.companion_cleanup_error(a).is_none());
    assert_eq!(serde_json::to_value(&cold.catalog).unwrap(), before);
    drop(cold);
    std::fs::remove_dir_all(root).unwrap();
}

struct CoreProofFault {
    memory: MemStore,
    fail: Arc<AtomicBool>,
    fail_read: Arc<AtomicBool>,
    tripped: Arc<AtomicBool>,
}
impl Store for CoreProofFault {
    fn read(&mut self, n: &str) -> io::Result<Option<Vec<u8>>> {
        self.memory.read(n)
    }
    fn read_limited(&mut self, n: &str, l: u64) -> io::Result<Option<Vec<u8>>> {
        if n == "companion/v2/core-proof-pending.json" && self.fail_read.load(Ordering::Relaxed) {
            self.tripped.store(true, Ordering::Relaxed);
            return Err(io::Error::other("continuity intent read failed"));
        }
        self.memory.read_limited(n, l)
    }
    fn read_range(&mut self, n: &str, o: u64, l: usize) -> io::Result<Vec<u8>> {
        self.memory.read_range(n, o, l)
    }
    fn write_atomic(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        self.memory.write_atomic(n, b)?;
        if n == "companion/v2/core-proof.json" && self.fail.load(Ordering::Relaxed) {
            self.tripped.store(true, Ordering::Relaxed);
            return Err(io::Error::other("visible continuity write sync failure"));
        }
        Ok(())
    }
    fn append(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        self.memory.append(n, b)
    }
    fn truncate(&mut self, n: &str, l: u64) -> io::Result<()> {
        self.memory.truncate(n, l)
    }
    fn remove(&mut self, n: &str) -> io::Result<()> {
        self.memory.remove(n)
    }
    fn describe(&self) -> String {
        self.memory.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
}
#[test]
fn failed_continuity_commit_reports_pending_and_storage_retry_preserves_the_exact_cold_candidate() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files);
    let a = s.active().unwrap();
    let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
    let start = s.develop_of(a).unwrap();
    let round = publish(&mut s, a);
    s.close_library().unwrap();
    drop(s);
    let fail = Arc::new(AtomicBool::new(false));
    let tripped = Arc::new(AtomicBool::new(false));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog.clone()),
            files: Box::new(CoreProofFault {
                memory: files.clone(),
                fail: fail.clone(),
                fail_read: Arc::new(AtomicBool::new(false)),
                tripped: tripped.clone(),
            }),
            dir: "proof-fault".into(),
            on_disk: false,
        },
        false,
    )
    .unwrap();
    fail.store(true, Ordering::Relaxed);
    let _ = s.execute("photo.rate", &json!({"ids":[b.0],"rating":3}));
    assert!(tripped.load(Ordering::Relaxed));
    assert!(s.unsaved().is_some(), "an unacknowledged continuity write needs the existing local recovery outlet");
    assert!(s.companion_photo_unsaved(b));
    let before = serde_json::to_value(&s.catalog).unwrap();
    let labels = s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>();
    fail.store(false, Ordering::Relaxed);
    s.persist().unwrap();
    assert!(s.unsaved().is_none());
    assert!(!s.companion_photo_unsaved(b));
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
    s.close_library().unwrap();
    drop(s);
    let mut cold = open(&catalog, &files);
    let permit = cold.verify_round_revert(a, &round).unwrap();
    cold.commit_verified_round_revert(permit).unwrap();
    assert_eq!(cold.develop_of(a).unwrap(), start);
    assert_eq!(cold.catalog.photo(b).unwrap().rating, 3);
}

#[test]
fn continuity_intent_read_failure_reports_pending_until_storage_only_retry() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = open(&catalog, &files);
    let a = s.active().unwrap();
    let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
    let start = s.develop_of(a).unwrap();
    let round = publish(&mut s, a);
    s.close_library().unwrap();
    drop(s);
    let fail_read = Arc::new(AtomicBool::new(false));
    let tripped = Arc::new(AtomicBool::new(false));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog.clone()),
            files: Box::new(CoreProofFault {
                memory: files.clone(),
                fail: Arc::new(AtomicBool::new(false)),
                fail_read: fail_read.clone(),
                tripped: tripped.clone(),
            }),
            dir: "continuity-read-fault".into(),
            on_disk: false,
        },
        false,
    )
    .unwrap();
    fail_read.store(true, Ordering::Relaxed);
    let _ = s.execute("photo.rate", &json!({"ids":[b.0],"rating":4}));
    assert!(tripped.load(Ordering::Relaxed));
    assert!(s.unsaved().is_some(), "an unreadable owning continuity intent must not report Saved");
    assert!(s.companion_photo_unsaved(b));
    let before = serde_json::to_value(&s.catalog).unwrap();
    let labels = s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>();
    fail_read.store(false, Ordering::Relaxed);
    s.persist().unwrap();
    assert!(s.unsaved().is_none());
    assert!(!s.companion_photo_unsaved(b));
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|e| e.label.clone()).collect::<Vec<_>>(), labels);
    s.close_library().unwrap();
    drop(s);
    let mut cold = open(&catalog, &files);
    let permit = cold.verify_round_revert(a, &round).unwrap();
    cold.commit_verified_round_revert(permit).unwrap();
    assert_eq!(cold.develop_of(a).unwrap(), start);
    assert_eq!(cold.catalog.photo(b).unwrap().rating, 4);
}

#[test]
fn one_round_reuses_its_owned_association_across_failed_sealing_and_unrelated_saves() {
    for visible in [false, true] {
        let (catalog, files) = (MemStore::new(), MemStore::new());
        let fail = Arc::new(AtomicBool::new(false));
        let mut s = Session::new();
        s.open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FaultStore { memory: files.clone(), fail: fail.clone(), visible }),
                dir: "association-owner".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
        let a = s.active().unwrap();
        let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
        let start = s.develop_of(a).unwrap();
        let round = publish(&mut s, a);
        let association_files = || files.files.lock().unwrap().keys().filter(|key| key.starts_with("companion/v2/revert-associations/")).count();
        fail.store(true, Ordering::Relaxed);
        s.execute("photo.rate", &json!({"ids":[b.0],"rating":3})).unwrap();
        for _ in 0..8 {
            assert!(s.persist().is_err());
            assert_eq!(association_files(), 1, "retry one logical publication without accumulating untracked copies");
            assert!(s.companion_photo_unsaved(a));
        }
        fail.store(false, Ordering::Relaxed);
        s.persist().unwrap();
        for index in 0..8 {
            s.execute("photo.rate", &json!({"ids":[b.0],"rating":index%5})).unwrap();
            s.persist().unwrap();
            assert_eq!(association_files(), 1, "ordinary metadata saves must reuse the owning round association");
        }
        for _ in 0..64 {
            if !s.collect_companion(a, 32).unwrap().pending {
                break;
            }
        }
        assert_eq!(association_files(), 1);
        s.close_library().unwrap();
        drop(s);
        let mut cold = open(&catalog, &files);
        let permit = cold.verify_round_revert(a, &round).unwrap();
        cold.commit_verified_round_revert(permit).unwrap();
        assert_eq!(cold.develop_of(a).unwrap(), start);
        assert_eq!(cold.catalog.photo(b).unwrap().rating, 2);
    }
}
