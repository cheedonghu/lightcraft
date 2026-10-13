use super::{LocalRoundId, PhotoScope, RoundIdentity};
use crate::{Session, library::LibraryStores};
use lightcraft_catalog::{MemStore, PhotoId, Store};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct FaultPlan(Arc<Mutex<Option<(String, bool, bool)>>>);
#[derive(Clone)]
struct FaultStore<S = MemStore> {
    memory: S,
    plan: FaultPlan,
}
impl<S> FaultStore<S> {
    fn fault(&self, key: &str, removal: bool) -> Option<bool> {
        let mut plan = self.plan.0.lock().unwrap();
        if plan.as_ref().is_some_and(|(part, _, remove)| *remove == removal && key.contains(part)) {
            plan.take().map(|(_, after, _)| after)
        } else {
            None
        }
    }
}
impl<S: Store> Store for FaultStore<S> {
    fn read(&mut self, k: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.memory.read(k)
    }
    fn read_limited(&mut self, k: &str, n: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.memory.read_limited(k, n)
    }
    fn read_range(&mut self, k: &str, o: u64, n: usize) -> std::io::Result<Vec<u8>> {
        self.memory.read_range(k, o, n)
    }
    fn write_atomic(&mut self, k: &str, b: &[u8]) -> std::io::Result<()> {
        match self.fault(k, false) {
            Some(false) => Err(std::io::Error::other("fixture storage unavailable before write")),
            Some(true) => {
                self.memory.write_atomic(k, b)?;
                Err(std::io::Error::other("fixture sync unavailable after write"))
            }
            None => self.memory.write_atomic(k, b),
        }
    }
    fn remove(&mut self, k: &str) -> std::io::Result<()> {
        match self.fault(k, true) {
            Some(false) => Err(std::io::Error::other("fixture storage unavailable before remove")),
            Some(true) => {
                self.memory.remove(k)?;
                Err(std::io::Error::other("fixture sync unavailable after remove"))
            }
            None => self.memory.remove(k),
        }
    }
    fn append(&mut self, k: &str, b: &[u8]) -> std::io::Result<()> {
        self.memory.append(k, b)
    }
    fn truncate(&mut self, k: &str, n: u64) -> std::io::Result<()> {
        self.memory.truncate(k, n)
    }
    fn describe(&self) -> String {
        "UX09 fault memory".into()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
}
fn open(files: impl Store + 'static) -> Session {
    let mut s = Session::new();
    s.open_library_in(LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(files), dir: "ux09".into(), on_disk: false }, true)
        .unwrap();
    s
}

#[derive(Default)]
struct IoCount {
    calls: usize,
    largest_read: u64,
    unbounded_reads: usize,
}
struct MeterStore {
    inner: FaultStore,
    meter: Arc<Mutex<IoCount>>,
}
impl Store for MeterStore {
    fn read(&mut self, key: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.meter.lock().unwrap().unbounded_reads += 1;
        self.inner.read(key)
    }
    fn read_limited(&mut self, key: &str, limit: u64) -> std::io::Result<Option<Vec<u8>>> {
        let mut count = self.meter.lock().unwrap();
        count.calls += 1;
        count.largest_read = count.largest_read.max(limit);
        drop(count);
        self.inner.read_limited(key, limit)
    }
    fn read_range(&mut self, key: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        let mut count = self.meter.lock().unwrap();
        count.calls += 1;
        count.largest_read = count.largest_read.max(length as u64);
        drop(count);
        self.inner.read_range(key, offset, length)
    }
    fn write_atomic(&mut self, key: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.meter.lock().unwrap().calls += 1;
        self.inner.write_atomic(key, bytes)
    }
    fn remove(&mut self, key: &str) -> std::io::Result<()> {
        self.meter.lock().unwrap().calls += 1;
        self.inner.remove(key)
    }
    fn append(&mut self, key: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.append(key, bytes)
    }
    fn truncate(&mut self, key: &str, length: u64) -> std::io::Result<()> {
        self.inner.truncate(key, length)
    }
    fn describe(&self) -> String {
        "UX09 measured memory filesystem".into()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.inner.retention_namespace()
    }
}

