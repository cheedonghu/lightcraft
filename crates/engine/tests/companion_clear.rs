use lightcraft_catalog::MemStore;
use lightcraft_engine::{
    Session,
    companion::{EditorState, LocalRoundId, PhotoScope},
    library::LibraryStores,
};
use serde_json::json;

#[test]
fn acknowledged_clear_retires_old_round_and_preserves_effect_history_and_draft_after_reopen() {
    let (catalog, files) = (MemStore::new(), MemStore::new());
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "clear-round".into(), on_disk: false },
        true,
    )
    .unwrap();
    let photo = s.active().unwrap();
    let start = s.develop_of(photo).unwrap();
    let round = LocalRoundId::new().unwrap();
    let token = s.acquire_companion_lease(photo, round.clone()).unwrap();
    let mut edit = (*start).clone();
    edit.light.exposure = 0.75;
    let op = s.develop_op(photo, edit, "AI edit plan").unwrap();
    s.commit_companion(&token, &round, s.photo_revision(photo), op).unwrap();
    drop(token);
    s.save_companion_conversation(photo,json!({"schema_version":2,"messages":[{"role":"user","text":"Old request"}],"rounds":[],"round_records":[{"local_round_id":round.as_str(),"original":start,"recent_complete_result":s.develop_of(photo).unwrap(),"complete_state":"complete"}]})).unwrap();
    s.set_companion_editor(photo, EditorState { draft: "Keep my unsent request".into(), ..Default::default() }).unwrap();
    s.persist().unwrap();
    let permit = s.verify_round_revert(photo, &round).unwrap();
    let before = serde_json::to_value(&s.catalog).unwrap();
    let undo = s.undo.iter().map(|entry| entry.label.clone()).collect::<Vec<_>>();
    let epoch = s.companion_photo_header(photo).unwrap().unwrap().conversation_epoch;
    let receipt = s.companion_clear(PhotoScope { photo, library_generation: s.library_generation() }, epoch).unwrap();
    assert_eq!(receipt.conversation_epoch, epoch + 1);
    assert!(s.companion_messages_page(photo, 0, 32).unwrap().records.is_empty());
    assert!(s.companion_round(photo, round.as_str()).unwrap().is_none());
    assert!(s.commit_verified_round_revert(permit).is_err());
    assert!(s.verify_round_revert(photo, &round).is_err());
    assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
    assert_eq!(s.undo.iter().map(|entry| entry.label.clone()).collect::<Vec<_>>(), undo);
    assert_eq!(s.companion_editor(photo).unwrap().draft, "Keep my unsent request");
    s.close_library().unwrap();
    drop(s);
    let mut cold = Session::new();
    cold.open_library_in(LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "clear-round".into(), on_disk: false }, false)
        .unwrap();
    assert_eq!(serde_json::to_value(&cold.catalog).unwrap(), before);
    assert_eq!(cold.companion_editor(photo).unwrap().draft, "Keep my unsent request");
    assert_eq!(cold.companion_photo_header(photo).unwrap().unwrap().conversation_epoch, epoch + 1);
    assert!(cold.companion_messages_page(photo, 0, 32).unwrap().records.is_empty());
    assert!(cold.verify_round_revert(photo, &round).is_err());
}

