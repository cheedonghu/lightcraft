//! Library validation precedes cancellation; both Open and New Library archive active rounds.
use super::*;

#[test]
fn invalid_library_keeps_the_round_and_new_library_archives_it() {
    let dir = std::env::temp_dir().join(format!("lc-companion-library-sync-{}", std::process::id()));
    let mut session = lightcraft_engine::Session::new().with_fs();
    let old = dir.join("old");
    session.open_library(&old, true).unwrap();
    let photo = session.catalog.photos().next().unwrap().id;
    session.execute("library.select", &json!({"ids":[photo.0],"active":photo.0})).unwrap();
    let mut app = LightcraftApp::new(session, crate::Services::default());
    let settings = app.session.develop_of(photo).unwrap();
    let generation = app.session.library_generation();
    let cancel = Arc::new(AtomicBool::new(false));
    app.ai.generation = generation;
    app.ai.conversations.insert(
        photo,
        Conversation {
            messages: vec![
                json!({"role":"user","text":"keep this conversation"}),
                json!({"role":"assistant","tool_calls":[{"id":"pending","name":"execute"}]}),
            ],
            ..Default::default()
        },
    );
    app.ai.jobs.insert(
        photo,
        Job {
            draft_revision: 0,
            lease: app.session.acquire_companion_lease(photo, LocalRoundId::new().unwrap()).unwrap(),
            identity: RoundIdentity {
                scope: PhotoScope { library_generation: generation, photo },
                local_round_id: LocalRoundId::new().unwrap(),
                submission_seq: 1,
                connection_generation: 0,
            },
            round: "round".into(),
            state: "awaiting_initial_execution".into(),
            prompt: "brighten".into(),
            user_message: 0,
            history: vec![],
            history_preflight: false,
            checkpoint: Checkpoint {
                photo,
                generation,
                original: settings.clone(),
                expected: settings,
                revision: app.session.photo_revision(photo),
                content: lightcraft_engine::media::content_key(app.session.catalog.photo(photo).unwrap()),
                eligible: true,
                local_round_id: None,
            },
            cancel: cancel.clone(),
            snapshot: None,
            plans: BTreeMap::new(),
            stopped: false,
            inflight_plan: None,
        },
    );
    let invalid = dir.join("file");
    std::fs::write(&invalid, b"not a library").unwrap();
    assert!(app.run("app.openLibrary", json!({"path":invalid.to_string_lossy()})).is_err());
    assert_eq!(app.session.library_generation(), generation);
    assert!(app.ai.is_busy(photo));
    assert!(!cancel.load(Ordering::Relaxed));
    let locked = dir.join("locked");
    let mut other_session = lightcraft_engine::Session::new().with_fs();
    other_session.open_library(&locked, false).unwrap();
    other_session.close_library().unwrap();
    drop(other_session);
    let held = lightcraft_catalog::LibraryLock::acquire(&locked, "other session").unwrap();
    assert!(app.run("app.openLibrary", json!({"path":locked.to_string_lossy()})).is_err());
    assert!(app.ai.is_busy(photo));
    assert!(!cancel.load(Ordering::Relaxed));
    drop(held);
    app.run("app.newLibrary", json!({"path":dir.to_string_lossy(),"name":"new"})).unwrap();
    assert!(cancel.load(Ordering::Relaxed));
    assert!(!app.ai.is_busy(photo));
    assert_ne!(app.session.library_generation(), generation);
    app.run("app.openLibrary", json!({"path":old.to_string_lossy()})).unwrap();
    let archive = app.session.load_companion_conversation(photo).unwrap().unwrap();
    assert_eq!(archive["messages"][0]["text"], "keep this conversation");
    assert!(archive["messages"].as_array().unwrap().iter().any(|m| m["role"] == "tool" && m["tool_call_id"] == "pending"));
    app.session.close_library().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

use crate::{LightcraftApp, Services, headless::Headless};
use lightcraft_catalog::{MemStore, PhotoId};
use lightcraft_engine::{
    Session,
    companion::{EditorState, storage::HistoryCursor},
    library::LibraryStores,
};
use serde_json::json;
use std::time::{Duration, Instant};
fn bottom_library_fixture(name: &str) -> (Session, MemStore, MemStore, PhotoId) {
    let core = MemStore::new();
    let files = MemStore::new();
    let mut s = Session::new();
    s.open_library_in(LibraryStores { catalog: Box::new(core.clone()), files: Box::new(files.clone()), dir: name.into(), on_disk: false }, true)
        .unwrap();
    let p = s.active().unwrap();
    s.save_companion_tail(p,None,&[json!({"role":"assistant","local_message_id":format!("{name}-giant"),"text":format!("{}END_{name}","readable sentence α🙂.\n".repeat(30000))})],&[],&[],&json!({"schema_version":2})).unwrap();
    let head = s.companion_photo_header(p).unwrap().unwrap();
    s.set_companion_editor(
        p,
        EditorState {
            draft: format!("draft-{name}"),
            follow_tail: false,
            reading_cursor: Some(HistoryCursor { record: 0, offset: 0 }),
            reading_epoch: Some(head.conversation_epoch),
            reading_count: Some(head.message_count),
            ..Default::default()
        },
    )
    .unwrap();
    s.persist().unwrap();
    (s, core, files, p)
}
fn settle_library_history(h: &mut Headless) {
    let start = Instant::now();
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historyLoading"] == false {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(15));
        std::thread::yield_now();
    }
    for _ in 0..3 {
        h.step();
    }
}
#[test]
fn library_switch_drops_an_old_bottom_request_before_restoring_equal_photo_reading() {
    for pending in [false, true] {
        let (a, _, _, pa) = bottom_library_fixture("A");
        let (mut b, core, files, pb) = bottom_library_fixture("B");
        assert_eq!(pa, pb);
        b.close_library().unwrap();
        drop(b);
        let mut h = Headless::new(LightcraftApp::new(a, Services::default()), [1400.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        settle_library_history(&mut h);
        if pending {
            h.app.run("ai.bottom", json!({})).unwrap();
        }
        h.app
            .session
            .open_library_in(LibraryStores { catalog: Box::new(core), files: Box::new(files), dir: "B".into(), on_disk: false }, true)
            .unwrap();
        // First real frame owns generation transition: no AI command before polling.
        h.step();
        settle_library_history(&mut h);
        let ed = h.app.session.companion_editor(pb).unwrap();
        assert_eq!(ed.draft, "draft-B");
        assert_eq!(ed.reading_cursor, Some(HistoryCursor { record: 0, offset: 0 }), "A's pending bottom continuation leaked to B");
    }
}