#[test]
fn abandoned_large_allocation_is_reclaimed_with_bounded_observed_io_after_a_smaller_retry() {
    let files = MemStore::new();
    let plan = FaultPlan::default();
    let meter = Arc::new(Mutex::new(IoCount::default()));
    let mut s = open(MeterStore { inner: FaultStore { memory: files.clone(), plan: plan.clone() }, meter: meter.clone() });
    let photo = s.active().unwrap();
    *plan.0.lock().unwrap() = Some(("/manifest-".into(), false, false));
    assert!(s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"x".repeat(9*1024*1024)}],"rounds":[]})).is_err());
    let pending = s.companion_pending(photo).unwrap().unwrap();
    // The small retry uses sequence 2 for its index, so blocks/<tx>-2.bin
    // belongs only to the abandoned large record, unlike its live metadata at 3.
    let abandoned_block = format!("companion/v2/photos/{}/blocks/{}-2.bin", photo.0, pending.transaction_id);
    let mut observer = files.clone();
    assert!(observer.read_limited(&abandoned_block, 4 * 1024 * 1024).unwrap().is_some());
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"small retry"}],"rounds":[]})).unwrap();
    assert_eq!(s.companion_photo_header(photo).unwrap().unwrap().transaction_id, pending.transaction_id);
    let mut done = false;
    for _ in 0..1000 {
        *meter.lock().unwrap() = IoCount::default();
        let report = s.collect_companion(photo, 1).unwrap();
        let count = meter.lock().unwrap();
        assert_eq!(count.unbounded_reads, 0, "collector never requests a whole record");
        assert!(count.largest_read <= 32 * 1024, "all verification reads remain small metadata/index reads");
        assert!(count.calls <= 40, "one work unit has a fixed I/O budget independent of abandoned record size");
        drop(count);
        if !report.pending {
            done = true;
            break;
        }
    }
    assert!(done);
    assert!(observer.read_limited(&abandoned_block, 4 * 1024 * 1024).unwrap().is_none(), "superseded highwater remains tracked for cleanup");
    assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "small retry");
}

#[test]
fn native_atomic_head_failures_keep_actual_photo_changes_and_recover_without_republishing() {
    for after in [false, true] {
        let dir = std::env::temp_dir().join(format!("lightcraft-ux09-native-fault-{}", LocalRoundId::new().unwrap().as_str()));
        let catalog = MemStore::new();
        let plan = FaultPlan::default();
        let mut s = Session::new();
        s.open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FaultStore { memory: lightcraft_catalog::FsStore::open(&dir).unwrap(), plan: plan.clone() }),
                dir: dir.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
        let photo = s.active().unwrap();
        s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"old native"}],"rounds":[]})).unwrap();
        s.execute("develop.set", &json!({"ids":[photo.0],"values":{"light.exposure":0.4}})).unwrap();
        *plan.0.lock().unwrap() = Some(("/HEAD.json".into(), after, false));
        assert!(s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"native result"}],"rounds":[]})).is_err());
        let transaction = s.companion_pending(photo).unwrap().unwrap().transaction_id;
        assert_eq!(s.develop_of(photo).unwrap().light.exposure, 0.4);
        assert!(s.companion_photo_unsaved(photo));
        s.persist().unwrap();
        assert_eq!(s.companion_photo_header(photo).unwrap().unwrap().transaction_id, transaction);
        assert_eq!(s.develop_of(photo).unwrap().light.exposure, 0.4);
        assert!(s.unsaved().is_none());
        s.close_library().unwrap();
        let mut reopened = Session::new();
        reopened
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(catalog.clone()),
                    files: Box::new(lightcraft_catalog::FsStore::open(&dir).unwrap()),
                    dir: dir.clone(),
                    on_disk: false,
                },
                false,
            )
            .unwrap();
        assert_eq!(reopened.develop_of(photo).unwrap().light.exposure, 0.4);
        assert_eq!(reopened.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "native result");
        reopened.close_library().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