use lightcraft_catalog::Store;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
struct ClearHeadFault {
    memory: MemStore,
    fail: Arc<AtomicBool>,
    tripped: Arc<AtomicBool>,
    visible: bool,
}
impl Store for ClearHeadFault {
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
        if n.ends_with("/HEAD.json") && self.fail.load(Ordering::Relaxed) {
            self.tripped.store(true, Ordering::Relaxed);
            if self.visible {
                self.memory.write_atomic(n, b)?;
            }
            return Err(io::Error::other("clear HEAD acknowledgement failed"));
        }
        self.memory.write_atomic(n, b)
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
fn failed_clear_keeps_its_identity_and_rejects_late_old_tail_until_explicit_retry() {
    for visible in [false, true] {
        let (catalog, files) = (MemStore::new(), MemStore::new());
        let fail = Arc::new(AtomicBool::new(false));
        let tripped = Arc::new(AtomicBool::new(false));
        let mut s = Session::new();
        s.open_library_in(
            LibraryStores {
                catalog: Box::new(catalog),
                files: Box::new(ClearHeadFault { memory: files, fail: fail.clone(), tripped: tripped.clone(), visible }),
                dir: "clear-ack-fault".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
        let photo = s.active().unwrap();
        s.save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"old context"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
        s.set_companion_editor(photo, EditorState { draft: "visible unsent draft".into(), ..Default::default() }).unwrap();
        s.persist().unwrap();
        let prefix = s.companion_photo_header(photo).unwrap().unwrap();
        let scope = PhotoScope { photo, library_generation: s.library_generation() };
        let before = serde_json::to_value(&s.catalog).unwrap();
        fail.store(true, Ordering::Relaxed);
        assert!(s.companion_clear(scope, prefix.conversation_epoch).is_err());
        assert!(tripped.load(Ordering::Relaxed));
        let pending = s.companion_pending(photo).unwrap().unwrap();
        assert!(
            s.save_companion_tail(photo, Some(&prefix), &[json!({"role":"user","text":"late old callback"})], &[], &[], &json!({"schema_version":2}))
                .is_err()
        );
        fail.store(false, Ordering::Relaxed);
        let receipt = s.companion_clear(scope, prefix.conversation_epoch).expect("old-prefix callback cannot replace the pending clear");
        assert_eq!(receipt.transaction_id, pending.transaction_id);
        assert_eq!(receipt.conversation_epoch, prefix.conversation_epoch + 1);
        assert!(s.companion_messages_page(photo, 0, 32).unwrap().records.is_empty());
        s.persist().unwrap();
        assert!(s.companion_messages_page(photo, 0, 32).unwrap().records.is_empty(), "retry never resurrects old queued messages");
        assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
        assert_eq!(s.companion_editor(photo).unwrap().draft, "visible unsent draft");
    }
}

#[test]
fn clear_acknowledgement_failure_reports_unsaved_until_the_confirmed_transaction_retries() {
    for visible in [false, true] {
        let (catalog, files) = (MemStore::new(), MemStore::new());
        let fail = Arc::new(AtomicBool::new(false));
        let tripped = Arc::new(AtomicBool::new(false));
        let mut s = Session::new();
        s.open_library_in(
            LibraryStores {
                catalog: Box::new(catalog),
                files: Box::new(ClearHeadFault { memory: files, fail: fail.clone(), tripped: tripped.clone(), visible }),
                dir: "clear-feedback".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
        let photo = s.active().unwrap();
        s.save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"old request"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
        s.set_companion_editor(photo, EditorState { draft: "keep draft".into(), ..Default::default() }).unwrap();
        s.persist().unwrap();
        let epoch = s.companion_photo_header(photo).unwrap().unwrap().conversation_epoch;
        let scope = PhotoScope { photo, library_generation: s.library_generation() };
        let before = serde_json::to_value(&s.catalog).unwrap();
        fail.store(true, Ordering::Relaxed);
        assert!(s.companion_clear(scope, epoch).is_err());
        assert!(tripped.load(Ordering::Relaxed));
        assert!(s.unsaved().is_some(), "an unacknowledged clear is not Saved even without dirty old data");
        assert!(s.companion_photo_unsaved(photo));
        fail.store(false, Ordering::Relaxed);
        s.companion_clear(scope, epoch).unwrap();
        assert!(s.unsaved().is_none());
        assert!(!s.companion_photo_unsaved(photo));
        assert_eq!(serde_json::to_value(&s.catalog).unwrap(), before);
        assert_eq!(s.companion_editor(photo).unwrap().draft, "keep draft");
    }
}

#[test]
fn pending_clear_blocks_only_its_photos_new_lease_until_acknowledged_retry() {
    let fail = Arc::new(AtomicBool::new(false));
    let tripped = Arc::new(AtomicBool::new(false));
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(MemStore::new()),
            files: Box::new(ClearHeadFault { memory: MemStore::new(), fail: fail.clone(), tripped, visible: true }),
            dir: "clear-admission".into(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let photo = s.active().unwrap();
    let other = s.catalog.photos().find(|p| p.id != photo).unwrap().id;
    s.save_companion_conversation(
        photo,
        json!({"schema_version":2,"messages":[{"role":"user","text":"old context"}],"rounds":[],"round_records":[]}),
    )
    .unwrap();
    let epoch = s.companion_photo_header(photo).unwrap().unwrap().conversation_epoch;
    let scope = PhotoScope { photo, library_generation: s.library_generation() };
    fail.store(true, Ordering::Relaxed);
    assert!(s.companion_clear(scope, epoch).is_err());
    assert!(
        s.acquire_companion_lease(photo, LocalRoundId::new().unwrap()).is_err(),
        "unacknowledged context replacement must block a new submission"
    );
    let independent = s.acquire_companion_lease(other, LocalRoundId::new().unwrap()).unwrap();
    drop(independent);
    fail.store(false, Ordering::Relaxed);
    s.companion_clear(scope, epoch).unwrap();
    let fresh = s.acquire_companion_lease(photo, LocalRoundId::new().unwrap()).unwrap();
    drop(fresh);
    assert!(s.companion_messages_page(photo, 0, 32).unwrap().records.is_empty());
}

#[test]
fn reopened_unacknowledged_clear_stays_protected_until_its_explicit_storage_retry() {
    for visible in [false, true] {
        let (catalog, files) = (MemStore::new(), MemStore::new());
        let fail = Arc::new(AtomicBool::new(false));
        let tripped = Arc::new(AtomicBool::new(false));
        let mut s = Session::new();
        s.open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(ClearHeadFault { memory: files.clone(), fail: fail.clone(), tripped, visible }),
                dir: "cold-clear".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
        let photo = s.active().unwrap();
        s.save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[{"role":"user","text":"old request"}],"rounds":[],"round_records":[]}),
        )
        .unwrap();
        s.set_companion_editor(photo, EditorState { draft: "saved draft".into(), ..Default::default() }).unwrap();
        s.persist().unwrap();
        let epoch = s.companion_photo_header(photo).unwrap().unwrap().conversation_epoch;
        fail.store(true, Ordering::Relaxed);
        assert!(s.companion_clear(PhotoScope { photo, library_generation: s.library_generation() }, epoch).is_err());
        drop(s);
        let mut cold = Session::new();
        cold.open_library_in(LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "cold-clear".into(), on_disk: false }, false)
            .unwrap();
        assert!(cold.acquire_companion_lease(photo, LocalRoundId::new().unwrap()).is_err(), "cold opening cannot bypass the persisted clear intent");
        assert!(cold.unsaved().is_some());
        assert!(cold.companion_photo_unsaved(photo));
        let other = cold.catalog.photos().find(|p| p.id != photo).unwrap().id;
        let independent = cold.acquire_companion_lease(other, LocalRoundId::new().unwrap()).unwrap();
        drop(independent);
        cold.companion_clear(PhotoScope { photo, library_generation: cold.library_generation() }, epoch).unwrap();
        assert!(cold.unsaved().is_none());
        assert_eq!(cold.companion_editor(photo).unwrap().draft, "saved draft");
        assert!(cold.companion_messages_page(photo, 0, 32).unwrap().records.is_empty());
        let fresh = cold.acquire_companion_lease(photo, LocalRoundId::new().unwrap()).unwrap();
        drop(fresh);
    }
}
