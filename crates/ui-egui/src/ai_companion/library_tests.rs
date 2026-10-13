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
            checkpoint: Checkpoint {
                photo,
                generation,
                original: settings.clone(),
                expected: settings,
                revision: app.session.photo_revision(photo),
                content: lightcraft_engine::media::content_key(app.session.catalog.photo(photo).unwrap()),
                eligible: true,
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
    let archive = app.session.photo_companion(photo).unwrap().unwrap();
    assert_eq!(archive["messages"][0]["text"], "keep this conversation");
    assert!(archive["messages"].as_array().unwrap().iter().any(|m| m["role"] == "tool" && m["tool_call_id"] == "pending"));
    app.session.close_library().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