fn receipt(s: &mut Session, photo: PhotoId) -> RoundIdentity {
    let id = RoundIdentity {
        scope: PhotoScope { library_generation: s.library_generation(), photo },
        local_round_id: LocalRoundId::new().unwrap(),
        submission_seq: 9,
        connection_generation: 3,
    };
    let record = json!({"local_round_id":id.local_round_id,"submission_seq":9,"connection_generation":3,"runtime_library_generation":id.scope.library_generation,"stages":[{"plan_id":"plan","execution_outcome":"succeeded","delivery_state":"pending"}]});
    s.save_companion_tail(photo, None, &[], &[], &[record], &json!({"schema_version":2})).unwrap();
    id
}
#[test]
fn receipt_retry_recovers_an_incomplete_data_write_with_its_original_reservation() {
    let plan = FaultPlan::default();
    let files = MemStore::new();
    let mut s = open(FaultStore { memory: files, plan: plan.clone() });
    let photo = s.active().unwrap();
    let id = receipt(&mut s, photo);
    *plan.0.lock().unwrap() = Some(("/blocks/".into(), false, false));
    assert!(s.confirm_companion_execution(&id, "plan").is_err());
    let tx = s.companion_pending(photo).unwrap().unwrap().transaction_id;
    assert!(s.confirm_companion_execution(&id, "plan").unwrap());
    assert_eq!(s.companion_photo_header(photo).unwrap().unwrap().transaction_id, tx);
    assert_eq!(s.companion_round(photo, id.local_round_id.as_str()).unwrap().unwrap()["backend_confirmed_plan"], "plan");
}

#[test]
fn replaced_conversations_reclaim_owned_data_in_bounded_steps_without_losing_current_history() {
    let mut s = open(MemStore::new());
    let photo = s.active().unwrap();
    for n in 0..16 {
        s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"role":"user","text":format!("saved{n}")}],"rounds":[]}))
            .unwrap();
    }
    let mut removed = 0;
    let mut done = false;
    for _ in 0..1000 {
        let report = s.collect_companion(photo, 4).unwrap();
        assert!(report.removed <= 4, "one reclamation step respects the caller budget");
        removed += report.removed;
        assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "saved15");
        if !report.pending {
            done = true;
            break;
        }
    }
    assert!(done, "bounded persisted work eventually finishes");
    assert!(removed >= 15, "superseded owned allocations are actually reclaimed");
}

#[cfg(unix)]
#[test]
fn native_reader_survives_reclamation_after_library_reopen_through_a_directory_alias() {
    let dir = std::env::temp_dir().join(format!("lightcraft-ux09-{}", LocalRoundId::new().unwrap().as_str()));
    let alias = dir.with_extension("alias");
    let catalog = MemStore::new();
    let mut s = Session::new();
    s.open_library_in(
        LibraryStores {
            catalog: Box::new(catalog.clone()),
            files: Box::new(lightcraft_catalog::FsStore::open(&dir).unwrap()),
            dir: dir.clone(),
            on_disk: false,
        },
        true,
    )
    .unwrap();
    let photo = s.active().unwrap();
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"captured old graph"}],"rounds":[]})).unwrap();
    let h = s.companion_photo_header(photo).unwrap().unwrap();
    let mut reader = s.companion_reader(&h).unwrap().unwrap();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        resume_rx.recv().unwrap();
        reader.messages(0, 1, 1024).unwrap().records[0]["text"].clone()
    });
    s.pin_companion_prefix(photo, None).unwrap();
    s.close_library().unwrap();
    std::os::unix::fs::symlink(&dir, &alias).unwrap();
    let mut reopened = Session::new();
    reopened
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog),
                files: Box::new(lightcraft_catalog::FsStore::open(&alias).unwrap()),
                dir: alias.clone(),
                on_disk: false,
            },
            false,
        )
        .unwrap();
    reopened.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"current graph"}],"rounds":[]})).unwrap();
    for _ in 0..1000 {
        if !reopened.collect_companion(photo, 8).unwrap().pending {
            break;
        }
    }
    resume_tx.send(()).unwrap();
    assert_eq!(worker.join().unwrap(), "captured old graph");
    for _ in 0..1000 {
        if !reopened.collect_companion(photo, 8).unwrap().pending {
            break;
        }
    }
    assert_eq!(reopened.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "current graph");
    reopened.close_library().unwrap();
    std::fs::remove_file(alias).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn activation_sync_failure_retains_the_same_transaction_until_retry_is_durable() {
    let plan = FaultPlan::default();
    let mut s = open(FaultStore { memory: MemStore::new(), plan: plan.clone() });
    let photo = s.active().unwrap();
    *plan.0.lock().unwrap() = Some(("companion/state.json".into(), true, false));
    assert!(s.save_companion_tail(photo, None, &[json!({"text":"actual result"})], &[], &[], &json!({"schema_version":2})).is_err());
    let pending = s.companion_pending(photo).unwrap().expect("activation failure must retain its durable reservation");
    *plan.0.lock().unwrap() = Some(("companion/state.json".into(), true, false));
    assert!(s.persist().is_err());
    assert!(s.unsaved().is_some(), "another activation failure must retain the logical retry payload");
    s.persist().unwrap();
    assert_eq!(s.companion_photo_header(photo).unwrap().unwrap().transaction_id, pending.transaction_id);
    assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "actual result");
    assert!(s.unsaved().is_none());
}

