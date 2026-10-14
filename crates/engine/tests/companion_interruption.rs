use lightcraft_catalog::{FsStore, MemStore, Store};
use lightcraft_engine::{
    Session,
    companion::{
        LocalRoundId,
        storage::{DisplayRecord, DisplayScan},
    },
    library::LibraryStores,
};
use serde_json::json;

struct Temporary(std::path::PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn large_verified_round_keeps_saved_activity_fields_on_native_and_bounded_fallback_reads() {
    for native in [false, true] {
        let directory = std::env::temp_dir().join(format!("lightcraft-ux15-record-{}", LocalRoundId::new().unwrap().as_str()));
        std::fs::create_dir(&directory).unwrap();
        let temporary = Temporary(directory);
        let mut session = Session::new();
        let files: Box<dyn Store> = if native { Box::new(FsStore::open(&temporary.0).unwrap()) } else { Box::new(MemStore::new()) };
        session.open_library_in(LibraryStores { catalog: Box::new(MemStore::new()), files, dir: temporary.0.clone(), on_disk: false }, true).unwrap();
        let photo = session.active().unwrap();
        let round = LocalRoundId::new().unwrap();
        session.save_companion_conversation(photo, json!({"schema_version":2,"messages":[],"rounds":[],"round_records":[{"local_round_id":round,"photo_id":photo.0,"request_text":"Verified saved activity","submission_seq":7,"connection_generation":9,"runtime_library_generation":session.library_generation(),"phase":"queued","stages":[],"opaque_extension":"x".repeat(2*1024*1024),"backend_terminal":{"opaque_extension":"x".repeat(2*1024*1024)}}]})).unwrap();
        let header = session.companion_photo_header(photo).unwrap().unwrap();
        let record = if native {
            let mut reader = session.companion_reader(&header).unwrap().unwrap();
            reader.rounds_display(0, 1).unwrap().records.remove(0)
        } else {
            let mut scan = DisplayScan::new(header, DisplayRecord::Round(0));
            let mut steps = 0;
            loop {
                steps += 1;
                if let Some(record) = session.companion_display_step(&mut scan).unwrap() {
                    break record;
                }
                assert!(steps < 32, "the bounded continuation must complete this verified record");
            }
        };
        assert_eq!(record["phase"], "queued", "saved activity must not disappear behind a selected-field projection");
        assert_eq!(record["request_text"], "Verified saved activity");
        assert_eq!(record["submission_seq"], 7);
        assert_eq!(record["connection_generation"], 9);
        assert!(record["runtime_library_generation"].as_u64().unwrap() > 0);
        assert!(record.get("backend_terminal").is_some(), "terminal presence must survive without a whole terminal payload buffer");
        assert!(serde_json::to_vec(&record).unwrap().len() < 64 * 1024);
        assert!(record.get("opaque_extension").is_none(), "additive data never becomes a retained display buffer");
    }
}