#[test]
fn ambiguous_head_retains_both_possible_graphs_until_durable_confirmation() {
    let plan = FaultPlan::default();
    let mut s = open(FaultStore { memory: MemStore::new(), plan: plan.clone() });
    let photo = s.active().unwrap();
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"old durable graph"}],"rounds":[]})).unwrap();
    let old = s.companion_photo_header(photo).unwrap().unwrap();
    *plan.0.lock().unwrap() = Some(("/HEAD.json".into(), true, false));
    assert!(s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"visible new graph"}],"rounds":[]})).is_err());
    for _ in 0..1000 {
        if !s.collect_companion(photo, 8).unwrap().pending {
            break;
        }
    }
    assert_eq!(s.companion_messages_bounded(&old, 0, 1, 1024).unwrap().records[0]["text"], "old durable graph");
    assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "visible new graph");
    s.persist().unwrap();
    assert!(s.unsaved().is_none());
}

#[test]
fn cleanup_failure_restarts_from_persisted_work_and_keeps_committed_history() {
    for after in [false, true] {
        let plan = FaultPlan::default();
        let files = MemStore::new();
        let mut s = open(FaultStore { memory: files.clone(), plan: plan.clone() });
        let photo = s.active().unwrap();
        for text in ["old", "current"] {
            s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":text}],"rounds":[]})).unwrap();
        }
        *plan.0.lock().unwrap() = Some(("/manifest-".into(), after, true));
        let mut failed = false;
        for _ in 0..100 {
            if s.collect_companion(photo, 1).is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed);
        assert!(s.unsaved().is_none(), "cleanup failure does not undo a successful save");
        assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "current");
        let mut reopened = open(FaultStore { memory: files, plan });
        let mut done = false;
        for _ in 0..1000 {
            if !reopened.collect_companion(photo, 4).unwrap().pending {
                done = true;
                break;
            }
        }
        assert!(done);
        assert_eq!(reopened.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "current");
    }
}

#[test]
fn stale_commit_receipt_cannot_remove_a_newer_pending_reservation() {
    let mut s = open(MemStore::new());
    let photo = s.active().unwrap();
    let old = s.companion_stage(photo, 0, &json!({"schema_version":2,"messages":[{"text":"old"}],"rounds":[]})).unwrap();
    s.companion_commit(&old).unwrap();
    let next = s.companion_stage(photo, 0, &json!({"schema_version":2,"messages":[{"text":"new"}],"rounds":[]})).unwrap();
    assert!(s.companion_commit(&old).is_err(), "an old receipt cannot consume a newer reservation");
    assert_eq!(s.companion_pending(photo).unwrap().unwrap().transaction_id, next.transaction_id);
    s.companion_commit(&next).unwrap();
    assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "new");
}

#[test]
fn shared_preview_survives_replacement_and_is_reclaimed_after_its_last_reference() {
    use base64ct::{Base64, Encoding};
    let image = lightcraft_raster::Rgba8::filled(8, 8, [120, 80, 40, 255]);
    let jpeg = crate::export::encode_image(
        &image,
        &crate::export::ExportOptions { format: crate::export::ExportFormat::Jpeg, quality: 75, ..Default::default() },
    )
    .unwrap();
    let mut s = open(MemStore::new());
    let photo = s.active().unwrap();
    s.save_companion_conversation(photo,json!({"schema_version":2,"messages":[{"text":"before","images":[{"mime_type":"image/jpeg","data":Base64::encode_string(&jpeg)}]}],"rounds":[]})).unwrap();
    let image = s.companion_messages_page(photo, 0, 1).unwrap().records[0]["images"][0].clone();
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"reused","images":[image]}],"rounds":[]})).unwrap();
    for _ in 0..1000 {
        if !s.collect_companion(photo, 8).unwrap().pending {
            break;
        }
    }
    assert_eq!(s.library.as_mut().unwrap().read_companion_preview(&image).unwrap(), jpeg);
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"no preview"}],"rounds":[]})).unwrap();
    for _ in 0..1000 {
        if !s.collect_companion(photo, 8).unwrap().pending {
            break;
        }
    }
    assert!(s.library.as_mut().unwrap().read_companion_preview(&image).is_err(), "last unreferenced owned preview must eventually be reclaimed");
    assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "no preview");
}

#[test]
fn unknown_cleanup_checkpoint_is_protected_and_does_not_block_other_photo_saving() {
    let mut files = MemStore::new();
    let mut s = open(files.clone());
    let ids: Vec<_> = s.catalog.photos().take(2).map(|p| p.id).collect();
    let photo = ids[0];
    for text in ["old", "current"] {
        s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":text}],"rounds":[]})).unwrap();
    }
    s.collect_companion(photo, 1).unwrap();
    let key = format!("companion/v2/photos/{}/gc/work.json", photo.0);
    let mut checkpoint: serde_json::Value = serde_json::from_slice(&files.read_limited(&key, 32768).unwrap().unwrap()).unwrap();
    checkpoint["format"] = json!("future-retirement-format");
    files.write_atomic(&key, &serde_json::to_vec(&checkpoint).unwrap()).unwrap();
    assert!(s.collect_companion(photo, 8).is_err(), "unknown cleanup formats must be protected");
    assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "current");
    s.save_companion_tail(ids[1], None, &[json!({"text":"independent B"})], &[], &[], &json!({"schema_version":2})).unwrap();
    assert_eq!(s.companion_messages_page(ids[1], 0, 1).unwrap().records[0]["text"], "independent B");
}

#[test]
fn reopened_incomplete_receipt_is_finished_before_a_new_tail_without_reexecution() {
    let plan = FaultPlan::default();
    let files = MemStore::new();
    let mut s = open(FaultStore { memory: files.clone(), plan: plan.clone() });
    let photo = s.active().unwrap();
    let id = receipt(&mut s, photo);
    *plan.0.lock().unwrap() = Some(("/blocks/".into(), false, false));
    assert!(s.confirm_companion_execution(&id, "plan").is_err());
    drop(s);
    let mut reopened = open(files);
    let prefix = reopened.companion_photo_header(photo).unwrap().unwrap();
    let before = reopened.develop_of(photo).unwrap().clone();
    reopened
        .save_companion_tail(photo, Some(&prefix), &[json!({"role":"user","text":"next round"})], &[], &[], &json!({"schema_version":2}))
        .unwrap();
    assert_eq!(reopened.companion_round(photo, id.local_round_id.as_str()).unwrap().unwrap()["backend_confirmed_plan"], "plan");
    assert_eq!(reopened.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "next round");
    assert_eq!(*reopened.develop_of(photo).unwrap(), *before);
}

#[test]
fn ordinary_storage_retry_finishes_failed_receipts_without_another_transport_event() {
    let plan = FaultPlan::default();
    let mut s = open(FaultStore { memory: MemStore::new(), plan: plan.clone() });
    let photo = s.active().unwrap();
    let id = receipt(&mut s, photo);
    *plan.0.lock().unwrap() = Some(("/pending.json".into(), false, false));
    assert!(s.confirm_companion_execution(&id, "plan").is_err());
    assert!(s.unsaved().is_some(), "failed durable receipt is independently visible as unsaved");
    let before = s.develop_of(photo).unwrap().clone();
    s.persist().unwrap();
    assert_eq!(s.companion_round(photo, id.local_round_id.as_str()).unwrap().unwrap()["backend_confirmed_plan"], "plan");
    assert_eq!(*s.develop_of(photo).unwrap(), *before);
    assert!(s.unsaved().is_none());
}

#[test]
fn normal_session_persistence_progresses_retirement_without_a_manual_collector_call() {
    let mut s = open(MemStore::new());
    let photo = s.active().unwrap();
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"old"}],"rounds":[]})).unwrap();
    let old = s.companion_photo_header(photo).unwrap().unwrap();
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"current"}],"rounds":[]})).unwrap();
    for _ in 0..100 {
        s.persist().unwrap();
    }
    assert!(s.companion_messages_bounded(&old, 0, 1, 1024).is_err(), "ordinary persistence must progress tracked retirement");
    assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "current");
    assert!(s.unsaved().is_none());
}

#[test]
fn pending_cleanup_failure_is_retried_without_turning_saved_history_into_unsaved() {
    let plan = FaultPlan::default();
    let mut s = open(FaultStore { memory: MemStore::new(), plan: plan.clone() });
    let photo = s.active().unwrap();
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"old"}],"rounds":[]})).unwrap();
    let old = s.companion_photo_header(photo).unwrap().unwrap();
    *plan.0.lock().unwrap() = Some(("/pending.json".into(), false, true));
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"saved"}],"rounds":[]})).unwrap();
    assert!(s.unsaved().is_none());
    for _ in 0..100 {
        s.persist().unwrap();
    }
    assert!(s.companion_pending(photo).unwrap().is_none(), "committed pending cleanup debt must progress");
    assert!(s.companion_messages_bounded(&old, 0, 1, 1024).is_err());
    assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "saved");
}

#[test]
fn confirming_ambiguous_storage_after_a_manual_edit_does_not_renew_revert_eligibility() {
    let plan = FaultPlan::default();
    let mut s = open(FaultStore { memory: MemStore::new(), plan: plan.clone() });
    let photo = s.active().unwrap();
    let local = LocalRoundId::new().unwrap();
    let conversation =
        json!({"schema_version":2,"messages":[{"text":"actual result"}],"rounds":[],"checkpoint":{"eligible":true,"local_round_id":local}});
    s.save_companion_conversation(photo, conversation.clone()).unwrap();
    let mut next = conversation;
    next["messages"][0]["text"] = json!("latest complete actual result");
    *plan.0.lock().unwrap() = Some(("/HEAD.json".into(), true, false));
    assert!(s.save_companion_conversation(photo, next).is_err());
    let pending = s.companion_pending(photo).unwrap().unwrap();
    *plan.0.lock().unwrap() = Some(("/HEAD.json".into(), false, false));
    assert!(matches!(s.execute("develop.set", &json!({"ids":[photo.0],"values":{"light.exposure":0.5}})), Err(crate::EngineError::NotSaved(_))));
    assert_eq!(s.develop_of(photo).unwrap().light.exposure, 0.5);
    s.companion_commit(&pending).unwrap();
    assert_eq!(s.companion_metadata(photo).unwrap().unwrap()["checkpoint"]["eligible"], false);
    assert_eq!(s.develop_of(photo).unwrap().light.exposure, 0.5);
}

#[test]
fn resumable_main_frame_reader_retains_its_graph_until_the_scan_is_actually_dropped() {
    let mut s = open(MemStore::new());
    let photo = s.active().unwrap();
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"paused old scan"}],"rounds":[]})).unwrap();
    let old = s.companion_photo_header(photo).unwrap().unwrap();
    assert!(s.companion_reader(&old).unwrap().is_none());
    let retained = s.retain_companion_history(&old).unwrap();
    s.pin_companion_prefix(photo, None).unwrap();
    s.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"text":"current"}],"rounds":[]})).unwrap();
    for _ in 0..1000 {
        if !s.collect_companion(photo, 8).unwrap().pending {
            break;
        }
    }
    assert_eq!(s.companion_messages_bounded(&old, 0, 1, 1024).unwrap().records[0]["text"], "paused old scan");
    drop(retained);
    for _ in 0..1000 {
        if !s.collect_companion(photo, 8).unwrap().pending {
            break;
        }
    }
    assert!(s.companion_messages_bounded(&old, 0, 1, 1024).is_err());
    assert_eq!(s.companion_messages_page(photo, 0, 1).unwrap().records[0]["text"], "current");
}

#[test]
fn every_archive_write_failure_preserves_truth_and_independent_photo_then_retries_same_transaction() {
    use base64ct::{Base64, Encoding};
    let jpeg = crate::export::encode_image(
        &lightcraft_raster::Rgba8::filled(8, 8, [40, 90, 140, 255]),
        &crate::export::ExportOptions { format: crate::export::ExportFormat::Jpeg, quality: 75, ..Default::default() },
    )
    .unwrap();
    for stage in ["/pending.json", "/previews/", "/blocks/", "/pages/", "/manifest-", "/HEAD.json", "/revert.json", "/gc/owners/", "/gc/owners.json"]
    {
        for after in [false, true] {
            let plan = FaultPlan::default();
            let files = MemStore::new();
            let mut s = open(FaultStore { memory: files.clone(), plan: plan.clone() });
            let ids: Vec<_> = s.catalog.photos().take(2).map(|p| p.id).collect();
            let a = ids[0];
            let b = ids[1];
            s.save_companion_conversation(a, json!({"schema_version":2,"messages":[{"text":"old durable result"}],"rounds":[]})).unwrap();
            let prefix = format!("companion/v2/photos/{}", a.0);
            *plan.0.lock().unwrap() = Some((format!("{prefix}{stage}"), after, false));
            let actual = json!({"schema_version":2,"messages":[{"text":"actual complete result","images":[{"mime_type":"image/jpeg","data":Base64::encode_string(&jpeg)}]}],"rounds":[]});
            assert!(s.save_companion_conversation(a, actual).is_err(), "fault must be exercised: {stage} after={after}");
            assert_eq!(s.load_companion_conversation(a).unwrap().unwrap()["messages"][0]["text"], "actual complete result");
            let transaction = s.companion_pending(a).unwrap().map(|p| p.transaction_id);
            let visible = s.companion_messages_page(a, 0, 1).unwrap().records[0]["text"].clone();
            assert!(visible == "old durable result" || visible == "actual complete result");
            s.save_companion_tail(b, None, &[json!({"text":"independent B"})], &[], &[], &json!({"schema_version":2})).unwrap();
            assert_eq!(s.companion_messages_page(b, 0, 1).unwrap().records[0]["text"], "independent B");
            assert!(s.unsaved().is_some());
            s.persist().unwrap();
            if let Some(transaction) = transaction {
                assert_eq!(s.companion_photo_header(a).unwrap().unwrap().transaction_id, transaction, "same reservation at {stage} after={after}");
            }
            let page = s.companion_messages_page(a, 0, 1).unwrap();
            assert_eq!(page.records.len(), 1);
            assert_eq!(page.records[0]["text"], "actual complete result");
            assert_eq!(s.library.as_mut().unwrap().read_companion_preview(&page.records[0]["images"][0]).unwrap(), jpeg);
            assert!(s.unsaved().is_none());
            for _ in 0..1000 {
                if !s.collect_companion(a, 8).unwrap().pending {
                    break;
                }
            }
            let mut reopened = open(files);
            assert_eq!(reopened.companion_messages_page(a, 0, 1).unwrap().records[0]["text"], "actual complete result");
            assert_eq!(reopened.companion_messages_page(b, 0, 1).unwrap().records[0]["text"], "independent B");
        }
    }
}
