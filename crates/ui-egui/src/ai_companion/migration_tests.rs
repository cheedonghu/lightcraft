use super::*;
use crate::headless::Headless;
use lightcraft_catalog::{FsStore, MemStore, SourceIdentity, SourceSnapshot, Store};
use lightcraft_engine::library::LibraryStores;
use std::io::Write;
use std::sync::{Condvar, Mutex};

#[test]
fn menu_exit_retry_waits_for_native_migration_and_preserves_each_latest_draft() {
    let directory = std::env::temp_dir().join(format!("lc-ux15-close-{}", LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir(&directory).unwrap();
    let catalog = MemStore::new();
    let mut original = lightcraft_engine::Session::new();
    original
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let first = original.active().unwrap();
    let second = original.catalog.photos().find(|p| p.id != first).unwrap().id;
    original.close_library().unwrap();
    drop(original);
    let source = serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{first.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"Original saved source"}],"rounds":[]}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic("companion/state.json", &source).unwrap();
    let gate = Arc::new((Mutex::new(Pause { released: true, pause_final: true, ..Default::default() }), Condvar::new()));
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(PausedStore { native: FsStore::open(&directory).unwrap(), gate: gate.clone() }),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1400.0, 1000.0], 1.0);
    h.app.ui.settings.gpu = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    while !gate.0.lock().unwrap().final_entered && Instant::now() < deadline {
        h.step();
        let _ = h.app.session.persist();
        std::thread::yield_now();
    }
    assert!(gate.0.lock().unwrap().final_entered);
    h.app.run("ai.open", json!({})).unwrap();
    for (photo, text) in [(first, "First latest unsent draft"), (second, "Second latest unsent draft 草稿")] {
        h.app.run("library.select", json!({"ids":[photo.0],"active":photo.0})).unwrap();
        h.step();
        h.step();
        assert_eq!(h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), Duration::from_secs(5))["ok"], true);
        assert_eq!(h.request("ui.text", json!({"text":text}), Duration::from_secs(5))["ok"], true);
    }
    let generation = h.app.session.library_generation();
    assert_eq!(h.request("ui.menu.invoke", json!({"command":"app.quit"}), Duration::from_secs(5))["ok"], true);
    assert!(!h.quit_requested(), "a live root publisher cannot be reported as a saved close");
    assert_eq!(h.app.session.library_generation(), generation);
    assert_eq!(h.app.session.active(), Some(second));
    assert!(files.read_limited("companion/state.json", 32 * 1024).unwrap().as_deref() == Some(source.as_slice()));
    gate.0.lock().unwrap().final_released = true;
    gate.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(15);
    while h.app.session.companion_migration_pending() && Instant::now() < deadline {
        h.step();
        let _ = h.app.session.persist();
        std::thread::yield_now();
    }
    assert!(!h.app.session.companion_migration_pending());
    h.step();
    h.step();
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:quitRetry"}), Duration::from_secs(5))["ok"], true);
    h.step();
    h.step();
    assert!(h.quit_requested());
    // The headless host exposes the permitted exit; the native host then calls
    // this same Session close before shutting down its renderer.
    h.app.session.close_library().unwrap();
    drop(h);
    let mut reopened = lightcraft_engine::Session::new();
    reopened
        .open_library_in(
            LibraryStores { catalog: Box::new(catalog), files: Box::new(FsStore::open(&directory).unwrap()), dir: directory.clone(), on_disk: false },
            true,
        )
        .unwrap();
    assert_eq!(reopened.companion_editor(first).unwrap().draft, "First latest unsent draft");
    assert_eq!(reopened.companion_editor(second).unwrap().draft, "Second latest unsent draft 草稿");
    reopened.close_library().unwrap();
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn committed_exit_cancels_a_paused_native_migration_before_root_publication() {
    let directory = std::env::temp_dir().join(format!("lc-ux15-exit-{}", LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir(&directory).unwrap();
    let catalog = MemStore::new();
    let mut original = lightcraft_engine::Session::new();
    original
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = original.active().unwrap();
    let settings = original.develop_of(photo).unwrap();
    original.close_library().unwrap();
    drop(original);
    let source = serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"Saved migration source ".repeat(50000)}],"rounds":[],"checkpoint":null}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic("companion/state.json", &source).unwrap();
    let gate = Arc::new((Mutex::new(Pause { released: true, pause_final: true, ..Default::default() }), Condvar::new()));
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog),
                files: Box::new(PausedStore { native: FsStore::open(&directory).unwrap(), gate: gate.clone() }),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1400.0, 1000.0], 1.0);
    h.app.ui.settings.gpu = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !gate.0.lock().unwrap().final_entered && Instant::now() < deadline {
        h.step();
        let _ = h.app.session.persist();
        std::thread::yield_now();
    }
    assert!(gate.0.lock().unwrap().final_entered, "actual native final publication must be paused");
    before_exit(&mut h.app);
    before_exit(&mut h.app);
    gate.0.lock().unwrap().final_released = true;
    gate.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.app.session.companion_migration_busy() && Instant::now() < deadline {
        h.step();
        std::thread::yield_now();
    }
    assert!(!h.app.session.companion_migration_busy(), "actual migration must retire after release");
    let preserved = files.read_limited("companion/state.json", 4 * 1024 * 1024).unwrap();
    assert!(preserved.as_deref() == Some(source.as_slice()), "committed exit revokes the old root publisher");
    assert_eq!(h.app.session.develop_of(photo).unwrap(), settings);
    drop(h);
    std::fs::remove_dir_all(directory).unwrap();
}
#[derive(Default)]
struct Pause {
    entered: bool,
    released: bool,
    pause_source: bool,
    pause_backup: bool,
    pause_final: bool,
    final_entered: bool,
    final_released: bool,
    fail_backup: bool,
    fail_final_visible: bool,
    owning_thread: Option<std::thread::ThreadId>,
    owning_source_bytes: usize,
}
// Observe actual Store source ranges without replacing migration logic.
struct ObservedSource {
    inner: Box<dyn SourceSnapshot>,
    gate: Arc<(Mutex<Pause>, Condvar)>,
}
impl SourceSnapshot for ObservedSource {
    fn identity(&self) -> &SourceIdentity {
        self.inner.identity()
    }
    fn read_range(&mut self, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        let mut gate = self.gate.0.lock().unwrap();
        if gate.owning_thread == Some(std::thread::current().id()) {
            gate.owning_source_bytes += length;
        }
        drop(gate);
        self.inner.read_range(offset, length)
    }
    fn verify_current(&mut self) -> std::io::Result<()> {
        self.inner.verify_current()
    }
    fn clone_reader(&self) -> std::io::Result<Box<dyn SourceSnapshot>> {
        Ok(Box::new(Self { inner: self.inner.clone_reader()?, gate: self.gate.clone() }))
    }
}
struct PausedStore {
    native: FsStore,
    gate: Arc<(Mutex<Pause>, Condvar)>,
}
impl Store for PausedStore {
    fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.native.read(name)
    }
    fn read_limited(&mut self, name: &str, limit: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.native.read_limited(name, limit)
    }
    fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.native.read_range(name, offset, length)
    }
    fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        if name.contains("/blocks/") {
            let (mutex, changed) = &*self.gate;
            let mut gate = mutex.lock().unwrap();
            gate.entered = true;
            changed.notify_all();
            while !gate.released {
                gate = changed.wait(gate).unwrap();
            }
        }
        self.native.write_atomic(name, bytes)
    }
    fn append(&mut self, name: &str, data: &[u8]) -> std::io::Result<()> {
        self.native.append(name, data)
    }
    fn truncate(&mut self, name: &str, length: u64) -> std::io::Result<()> {
        self.native.truncate(name, length)
    }
    fn remove(&mut self, name: &str) -> std::io::Result<()> {
        self.native.remove(name)
    }
    fn describe(&self) -> String {
        self.native.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.native.retention_namespace()
    }
    fn conservative_retention(&self) -> bool {
        self.native.conservative_retention()
    }
    fn source_snapshot(&mut self, name: &str) -> std::io::Result<Option<Box<dyn SourceSnapshot>>> {
        let (mutex, changed) = &*self.gate;
        let mut gate = mutex.lock().unwrap();
        if (gate.pause_source && name == "companion/state.json") || (gate.pause_backup && name == "companion/state.v1.backup.json") {
            gate.entered = true;
            changed.notify_all();
            while !gate.released {
                gate = changed.wait(gate).unwrap();
            }
        }
        drop(gate);
        Ok(self.native.source_snapshot(name)?.map(|inner| Box::new(ObservedSource { inner, gate: self.gate.clone() }) as Box<dyn SourceSnapshot>))
    }
    fn copy_source_atomic(&mut self, name: &str, source: &mut dyn SourceSnapshot) -> std::io::Result<()> {
        let mut gate = self.gate.0.lock().unwrap();
        if name == "companion/state.v1.backup.json" && gate.fail_backup {
            gate.fail_backup = false;
            return Err(std::io::Error::other("native backup destination failure"));
        }
        drop(gate);
        self.native.copy_source_atomic(name, source)
    }
    fn replace_source_atomic(&mut self, name: &str, expected: &SourceIdentity, bytes: &[u8]) -> std::io::Result<()> {
        let (mutex, changed) = &*self.gate;
        let mut gate = mutex.lock().unwrap();
        if gate.pause_final {
            gate.final_entered = true;
            changed.notify_all();
            while !gate.final_released {
                gate = changed.wait(gate).unwrap();
            }
        }
        drop(gate);
        self.native.replace_source_atomic(name, expected, bytes)
    }
    fn replace_source_atomic_authorized(
        &mut self,
        name: &str,
        expected: &SourceIdentity,
        bytes: &[u8],
        authorized: &dyn Fn() -> bool,
    ) -> std::io::Result<()> {
        let (mutex, changed) = &*self.gate;
        let mut gate = mutex.lock().unwrap();
        if gate.pause_final {
            gate.final_entered = true;
            changed.notify_all();
            while !gate.final_released {
                gate = changed.wait(gate).unwrap();
            }
        }
        drop(gate);
        self.native.replace_source_atomic_authorized(name, expected, bytes, authorized)?;
        let mut gate = self.gate.0.lock().unwrap();
        if gate.fail_final_visible {
            gate.fail_final_visible = false;
            return Err(std::io::Error::other("native visible root publication failure"));
        }
        Ok(())
    }
    fn background_writer(&self) -> Option<Box<dyn Store>> {
        Some(Box::new(Self { native: FsStore::open(self.native.dir()).ok()?, gate: self.gate.clone() }))
    }
}
#[test]
fn paused_native_migration_keeps_real_frames_photo_edits_and_latest_draft_live() {
    let directory = std::env::temp_dir().join(format!("lc-ux10-live-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut original = lightcraft_engine::Session::new();
    original
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = original.active().unwrap();
    original.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"legacy large source ".repeat(60000)}],"rounds":[],"checkpoint":null}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic("companion/state.json", &source).unwrap();
    let gate = Arc::new((Mutex::new(Pause { owning_thread: Some(std::thread::current().id()), ..Default::default() }), Condvar::new()));
    let gate_worker = gate.clone();
    let worker_dir = directory.clone();
    let worker_catalog = catalog.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    let opener = std::thread::spawn(move || {
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(worker_catalog),
                    files: Box::new(PausedStore { native: FsStore::open(&worker_dir).unwrap(), gate: gate_worker }),
                    dir: worker_dir,
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        sender.send(session).unwrap();
    });
    let opened = receiver.recv_timeout(Duration::from_secs(1));
    if opened.is_err() {
        let (mutex, changed) = &*gate;
        mutex.lock().unwrap().released = true;
        changed.notify_all();
        opener.join().unwrap();
        assert!(opened.is_ok(), "native migration must yield the owning Session while filesystem staging is paused");
        return;
    }
    let session = opened.unwrap();
    opener.join().unwrap();
    let (mutex, changed) = &*gate;
    let current = mutex.lock().unwrap();
    let (current, _) = changed.wait_timeout_while(current, Duration::from_secs(5), |gate| !gate.entered).unwrap();
    assert!(current.entered);
    drop(current);
    let mut headless = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1000.0, 680.0], 1.0);
    let before = Instant::now();
    headless.app.run("develop.set", json!({"ids":[photo.0],"values":{"light.exposure":0.5}})).unwrap();
    headless.step();
    assert!(before.elapsed() < Duration::from_secs(1), "ordinary control/frame waited for migration filesystem");
    headless.step();
    assert_eq!(files.read_limited("companion/state.json", 4 * 1024 * 1024).unwrap(), Some(source.clone()));
    mutex.lock().unwrap().owning_source_bytes = 0;
    headless.app.run("ai.open", json!({})).unwrap();
    headless.step();
    headless.step();
    assert_eq!(headless.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), Duration::from_secs(5))["ok"], true);
    assert_eq!(headless.request("ui.text", json!({"text":"latest live draft 草稿"}), Duration::from_secs(5))["ok"], true);
    let source_bytes = mutex.lock().unwrap().owning_source_bytes;
    mutex.lock().unwrap().released = true;
    changed.notify_all();
    assert_eq!(source_bytes, 0, "pre-stage panel/frame must not scan the original source on its owning thread");
    let deadline = Instant::now() + Duration::from_secs(20);
    while headless.app.session.companion_migration_pending() && Instant::now() < deadline {
        headless.step();
        let _ = headless.app.session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!headless.app.session.companion_migration_pending());
    assert_eq!(headless.app.session.companion_editor(photo).unwrap().draft, "latest live draft 草稿");
    assert_eq!(headless.app.session.develop_of(photo).unwrap().light.exposure, 0.5);
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 4 * 1024 * 1024).unwrap(), Some(source));
    headless.app.session.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn paused_final_source_publication_keeps_real_frames_and_latest_overlay_live() {
    let directory = std::env::temp_dir().join(format!("lc-ux10-final-live-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut original = lightcraft_engine::Session::new();
    original
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = original.active().unwrap();
    original.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"source for final proof ".repeat(50000)}],"rounds":[],"checkpoint":null}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic("companion/state.json", &source).unwrap();
    let gate = Arc::new((Mutex::new(Pause { released: true, pause_final: true, ..Default::default() }), Condvar::new()));
    let gate_ui = gate.clone();
    let ui_dir = directory.clone();
    let (sender, receiver) = mpsc::channel();
    let ui = std::thread::spawn(move || {
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(catalog),
                    files: Box::new(PausedStore { native: FsStore::open(&ui_dir).unwrap(), gate: gate_ui.clone() }),
                    dir: ui_dir,
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let mut headless = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1000.0, 680.0], 1.0);
        let mut updated = false;
        let deadline = Instant::now() + Duration::from_secs(20);
        while headless.app.session.companion_migration_pending() && Instant::now() < deadline {
            if !updated && gate_ui.0.lock().unwrap().final_entered {
                headless.app.run("develop.set", json!({"ids":[photo.0],"values":{"light.exposure":0.8}})).unwrap();
                headless
                    .app
                    .session
                    .set_companion_editor(
                        photo,
                        lightcraft_engine::companion::EditorState { draft: "latest after final proof paused".into(), ..Default::default() },
                    )
                    .unwrap();
                headless.step();
                sender.send(()).unwrap();
                updated = true;
            }
            headless.step();
            let _ = headless.app.session.persist();
            std::thread::sleep(Duration::from_millis(5));
        }
        let editor = headless.app.session.companion_editor(photo).unwrap();
        let exposure = headless.app.session.develop_of(photo).unwrap().light.exposure;
        headless.app.session.close_library().unwrap();
        (editor, exposure)
    });
    let (mutex, changed) = &*gate;
    let current = mutex.lock().unwrap();
    let (current, _) = changed.wait_timeout_while(current, Duration::from_secs(5), |p| !p.final_entered).unwrap();
    assert!(current.final_entered);
    drop(current);
    let advanced = receiver.recv_timeout(Duration::from_secs(1)).is_ok();
    assert_eq!(files.read_limited("companion/state.json", 4 * 1024 * 1024).unwrap(), Some(source.clone()));
    mutex.lock().unwrap().final_released = true;
    changed.notify_all();
    let (editor, exposure) = ui.join().unwrap();
    assert!(advanced, "fresh final source proof/publication blocked actual ordinary command and frame");
    assert_eq!(editor.draft, "latest after final proof paused");
    assert_eq!(exposure, 0.8);
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 4 * 1024 * 1024).unwrap(), Some(source));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn paused_native_source_capture_yields_session_and_actual_frames() {
    let directory = std::env::temp_dir().join(format!("lc-ux10-open-live-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut original = lightcraft_engine::Session::new();
    original
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = original.active().unwrap();
    original.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"legacy large source ".repeat(60000)}],"rounds":[],"checkpoint":null}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic("companion/state.json", &source).unwrap();
    let gate = Arc::new((Mutex::new(Pause { pause_source: true, ..Default::default() }), Condvar::new()));
    let gate_worker = gate.clone();
    let worker_dir = directory.clone();
    let worker_catalog = catalog.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    let opener = std::thread::spawn(move || {
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(worker_catalog),
                    files: Box::new(PausedStore { native: FsStore::open(&worker_dir).unwrap(), gate: gate_worker }),
                    dir: worker_dir,
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        sender.send(session).unwrap();
    });
    let opened = receiver.recv_timeout(Duration::from_secs(1));
    if opened.is_err() {
        let (mutex, changed) = &*gate;
        mutex.lock().unwrap().released = true;
        changed.notify_all();
        opener.join().unwrap();
        assert!(opened.is_ok(), "native source capture must yield the owning Session before the complete hash or scan");
        return;
    }
    let session = opened.unwrap();
    opener.join().unwrap();
    let (mutex, changed) = &*gate;
    let current = mutex.lock().unwrap();
    let (current, _) = changed.wait_timeout_while(current, Duration::from_secs(5), |gate| !gate.entered).unwrap();
    assert!(current.entered);
    drop(current);
    let mut headless = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1000.0, 680.0], 1.0);
    let before = Instant::now();
    headless.app.run("develop.set", json!({"ids":[photo.0],"values":{"light.exposure":0.5}})).unwrap();
    headless.step();
    assert!(before.elapsed() < Duration::from_secs(1), "ordinary control/frame waited for migration filesystem");
    headless
        .app
        .session
        .set_companion_editor(photo, lightcraft_engine::companion::EditorState { draft: "latest live draft 草稿".into(), ..Default::default() })
        .unwrap();
    headless.step();
    assert_eq!(files.read_limited("companion/state.json", 4 * 1024 * 1024).unwrap(), Some(source.clone()));
    mutex.lock().unwrap().released = true;
    changed.notify_all();
    let deadline = Instant::now() + Duration::from_secs(20);
    while headless.app.session.companion_migration_pending() && Instant::now() < deadline {
        headless.step();
        let _ = headless.app.session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!headless.app.session.companion_migration_pending());
    assert_eq!(headless.app.session.companion_editor(photo).unwrap().draft, "latest live draft 草稿");
    assert_eq!(headless.app.session.develop_of(photo).unwrap().light.exposure, 0.5);
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 4 * 1024 * 1024).unwrap(), Some(source));
    headless.app.session.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cancelled_a_publication_retires_without_touching_new_library_b() {
    let directory = std::env::temp_dir().join(format!("lc-ux10-generation-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut original = lightcraft_engine::Session::new();
    original
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = original.active().unwrap();
    original.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"cancel source ".repeat(50000)}],"rounds":[],"checkpoint":null}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic("companion/state.json", &source).unwrap();
    let gate = Arc::new((Mutex::new(Pause { released: true, pause_final: true, ..Default::default() }), Condvar::new()));
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog),
                files: Box::new(PausedStore { native: FsStore::open(&directory).unwrap(), gate: gate.clone() }),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1000., 680.], 1.);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !gate.0.lock().unwrap().final_entered && Instant::now() < deadline {
        h.step();
        let _ = h.app.session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(gate.0.lock().unwrap().final_entered);
    assert!(h.app.session.companion_migration_busy());
    let retirement = h.app.session.cancel_companion_migration().unwrap();
    assert!(!retirement.retired());
    let b_files = MemStore::new();
    h.app
        .session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(b_files.clone()), dir: "B-independent".into(), on_disk: false },
            true,
        )
        .unwrap();
    let b_photo = h.app.session.active().unwrap();
    h.app.run("develop.set", json!({"ids":[b_photo.0],"values":{"light.exposure":0.6}})).unwrap();
    h.step();
    assert!(!retirement.retired(), "actual A producer is still paused after B is live");
    gate.0.lock().unwrap().final_released = true;
    gate.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !retirement.retired() && Instant::now() < deadline {
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(retirement.retired());
    assert_eq!(files.read_limited("companion/state.json", 4 * 1024 * 1024).unwrap(), Some(source));
    assert!(b_files.get("companion/state.json").is_none());
    assert!(b_files.get("companion/state.v1.backup.json").is_none());
    assert_eq!(h.app.session.develop_of(b_photo).unwrap().light.exposure, 0.6);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn paused_small_native_migration_also_yields_actual_frames() {
    let directory = std::env::temp_dir().join(format!("lc-ux10-small-live-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut original = lightcraft_engine::Session::new();
    original
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = original.active().unwrap();
    original.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"legacy large source ".repeat(100)}],"rounds":[],"checkpoint":null}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic("companion/state.json", &source).unwrap();
    let gate = Arc::new((Mutex::new(Pause::default()), Condvar::new()));
    let gate_worker = gate.clone();
    let worker_dir = directory.clone();
    let worker_catalog = catalog.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    let opener = std::thread::spawn(move || {
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(worker_catalog),
                    files: Box::new(PausedStore { native: FsStore::open(&worker_dir).unwrap(), gate: gate_worker }),
                    dir: worker_dir,
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        sender.send(session).unwrap();
    });
    let opened = receiver.recv_timeout(Duration::from_secs(1));
    if opened.is_err() {
        let (mutex, changed) = &*gate;
        mutex.lock().unwrap().released = true;
        changed.notify_all();
        opener.join().unwrap();
        assert!(opened.is_ok(), "native staging must yield actual frames even for a small source");
        return;
    }
    let session = opened.unwrap();
    opener.join().unwrap();
    let (mutex, changed) = &*gate;
    let current = mutex.lock().unwrap();
    let (current, _) = changed.wait_timeout_while(current, Duration::from_secs(5), |gate| !gate.entered).unwrap();
    assert!(current.entered);
    drop(current);
    let mut headless = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1000.0, 680.0], 1.0);
    let before = Instant::now();
    headless.app.run("develop.set", json!({"ids":[photo.0],"values":{"light.exposure":0.5}})).unwrap();
    headless.step();
    assert!(before.elapsed() < Duration::from_secs(1), "ordinary control/frame waited for migration filesystem");
    headless
        .app
        .session
        .set_companion_editor(photo, lightcraft_engine::companion::EditorState { draft: "latest live draft 草稿".into(), ..Default::default() })
        .unwrap();
    headless.step();
    assert_eq!(files.read_limited("companion/state.json", 4 * 1024 * 1024).unwrap(), Some(source.clone()));
    mutex.lock().unwrap().released = true;
    changed.notify_all();
    let deadline = Instant::now() + Duration::from_secs(20);
    while headless.app.session.companion_migration_pending() && Instant::now() < deadline {
        headless.step();
        let _ = headless.app.session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!headless.app.session.companion_migration_pending());
    assert_eq!(headless.app.session.companion_editor(photo).unwrap().draft, "latest live draft 草稿");
    assert_eq!(headless.app.session.develop_of(photo).unwrap().light.exposure, 0.5);
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 4 * 1024 * 1024).unwrap(), Some(source));
    headless.app.session.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cold_legacy_text_and_known_plans_are_readable_without_invented_cards() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let catalog = MemStore::new();
        let files = MemStore::new();
        let mut initial = lightcraft_engine::Session::new();
        initial
            .open_library_in(
                LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "legacy-readable".into(), on_disk: false },
                true,
            )
            .unwrap();
        let photo = initial.active().unwrap();
        initial.close_library().unwrap();
        files.set("companion/state.json",serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"Legacy retained question"},{"role":"assistant","text":"Original readable answer"}],"rounds":[{"plan_id":"original-plan-1","reason":"Original gentle exposure plan","operations":[{"type":"set_controls","values":{"light.exposure":0.4}}]},{"plan_id":"original-plan-2","reason":"Original contrast plan","operations":[{"type":"set_controls","values":{"light.contrast":0.2}}]}],"checkpoint":{"eligible":true}}}})).unwrap());
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "legacy-readable".into(), on_disk: false },
                true,
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while session.companion_migration_pending() && Instant::now() < deadline {
            let _ = session.persist();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!session.companion_migration_pending());
        let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1100., 750.], 1.);
        h.app.ui.language = language;
        h.app.ui.settings.gpu = false;
        h.app.run("ai.open", json!({})).unwrap();
        for _ in 0..12 {
            h.step();
        }
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert!(status["text"].as_str().unwrap().contains("Original readable answer"));
        assert_eq!(status["legacyPlans"][0]["plan_id"], "original-plan-1", "cold original plan must have a separate truthful legacy view");
        assert_eq!(status["legacyPlans"][0]["operations"][0]["values"]["light.exposure"], 0.4);
        assert_eq!(status["resultCards"], json!([]));
        assert!(h.app.run("ai.undo", json!({})).is_err());
        assert!(h.painted_text().iter().any(|text| text == language.tr("Legacy plan: execution details unavailable")));
        assert!(h.app.widgets.iter().any(|(id, _)| id == "ai:legacyPlan"));
        let result = h.request("ui.clickWidget", json!({"id":"button:aiLegacyNext"}), Duration::from_secs(10));
        assert_eq!(result["ok"], true);
        for _ in 0..12 {
            h.step();
        }
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(status["legacyPlans"][0]["plan_id"], "original-plan-2");
        assert_eq!(status["resultCards"], json!([]));
        assert!(
            h.painted_text()
                .iter()
                .any(|text| text == &format!("{}{}+0.2", language.tr("Contrast"), if language == crate::i18n::Locale::ZhHans { "：" } else { ": " })),
            "original fractional target must not be rounded to the current slider precision"
        );
        if let Ok(directory) = std::env::var("UX10_SCREENSHOT_DIR") {
            h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
            h.app.services.png = Some(Box::new(|image| {
                lightcraft_engine::export::encode_image(
                    image,
                    &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                )
                .unwrap()
            }));
            assert_eq!(
                h.request(
                    "ui.screenshot",
                    json!({"path":format!("{directory}/legacy-plan-{}.png",language.code()),"headless":true}),
                    Duration::from_secs(20)
                )["ok"],
                true
            );
        }
    }
}

#[test]
fn retired_migration_receipt_stays_retired_when_a_new_retry_is_live() {
    let directory = std::env::temp_dir().join(format!("lc-ux10-retirement-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = lightcraft_engine::Session::new();
    initial
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"same recoverable source ".repeat(30000)}],"rounds":[],"checkpoint":null}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic("companion/state.json", &source).unwrap();
    let gate = Arc::new((Mutex::new(Pause { released: true, pause_final: true, ..Default::default() }), Condvar::new()));
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog),
                files: Box::new(PausedStore { native: FsStore::open(&directory).unwrap(), gate: gate.clone() }),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !gate.0.lock().unwrap().final_entered && Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(gate.0.lock().unwrap().final_entered);
    let retired = session.cancel_companion_migration().unwrap();
    gate.0.lock().unwrap().final_released = true;
    gate.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !retired.retired() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(retired.retired());
    {
        let mut g = gate.0.lock().unwrap();
        g.final_entered = false;
        g.final_released = false;
    }
    let _ = session.retry_companion_migration();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.0.lock().unwrap().final_entered && Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(gate.0.lock().unwrap().final_entered);
    assert!(session.companion_migration_busy());
    let stayed_retired = retired.retired();
    gate.0.lock().unwrap().final_released = true;
    gate.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.companion_migration_pending() && Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!session.companion_migration_pending());
    assert!(stayed_retired, "a new retry must not resurrect the prior producer's retirement receipt");
    assert_eq!(files.read_limited("companion/state.v1.backup.json", 4 * 1024 * 1024).unwrap(), Some(source));
    session.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_large_native_backup_keeps_bounded_legacy_reading_and_visible_retry() {
    let directory = std::env::temp_dir().join(format!("lc-ux10-read-failure-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = lightcraft_engine::Session::new();
    initial
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic_with("companion/state.json",&mut |w|{
        write!(w,r#"{{"format":"lightcraft-companion","version":1,"photos":{{"{}":{{"schema_version":1,"messages":[{{"role":"user","text":"OLD RETAINED QUESTION "#,photo.0)?;
        let bytes=vec![b'x';256*1024];for _ in 0..260 {w.write_all(&bytes)?;}
        w.write_all(br#""},{"role":"assistant","text":"Original next readable answer"}],"rounds":[{"plan_id":"old-known-plan","reason":"Original gentle exposure","operations":[{"type":"set_controls","values":{"light.exposure":0.4}}]}],"checkpoint":{"eligible":true}}}}"#)?;Ok(())
    }).unwrap();
    let mut decoder =
        serde_json::Deserializer::from_reader(std::io::BufReader::new(std::fs::File::open(directory.join("companion/state.json")).unwrap()));
    serde::Deserialize::deserialize(&mut decoder).map(|_: serde::de::IgnoredAny| ()).unwrap();
    decoder.end().unwrap();
    let gate = Arc::new((Mutex::new(Pause { released: true, fail_backup: true, ..Default::default() }), Condvar::new()));
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog),
                files: Box::new(PausedStore { native: FsStore::open(&directory).unwrap(), gate }),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while session.companion_migration_error().is_none() && Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(session.companion_migration_error().is_some());
    assert!(session.companion_migration_pending());
    assert!(session.companion_photo_header(photo).unwrap().is_none());
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1100., 750.], 1.);
    h.app.ui.settings.gpu = false;
    h.app.run("ai.open", json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut status = json!({});
    while Instant::now() < deadline {
        h.step();
        status = h.app.run("ai.status", json!({})).unwrap();
        if status["text"].as_str().unwrap().contains("OLD RETAINED QUESTION") {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        status["text"].as_str().unwrap().contains("OLD RETAINED QUESTION"),
        "failed activation must retain the real bounded original reader: {status}"
    );
    assert_eq!(status["legacyPlans"][0]["plan_id"], "old-known-plan");
    assert_eq!(status["migrationPending"], true);
    assert!(h.app.widgets.iter().any(|(id, _)| id == "button:aiRetryMigration"));
    h.app.run("ai.history", json!({"record":1,"offset":0})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["text"].as_str().unwrap().contains("Original next readable answer") {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Ok(output) = std::env::var("UX10_SCREENSHOT_DIR") {
        h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        h.app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
            h.app.ui.language = language;
            h.step();
            h.step();
            assert_eq!(
                h.request(
                    "ui.screenshot",
                    json!({"path":format!("{output}/legacy-migration-failure-{}.png",language.code()),"headless":true}),
                    Duration::from_secs(20)
                )["ok"],
                true
            );
        }
    }
    let _ = h.app.run("develop.set", json!({"ids":[photo.0],"values":{"light.exposure":0.7}}));
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.7);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiRetryMigration"}), Duration::from_secs(10))["ok"], true);
    let deadline = Instant::now() + Duration::from_secs(30);
    while h.app.session.companion_migration_pending() && Instant::now() < deadline {
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!h.app.session.companion_migration_pending());
    assert_eq!(h.app.session.companion_messages_page(photo, 1, 1).unwrap().records[0]["text"], "Original next readable answer");
    assert!(directory.join("companion/state.v1.backup.json").metadata().unwrap().len() > 64 * 1024 * 1024);
    h.app.session.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn legacy_png_and_missing_reference_are_readable_with_local_degradation() {
    let catalog = MemStore::new();
    let files = MemStore::new();
    let mut initial = lightcraft_engine::Session::new();
    initial
        .open_library_in(
            LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(files.clone()), dir: "legacy-image-reading".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = initial.active().unwrap();
    let png = lightcraft_engine::export::encode_image(
        &lightcraft_raster::Rgba8::from_fn(32, 32, |_, _| [24, 196, 212, 255]),
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    let attachment = initial.library.as_mut().unwrap().companion_attachment(photo, &png).unwrap();
    initial.close_library().unwrap();
    let missing = format!("companion/preview-{}-00000000000000000000000000000000.bin", photo.0);
    let records = json!([{"role":"user","text":"Original valid PNG question","images":[{"mime_type":"image/png","attachment":attachment}]},{"role":"user","text":"Original missing preview question","images":[{"mime_type":"image/jpeg","attachment":missing}]}]);
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":records,"rounds":[],"checkpoint":null}}})).unwrap();
    files.set("companion/state.json", source.clone());
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(catalog), files: Box::new(files.clone()), dir: "legacy-image-reading".into(), on_disk: false },
            true,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.companion_migration_pending() && Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !session.companion_migration_pending(),
        "valid PNG and unavailable refs must migrate as retained legacy data: {:?}",
        session.companion_migration_error()
    );
    assert_eq!(session.companion_messages_page(photo, 0, 2).unwrap().records, records.as_array().unwrap().clone());
    assert_eq!(files.get("companion/state.v1.backup.json"), Some(source));
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1100., 750.], 1.);
    h.app.ui.settings.gpu = false;
    h.app.run("ai.open", json!({})).unwrap();
    for _ in 0..12 {
        h.step();
    }
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["historicalPreview"]["available"], true);
    assert_eq!(status["historicalPreview"]["record"], 0);
    assert_eq!(h.request("ui.hoverWidget", json!({"id":"ai:messages"}), Duration::from_secs(5))["ok"], true);
    assert_eq!(h.request("ui.scroll", json!({"dy":-300}), Duration::from_secs(5))["ok"], true);
    assert!(h.step_until(Duration::from_secs(5), |h| h.app.widgets.iter().any(|(id, _)| id == "image:aiHistoricalPreview")));
    let rect = h.app.widgets.iter().find(|(id, _)| id == "image:aiHistoricalPreview").unwrap().1;
    let image = h.paint();
    let center = rect.center();
    assert_eq!(
        image.pixels[center.y as usize * image.size[0] + center.x as usize],
        egui::Color32::from_rgb(24, 196, 212),
        "original historical PNG pixels must be rendered"
    );
    if let Ok(output) = std::env::var("UX10_SCREENSHOT_DIR") {
        h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        h.app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
            h.app.ui.language = language;
            h.step();
            h.step();
            assert_eq!(
                h.request(
                    "ui.screenshot",
                    json!({"path":format!("{output}/legacy-image-{}.png",language.code()),"headless":true}),
                    Duration::from_secs(20)
                )["ok"],
                true
            );
        }
        h.app.ui.language = crate::i18n::Locale::En;
        for _ in 0..20 {
            h.step();
        }
    }
    assert_eq!(h.request("ui.hoverWidget", json!({"id":"button:aiHistoricalNext"}), Duration::from_secs(10))["ok"], true);
    for _ in 0..10 {
        h.step();
    }
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoricalNext"}), Duration::from_secs(10))["ok"], true);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historicalPreview"]["position"] == 1 && status["historyLoading"] == false {
            break;
        }
        assert!(Instant::now() < deadline, "{status}");
        std::thread::yield_now();
    }
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["historicalPreview"]["available"], false);
    assert_eq!(status["historicalPreview"]["record"], 1);
    assert_eq!(h.request("ui.hoverWidget", json!({"id":"ai:messages"}), Duration::from_secs(5))["ok"], true);
    assert_eq!(h.request("ui.scroll", json!({"dy":-300}), Duration::from_secs(5))["ok"], true);
    assert!(
        h.step_until(Duration::from_secs(5), |h| h
            .painted_text()
            .iter()
            .any(|text| text == "Historical preview unavailable. Original reference is retained.")),
        "{:?}",
        h.painted_text()
    );
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "image:aiHistoricalPreview"));
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 2).unwrap().records, records.as_array().unwrap().clone());
    if let Ok(output) = std::env::var("UX10_SCREENSHOT_DIR") {
        for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
            h.app.ui.language = language;
            h.step();
            h.step();
            assert_eq!(
                h.request(
                    "ui.screenshot",
                    json!({"path":format!("{output}/legacy-missing-image-{}.png",language.code()),"headless":true}),
                    Duration::from_secs(20)
                )["ok"],
                true
            );
        }
    }
}

#[test]
fn original_legacy_plan_stays_readable_after_new_format_round_save() {
    for background in [false, true] {
        let catalog = MemStore::new();
        let mut files = MemStore::new();
        files.background = background;
        let mut initial = lightcraft_engine::Session::new();
        initial
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(catalog.clone()),
                    files: Box::new(files.clone()),
                    dir: "legacy-continuation".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let photo = initial.active().unwrap();
        initial.close_library().unwrap();
        files.set("companion/state.json",serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"Original request stays readable"}],"rounds":[{"plan_id":"old-original-plan","reason":"Original exposure reason","operations":[{"type":"set_controls","values":{"light.exposure":0.4}}]}]}}})).unwrap());
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(catalog.clone()),
                    files: Box::new(files.clone()),
                    dir: "legacy-continuation".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while session.companion_migration_pending() && Instant::now() < deadline {
            let _ = session.persist();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!session.companion_migration_pending());
        let before = (*session.develop_of(photo).unwrap()).clone();
        let mut after = before.clone();
        after.light.exposure = 0.5;
        session.set_develop(photo, after.clone(), "Later applied result").unwrap();
        let prefix = session.companion_photo_header(photo).unwrap().unwrap();
        session
        .save_companion_tail(
            photo,
            Some(&prefix),
            &[json!({"role":"user","text":"New format request","local_round_id":"new-format-round"})],
            &[],
            &[json!({"local_round_id":"new-format-round","photo_id":photo.0,"local_terminal":"completed","recent_complete_result":after,"stages":[{"stage":"initial","reason":"Later applied exposure","before_state":before,"proposed_state":after,"applied_state":after,"locally_published":true,"delivery_state":"confirmed"}]})],
            &json!({"schema_version":2,"checkpoint":null}),
        )
        .unwrap();
        session.close_library().unwrap();
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "legacy-continuation".into(), on_disk: false },
                true,
            )
            .unwrap();
        let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1100., 750.], 1.);
        h.app.ui.settings.gpu = false;
        h.app.run("ai.open", json!({})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            h.step();
            let status = h.app.run("ai.status", json!({})).unwrap();
            if status["historyLoading"] == false {
                break status;
            }
            assert!(Instant::now() < deadline, "cold history did not finish loading");
            std::thread::yield_now();
        };
        assert!(status["text"].as_str().unwrap().contains("Original request stays readable"));
        assert_eq!(status["legacyPlans"][0]["plan_id"], "old-original-plan", "new round saves must keep original legacy plan navigation");
        assert_eq!(status["legacyPlans"][0]["operations"][0]["values"]["light.exposure"], 0.4);
        assert!(status["resultCards"].as_array().unwrap().iter().all(|card| card["plan_id"] != "old-original-plan"));
        assert_eq!(status["resultCards"][0]["localRoundId"], "new-format-round", "actual later result must remain visible: {background}: {status}");
        assert_eq!(status["resultCards"][0]["stages"][0]["status"], "applied");
        assert_eq!(status["resultCards"][0]["canUndo"], false);
    }
}

fn legacy_outgoing(h: &mut Headless, requests: &mpsc::Receiver<network::Request>, suffix: &str) -> network::Request {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        h.step();
        if let Ok(request) = requests.try_recv() {
            assert!(request.path.ends_with(suffix), "unexpected {}", request.path);
            return request;
        }
        assert!(Instant::now() < deadline, "{suffix} was not emitted: {}", h.app.run("ai.status", json!({})).unwrap());
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn complete_legacy_tools_and_image_project_without_giant_additive_metadata() {
    let directory = std::env::temp_dir().join(format!("lc-ux10-projection-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = lightcraft_engine::Session::new();
    initial
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = initial.active().unwrap();
    let png = lightcraft_engine::export::encode_image(
        &lightcraft_raster::Rgba8::from_fn(32, 32, |_, _| [24, 196, 212, 255]),
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    let attachment = initial.library.as_mut().unwrap().companion_attachment(photo, &png).unwrap();
    initial.close_library().unwrap();
    let nested = format!("{}true{}", "[".repeat(300), "]".repeat(300));
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic_with("companion/state.json",&mut |w|{
        write!(w,r#"{{"format":"lightcraft-companion","version":1,"photos":{{"{}":{{"schema_version":1,"messages":[{{"role":"user","text":"","images":[{{"mime_type":"image/png","attachment":"{}","additive":{}}}],"additive":""#,photo.0,attachment,nested)?;let bytes=vec![b'x';256*1024];for _ in 0..260{w.write_all(&bytes)?;}
        w.write_all(br#""},{"role":"assistant","text":"Original tool proposal","tool_calls":[{"id":"old-tool-1","name":"get_photo_state","arguments":{}}]},{"role":"tool","tool_call_id":"old-tool-1","text":"Original actual state result"},{"role":"assistant","text":"Original complete answer"}],"rounds":[]}}}"#)?;Ok(())
    }).unwrap();
    let mut decoder =
        serde_json::Deserializer::from_reader(std::io::BufReader::new(std::fs::File::open(directory.join("companion/state.json")).unwrap()));
    serde::Deserialize::deserialize(&mut decoder).map(|_: serde::de::IgnoredAny| ()).unwrap();
    decoder.end().unwrap();
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(catalog), files: Box::new(FsStore::open(&directory).unwrap()), dir: directory.clone(), on_disk: false },
            true,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while session.companion_migration_pending() && Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!session.companion_migration_pending());
    let mut app = LightcraftApp::new(session, crate::Services::default());
    let (connection, requests, events) = network::test_connection();
    app.ai.generation = app.session.library_generation();
    app.ai.connection = Some(connection);
    let mut h = Headless::new(app, [1100., 750.], 1.);
    h.app.ui.settings.gpu = false;
    h.app.run("ai.open", json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historyLoading"] == false {
            assert_eq!(status["historicalPreview"]["available"], true, "valid original PNG must remain viewable in a fragmented message");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    h.app.run("ai.start", json!({"prompt":"New complete request"})).unwrap();
    let creation = legacy_outgoing(&mut h, &requests, "/v1/rounds");
    events.send(network::Event::Server {kind:"round.state".into(),data:json!({"round_id":"legacy-projection-new","payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"library_generation":creation.identity.scope.library_generation,"photo_id":creation.identity.scope.photo.0},"state":"awaiting_input"}})}).unwrap();
    let input = legacy_outgoing(&mut h, &requests, "/input");
    assert_eq!(
        input.body["history"],
        json!([{"role":"user","text":"","images":[{"mime_type":"image/png","data":Base64::encode_string(&png)}]},{"role":"assistant","text":"Original tool proposal","tool_calls":[{"id":"old-tool-1","name":"get_photo_state","arguments":{}}]},{"role":"tool","tool_call_id":"old-tool-1","text":"Original actual state result"},{"role":"assistant","text":"Original complete answer"}])
    );
    h.app.run("ai.stop", json!({})).unwrap();
    h.step();
    h.step();
    h.app.session.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn explicit_save_and_close_report_pending_native_migration_then_keep_latest_draft() {
    let directory = std::env::temp_dir().join(format!("lc-ux10-close-readiness-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = lightcraft_engine::Session::new();
    initial
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"Original close source"}],"rounds":[]}}})).unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    files.write_atomic("companion/state.json", &source).unwrap();
    let gate = Arc::new((Mutex::new(Pause { released: true, pause_final: true, ..Default::default() }), Condvar::new()));
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(PausedStore { native: FsStore::open(&directory).unwrap(), gate: gate.clone() }),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.0.lock().unwrap().final_entered && Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(gate.0.lock().unwrap().final_entered);
    let saved = session.persist();
    let closed = session.close_library();
    assert!(session.companion_migration_busy());
    session
        .set_companion_editor(
            photo,
            lightcraft_engine::companion::EditorState { draft: "Newest draft while close waits 草稿".into(), ..Default::default() },
        )
        .unwrap();
    assert_eq!(files.read_limited("companion/state.json", 32 * 1024).unwrap(), Some(source));
    gate.0.lock().unwrap().final_released = true;
    gate.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.companion_migration_pending() && Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!session.companion_migration_pending());
    session.close_library().unwrap();
    let mut reopened = lightcraft_engine::Session::new();
    reopened
        .open_library_in(
            LibraryStores { catalog: Box::new(catalog), files: Box::new(FsStore::open(&directory).unwrap()), dir: directory.clone(), on_disk: false },
            true,
        )
        .unwrap();
    assert_eq!(reopened.companion_editor(photo).unwrap().draft, "Newest draft while close waits 草稿");
    reopened.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(saved.is_err(), "explicit Save cannot claim a live migration producer is durably saved");
    assert!(closed.is_err(), "Close cannot claim readiness while publication authority is live");
}

#[test]
fn source_replacement_and_same_length_inplace_edit_never_activate_stale_graphs() {
    for inplace in [false, true] {
        let directory = std::env::temp_dir().join(format!("lc-ux10-source-identity-{}-{inplace}", std::process::id()));
        let catalog = MemStore::new();
        let mut initial = lightcraft_engine::Session::new();
        initial
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(catalog.clone()),
                    files: Box::new(FsStore::open(&directory).unwrap()),
                    dir: directory.clone(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let photo = initial.active().unwrap();
        initial.close_library().unwrap();
        let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"captured original content"}],"rounds":[]}}})).unwrap();
        let replacement = String::from_utf8(source.clone()).unwrap().replace("captured", "replaced").into_bytes();
        assert_eq!(source.len(), replacement.len());
        let mut files = FsStore::open(&directory).unwrap();
        files.write_atomic("companion/state.json", &source).unwrap();
        let gate = Arc::new((Mutex::new(Pause { released: true, pause_final: true, ..Default::default() }), Condvar::new()));
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(catalog),
                    files: Box::new(PausedStore { native: FsStore::open(&directory).unwrap(), gate: gate.clone() }),
                    dir: directory.clone(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !gate.0.lock().unwrap().final_entered && Instant::now() < deadline {
            let _ = session.persist();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(gate.0.lock().unwrap().final_entered);
        if inplace {
            let path = directory.join("companion/state.json");
            let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
            let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            file.write_all(&replacement).unwrap();
            file.sync_all().unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
            assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), modified);
        } else {
            files.write_atomic("companion/state.json", &replacement).unwrap();
        }
        gate.0.lock().unwrap().final_released = true;
        gate.1.notify_all();
        let deadline = Instant::now() + Duration::from_secs(10);
        while session.companion_migration_error().is_none() && Instant::now() < deadline {
            let _ = session.persist();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(session.companion_migration_error().is_some());
        assert!(session.companion_migration_pending());
        assert_eq!(files.read_limited("companion/state.json", 32 * 1024).unwrap(), Some(replacement));
        assert_eq!(files.read_limited("companion/state.v1.backup.json", 32 * 1024).unwrap(), Some(source));
        assert!(session.acquire_companion_lease(photo, LocalRoundId::new().unwrap()).is_err());
        assert!(session.close_library().is_err());
        drop(session);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn native_large_unconfirmed_activation_recovery_keeps_frames_and_latest_editor_live() {
    let directory = std::env::temp_dir().join(format!("lc-ux10-native-recovery-{}", std::process::id()));
    let catalog = MemStore::new();
    let mut initial = lightcraft_engine::Session::new();
    initial
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(FsStore::open(&directory).unwrap()),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = initial.active().unwrap();
    initial.close_library().unwrap();
    let mut files = FsStore::open(&directory).unwrap();
    let source_length=files.write_atomic_with("companion/state.json",&mut |writer|{write!(writer,r#"{{"format":"lightcraft-companion","version":1,"photos":{{"{}":{{"schema_version":1,"messages":[{{"role":"user","text":"Readable original recovery text","additive":""#,photo.0)?;let bytes=vec![b'x';256*1024];for _ in 0..260{writer.write_all(&bytes)?;}writer.write_all(br#""}],"rounds":[{"plan_id":"original-recovery-plan","operations":[]}]}}}"#)?;Ok(())}).unwrap();
    let mut decoder =
        serde_json::Deserializer::from_reader(std::io::BufReader::new(std::fs::File::open(directory.join("companion/state.json")).unwrap()));
    serde::Deserialize::deserialize(&mut decoder).map(|_: serde::de::IgnoredAny| ()).unwrap();
    decoder.end().unwrap();
    let gate = Arc::new((Mutex::new(Pause { released: true, fail_final_visible: true, ..Default::default() }), Condvar::new()));
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(PausedStore { native: FsStore::open(&directory).unwrap(), gate: gate.clone() }),
                dir: directory.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while session.companion_migration_error().is_none() && Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(session.companion_migration_error().is_some());
    assert_eq!(serde_json::from_slice::<Value>(&files.read_limited("companion/state.json", 32 * 1024).unwrap().unwrap()).unwrap()["version"], 2);
    drop(session);
    let gate = Arc::new((Mutex::new(Pause { pause_backup: true, ..Default::default() }), Condvar::new()));
    let opener_gate = gate.clone();
    let opener_dir = directory.clone();
    let opener_catalog = catalog.clone();
    let (sender, receiver) = mpsc::channel();
    let opener = std::thread::spawn(move || {
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(opener_catalog),
                    files: Box::new(PausedStore { native: FsStore::open(&opener_dir).unwrap(), gate: opener_gate }),
                    dir: opener_dir,
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        sender.send(session).unwrap();
    });
    let opened = receiver.recv_timeout(Duration::from_secs(1));
    if opened.is_err() {
        gate.0.lock().unwrap().released = true;
        gate.1.notify_all();
        opener.join().unwrap();
        assert!(opened.is_ok(), "large recovery backup capture must belong to an owned producer");
        return;
    }
    let session = opened.unwrap();
    opener.join().unwrap();
    let current = gate.0.lock().unwrap();
    let (current, _) = gate.1.wait_timeout_while(current, Duration::from_secs(5), |gate| !gate.entered).unwrap();
    assert!(current.entered);
    drop(current);
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1000., 680.], 1.);
    h.app.ui.settings.gpu = false;
    let before = Instant::now();
    h.app.run("develop.set", json!({"ids":[photo.0],"values":{"light.exposure":0.45}})).unwrap();
    h.step();
    assert!(before.elapsed() < Duration::from_secs(1));
    h.app
        .session
        .set_companion_editor(
            photo,
            lightcraft_engine::companion::EditorState { draft: "Latest draft during recovery 草稿".into(), ..Default::default() },
        )
        .unwrap();
    assert!(h.app.session.persist().is_err());
    assert!(h.app.session.close_library().is_err());
    gate.0.lock().unwrap().released = true;
    gate.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(30);
    while h.app.session.companion_migration_pending() && Instant::now() < deadline {
        h.step();
        let _ = h.app.session.persist();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!h.app.session.companion_migration_pending(), "{:?}", h.app.session.companion_migration_error());
    assert_eq!(h.app.session.companion_rounds_page(photo, 0, 1).unwrap().records[0]["plan_id"], "original-recovery-plan");
    assert_eq!(directory.join("companion/state.v1.backup.json").metadata().unwrap().len(), source_length);
    h.app.session.close_library().unwrap();
    drop(h);
    let mut reopened = lightcraft_engine::Session::new();
    reopened
        .open_library_in(
            LibraryStores { catalog: Box::new(catalog), files: Box::new(FsStore::open(&directory).unwrap()), dir: directory.clone(), on_disk: false },
            true,
        )
        .unwrap();
    assert_eq!(reopened.companion_editor(photo).unwrap().draft, "Latest draft during recovery 草稿");
    assert_eq!(reopened.develop_of(photo).unwrap().light.exposure, 0.45);
    reopened.close_library().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn corrupted_giant_legacy_text_is_not_exposed_as_original_visible_history() {
    for padding in [512 * 1024, 5 * 1024 * 1024] {
        let directory = std::env::temp_dir().join(format!("lc-ux10-legacy-fragment-integrity-{}-{padding}", std::process::id()));
        let catalog = MemStore::new();
        let mut initial = lightcraft_engine::Session::new();
        initial
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(catalog.clone()),
                    files: Box::new(FsStore::open(&directory).unwrap()),
                    dir: directory.clone(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let photo = initial.active().unwrap();
        initial.close_library().unwrap();
        let original = "ORIGINAL_LEGACY_QUESTION";
        let corrupted = "CORRUPT_LEGACY_QUESTION!";
        assert_eq!(original.len(), corrupted.len());
        let text = format!("{original}{}", "x".repeat(padding));
        let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":text}],"rounds":[]}}})).unwrap();
        let mut files = FsStore::open(&directory).unwrap();
        files.write_atomic("companion/state.json", &source).unwrap();
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(catalog.clone()),
                    files: Box::new(FsStore::open(&directory).unwrap()),
                    dir: directory.clone(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while session.companion_migration_pending() && Instant::now() < deadline {
            let _ = session.persist();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!session.companion_migration_pending());
        assert!(
            session.companion_history_slice(photo, lightcraft_engine::companion::storage::HistoryCursor::default(), 1024).unwrap().fragments[0]
                .starts_with(original)
        );
        session.close_library().unwrap();
        drop(session);
        assert_eq!(files.read_limited("companion/state.v1.backup.json", 8 * 1024 * 1024).unwrap(), Some(source));
        let mut damaged = false;
        for entry in std::fs::read_dir(directory.join(format!("companion/v2/photos/{}/blocks", photo.0))).unwrap() {
            let path = entry.unwrap().path();
            let mut bytes = std::fs::read(&path).unwrap();
            if let Some(offset) = bytes.windows(original.len()).position(|bytes| bytes == original.as_bytes()) {
                bytes[offset..offset + original.len()].copy_from_slice(corrupted.as_bytes());
                std::fs::write(path, bytes).unwrap();
                damaged = true;
                break;
            }
        }
        assert!(damaged, "fault must affect the actual original text data block");
        let mut session = lightcraft_engine::Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(catalog),
                    files: Box::new(FsStore::open(&directory).unwrap()),
                    dir: directory.clone(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1000., 680.], 1.);
        h.app.ui.settings.gpu = false;
        h.app.run("ai.open", json!({})).unwrap();
        h.app.run("ai.history", json!({"record":0,"offset":0})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut status = json!({});
        while Instant::now() < deadline {
            h.step();
            status = h.app.run("ai.status", json!({})).unwrap();
            if status["historyLoading"] == false {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(status["historyLoading"], false);
        let visible = status["text"].as_str().unwrap_or_default().to_owned();
        h.app.session.close_library().unwrap();
        drop(h);
        std::fs::remove_dir_all(directory).unwrap();
        assert!(!visible.contains(corrupted), "actual visible history must reject corrupted original bytes");
    }
}

#[test]
fn small_legacy_png_with_deep_additive_image_metadata_keeps_visible_text_and_image() {
    for fragmented in [false, true] {
        for native in [true, false] {
            let directory = std::env::temp_dir().join(format!("lc-ux10-small-image-{}", LocalRoundId::new().unwrap().as_str()));
            let catalog = MemStore::new();
            let memory = MemStore::new();
            let files = || -> Box<dyn Store> { if native { Box::new(FsStore::open(&directory).unwrap()) } else { Box::new(memory.clone()) } };
            let mut initial = lightcraft_engine::Session::new();
            initial
                .open_library_in(LibraryStores { catalog: Box::new(catalog.clone()), files: files(), dir: directory.clone(), on_disk: false }, true)
                .unwrap();
            let photo = initial.active().unwrap();
            let png = lightcraft_engine::export::encode_image(
                &lightcraft_raster::Rgba8::from_fn(32, 32, |_, _| [24, 196, 212, 255]),
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap();
            let attachment = initial.library.as_mut().unwrap().companion_attachment(photo, &png).unwrap();
            initial.close_library().unwrap();
            let deep = format!("{}true{}", "[".repeat(300), "]".repeat(300));
            let extra = if fragmented { format!(r#","opaque":"{}""#, "x".repeat(512 * 1024)) } else { String::new() };
            let source = format!(
                r#"{{"format":"lightcraft-companion","version":1,"photos":{{"{}":{{"schema_version":1,"messages":[{{"role":"user","text":"Original small valid history","images":[{{"mime_type":"image/png","attachment":"{}","additive":{}}}]{} }}],"rounds":[]}}}}}}"#,
                photo.0, attachment, deep, extra
            );
            files().write_atomic("companion/state.json", source.as_bytes()).unwrap();
            drop(initial);
            let mut session = lightcraft_engine::Session::new();
            session
                .open_library_in(LibraryStores { catalog: Box::new(catalog), files: files(), dir: directory.clone(), on_disk: false }, true)
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while session.companion_migration_pending() && Instant::now() < deadline {
                let _ = session.persist();
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(!session.companion_migration_pending(), "{:?}", session.companion_migration_error());
            assert_eq!(files().read("companion/state.v1.backup.json").unwrap().unwrap(), source.as_bytes());
            let header = session.companion_photo_header(photo).unwrap().unwrap();
            if let Some(mut reader) = session.companion_reader(&header).unwrap() {
                let protocol = reader.protocol_messages(0, 1, 32 * 1024 * 1024).unwrap();
                assert_eq!(protocol.records[0]["text"], "Original small valid history");
                assert_eq!(protocol.records[0]["images"][0]["attachment"], attachment);
                assert!(protocol.records[0]["images"][0].get("additive").is_none());
            }
            let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1100., 750.], 1.);
            h.app.ui.settings.gpu = false;
            h.app.run("ai.open", json!({})).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let status = loop {
                h.step();
                let status = h.app.run("ai.status", json!({})).unwrap();
                if status["historyLoading"] == false {
                    break status;
                }
                assert!(Instant::now() < deadline, "{status}");
                std::thread::yield_now();
            };
            assert!(status["text"].as_str().unwrap().contains("Original small valid history"), "{native}: {status}");
            assert_eq!(status["historicalPreview"]["available"], true, "{native}: {status}");
            h.step();
            h.step();
            assert_eq!(h.request("ui.hoverWidget", json!({"id":"ai:messages"}), Duration::from_secs(5))["ok"], true);
            assert_eq!(h.request("ui.scroll", json!({"dy":-300}), Duration::from_secs(5))["ok"], true);
            assert!(h.step_until(Duration::from_secs(5), |h| h.app.widgets.iter().any(|(id, _)| id == "image:aiHistoricalPreview")));
            let rect = h.app.widgets.iter().find(|(id, _)| id == "image:aiHistoricalPreview").unwrap().1;
            let image = h.paint();
            let center = rect.center();
            assert_eq!(image.pixels[center.y as usize * image.size[0] + center.x as usize], egui::Color32::from_rgb(24, 196, 212));
            if !native {
                let (connection, requests, _) = network::test_connection();
                h.app.ai.connection = Some(connection);
                let submitted = h.app.run("ai.start", json!({"prompt":"Continue from the complete original history"}));
                assert!(submitted.is_ok(), "complete retained history must remain submit-capable: {submitted:?}");
                native_history_tests::outgoing(&mut h, &requests, "/v1/rounds");
                h.app.run("ai.stop", json!({})).unwrap();
            }
            assert!(h.settle(Duration::from_secs(10)));
            h.app.session.close_library().unwrap();
            drop(h);
            if native {
                std::fs::remove_dir_all(directory).unwrap();
            }
        }
    }
}

#[test]
fn legacy_image_navigation_resets_for_different_page_counts_and_keeps_missing_refs_recoverable() {
    for native in [true, false] {
        let directory = std::env::temp_dir().join(format!("lc-ux10-page-images-{}", LocalRoundId::new().unwrap().as_str()));
        let catalog = MemStore::new();
        let memory = MemStore::new();
        let files = || -> Box<dyn Store> { if native { Box::new(FsStore::open(&directory).unwrap()) } else { Box::new(memory.clone()) } };
        let mut initial = lightcraft_engine::Session::new();
        initial
            .open_library_in(LibraryStores { catalog: Box::new(catalog.clone()), files: files(), dir: directory.clone(), on_disk: false }, true)
            .unwrap();
        let photo = initial.active().unwrap();
        let png = lightcraft_engine::export::encode_image(
            &lightcraft_raster::Rgba8::from_fn(32, 32, |_, _| [24, 196, 212, 255]),
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
        )
        .unwrap();
        let name = initial.library.as_mut().unwrap().companion_attachment(photo, &png).unwrap();
        initial.close_library().unwrap();
        drop(initial);
        let missing = format!("companion/preview-{}-00000000000000000000000000000000.bin", photo.0);
        let messages: Vec<_> = (0..33)
            .map(|record| {
                let mut images = vec![json!({"mime_type":"image/png","attachment":name})];
                if record < 32 {
                    images.push(json!({"mime_type":"image/png","attachment":if record==0 {&missing}else{&name}}));
                }
                json!({"role":"user","text":format!("Original page message {record}"),"images":images})
            })
            .collect();
        let source = serde_json::to_vec(
            &json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":messages,"rounds":[]}}}),
        )
        .unwrap();
        files().write_atomic("companion/state.json", &source).unwrap();
        let mut session = lightcraft_engine::Session::new();
        session.open_library_in(LibraryStores { catalog: Box::new(catalog), files: files(), dir: directory.clone(), on_disk: false }, true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while session.companion_migration_pending() && Instant::now() < deadline {
            let _ = session.persist();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!session.companion_migration_pending(), "{:?}", session.companion_migration_error());
        let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1100., 750.], 1.);
        h.app.ui.settings.gpu = false;
        h.app.run("ai.open", json!({})).unwrap();
        let ready = |h: &mut Headless| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                h.step();
                let status = h.app.run("ai.status", json!({})).unwrap();
                if status["historyLoading"] == false {
                    return status;
                }
                assert!(Instant::now() < deadline, "{status}");
                std::thread::yield_now();
            }
        };
        h.app.run("ai.history", json!({"record":0,"offset":0})).unwrap();
        assert_eq!(ready(&mut h)["historicalPreview"]["total"], 64);
        h.app.run("ai.historicalImage", json!({"index":5})).unwrap();
        assert_eq!(ready(&mut h)["historicalPreview"]["position"], 5);
        h.app.run("ai.history", json!({"record":32,"offset":0})).unwrap();
        let status = ready(&mut h);
        assert_eq!(status["historicalPreview"]["position"], 0, "page-local selection must reset: {native}: {status}");
        assert_eq!(status["historicalPreview"]["total"], 1);
        assert_eq!(status["historicalPreview"]["available"], true);
        h.app.run("ai.historicalImage", json!({"index":0})).unwrap();
        assert_eq!(ready(&mut h)["historicalPreview"]["record"], 32);
        h.app.run("ai.history", json!({"record":0,"offset":0})).unwrap();
        let status = ready(&mut h);
        assert_eq!(status["historicalPreview"]["position"], 0);
        assert_eq!(status["historicalPreview"]["total"], 64);
        h.app.run("ai.historicalImage", json!({"index":1})).unwrap();
        let status = ready(&mut h);
        assert_eq!(status["historicalPreview"]["available"], false);
        assert_eq!(status["historicalPreview"]["total"], 64);
        h.app.run("ai.historicalImage", json!({"index":0})).unwrap();
        assert_eq!(ready(&mut h)["historicalPreview"]["available"], true);
        assert_eq!(files().read("companion/state.v1.backup.json").unwrap().unwrap(), source);
        assert!(h.settle(Duration::from_secs(10)));
        h.app.session.close_library().unwrap();
        drop(h);
        if native {
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
}

#[test]
fn fragmented_legacy_png_remains_viewable_on_the_real_memory_fallback() {
    let catalog = MemStore::new();
    let files = MemStore::new();
    let open = |session: &mut lightcraft_engine::Session| {
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(catalog.clone()),
                    files: Box::new(files.clone()),
                    dir: "ux10-fragment-image".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
    };
    let mut initial = lightcraft_engine::Session::new();
    open(&mut initial);
    let photo = initial.active().unwrap();
    let png = lightcraft_engine::export::encode_image(
        &lightcraft_raster::Rgba8::from_fn(32, 32, |_, _| [24, 196, 212, 255]),
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    let attachment = initial.library.as_mut().unwrap().companion_attachment(photo, &png).unwrap();
    initial.close_library().unwrap();
    drop(initial);
    let message = json!({"role":"user","text":"Original fragmented PNG question","images":[{"mime_type":"image/png","attachment":attachment}],"additive":"x".repeat(512*1024)});
    let source = serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[message.clone()],"rounds":[]}}})).unwrap();
    files.clone().write_atomic("companion/state.json", &source).unwrap();
    let mut session = lightcraft_engine::Session::new();
    open(&mut session);
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.companion_migration_pending() && Instant::now() < deadline {
        let _ = session.persist();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!session.companion_migration_pending(), "{:?}", session.companion_migration_error());
    let header = session.companion_photo_header(photo).unwrap().unwrap();
    assert!(session.companion_reader(&header).unwrap().is_none(), "the main history must really use fallback");
    let page = session.companion_history_slice_at(&header, Default::default(), 256 * 1024).unwrap();
    assert!(page.items.iter().any(|item| matches!(item, lightcraft_engine::companion::storage::HistoryItem::Fragment { .. })));
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1100., 750.], 1.);
    h.app.ui.settings.gpu = false;
    h.app.run("ai.open", json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historyLoading"] == false {
            break status;
        }
        assert!(Instant::now() < deadline, "{status}");
    };
    assert!(status["text"].as_str().unwrap().contains("Original fragmented PNG question"), "{status}");
    assert_eq!(status["historicalPreview"]["available"], true, "a valid original image must be reachable: {status}");
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records, vec![message]);
    assert_eq!(files.get("companion/state.v1.backup.json"), Some(source));
    assert!(h.settle(Duration::from_secs(10)));
    h.app.session.close_library().unwrap();
}

#[derive(Default)]
struct ContextGate {
    transaction: String,
    owning_thread: Option<std::thread::ThreadId>,
    main_reads: usize,
    entered: bool,
    released: bool,
}
#[derive(Clone)]
struct GatedMemoryContext {
    memory: MemStore,
    gate: Arc<(Mutex<ContextGate>, Condvar)>,
}
impl GatedMemoryContext {
    fn observe(&self, name: &str) {
        let mut gate = self.gate.0.lock().unwrap();
        if gate.transaction.is_empty() || !name.contains("/blocks/") || !name.contains(&gate.transaction) {
            return;
        }
        if gate.owning_thread == Some(std::thread::current().id()) {
            gate.main_reads += 1;
        } else {
            gate.entered = true;
            self.gate.1.notify_all();
            while !gate.released {
                gate = self.gate.1.wait(gate).unwrap();
            }
        }
    }
}
impl Store for GatedMemoryContext {
    fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.observe(name);
        self.memory.read(name)
    }
    fn read_limited(&mut self, name: &str, limit: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.observe(name);
        self.memory.read_limited(name, limit)
    }
    fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.observe(name);
        self.memory.read_range(name, offset, length)
    }
    fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.memory.write_atomic(name, bytes)
    }
    fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.memory.append(name, bytes)
    }
    fn remove(&mut self, name: &str) -> std::io::Result<()> {
        self.memory.remove(name)
    }
    fn truncate(&mut self, name: &str, length: u64) -> std::io::Result<()> {
        self.memory.truncate(name, length)
    }
    fn describe(&self) -> String {
        self.memory.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.memory.retention_namespace()
    }
    fn source_snapshot(&mut self, name: &str) -> std::io::Result<Option<Box<dyn SourceSnapshot>>> {
        self.memory.source_snapshot(name)
    }
    fn copy_source_atomic(&mut self, name: &str, source: &mut dyn SourceSnapshot) -> std::io::Result<()> {
        self.memory.copy_source_atomic(name, source)
    }
    fn replace_source_atomic(&mut self, name: &str, expected: &SourceIdentity, bytes: &[u8]) -> std::io::Result<()> {
        self.memory.replace_source_atomic(name, expected, bytes)
    }
    fn replace_source_atomic_authorized(
        &mut self,
        name: &str,
        expected: &SourceIdentity,
        bytes: &[u8],
        authorized: &dyn Fn() -> bool,
    ) -> std::io::Result<()> {
        self.memory.replace_source_atomic_authorized(name, expected, bytes, authorized)
    }
    fn migration_writer(&self) -> Option<Box<dyn Store>> {
        Some(Box::new(self.clone()))
    }
}

#[test]
fn memory_history_submission_paints_and_stops_before_captured_record_reader_retires() {
    let catalog = MemStore::new();
    let memory = MemStore::new();
    let mut initial = lightcraft_engine::Session::new();
    initial
        .open_library_in(
            LibraryStores { catalog: Box::new(catalog.clone()), files: Box::new(memory.clone()), dir: "memory-preflight".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = initial.active().unwrap();
    let png = lightcraft_engine::export::encode_image(
        &lightcraft_raster::Rgba8::from_fn(32, 32, |_, _| [24, 196, 212, 255]),
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
    )
    .unwrap();
    let attachment = initial.library.as_mut().unwrap().companion_attachment(photo, &png).unwrap();
    initial.close_library().unwrap();
    let source=serde_json::to_vec(&json!({"format":"lightcraft-companion","version":1,"photos":{photo.0.to_string():{"schema_version":1,"messages":[{"role":"user","text":"Complete original context", "images":[{"mime_type":"image/png","attachment":attachment}],"opaque":"x".repeat(1024*1024)}],"rounds":[]}}})).unwrap();
    memory.set("companion/state.json", source.clone());
    let gate = Arc::new((Mutex::new(ContextGate::default()), Condvar::new()));
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog),
                files: Box::new(GatedMemoryContext { memory: memory.clone(), gate: gate.clone() }),
                dir: "memory-preflight".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while session.companion_migration_pending() {
        assert!(Instant::now() < deadline);
        let _ = session.persist();
        std::thread::yield_now();
    }
    let header = session.companion_photo_header(photo).unwrap().unwrap();
    assert!(header.history_complete());
    assert!(session.companion_reader(&header).unwrap().is_none(), "this must exercise the actual no-background-page Mem adapter");
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1100., 750.], 1.);
    h.app.ui.settings.gpu = false;
    h.app.run("ai.open", json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["historyLoading"] == false {
            break;
        }
        assert!(Instant::now() < deadline, "{status}");
        std::thread::yield_now();
    }
    assert!(h.settle(Duration::from_secs(15)));
    let (connection, requests, _) = network::test_connection();
    h.app.ai.next_connection = Some(connection);
    {
        let mut state = gate.0.lock().unwrap();
        state.transaction = header.transaction_id.clone();
        state.owning_thread = Some(std::thread::current().id());
    }
    let submitted = h.app.run("ai.start", json!({"prompt":"Keep the complete original context"}));
    let owning_reads = gate.0.lock().unwrap().main_reads;
    // Always release a genuinely paused filesystem adapter before reporting a
    // failure, so the test cannot strand an owned producer.
    if owning_reads != 0 || submitted.is_err() {
        gate.0.lock().unwrap().released = true;
        gate.1.notify_all();
        assert!(submitted.is_ok(), "{submitted:?}");
        assert_eq!(owning_reads, 0, "submission must not read captured history blocks on the owning thread");
    }
    let state = gate.0.lock().unwrap();
    let (state, _) = gate.1.wait_timeout_while(state, Duration::from_secs(5), |state| !state.entered).unwrap();
    let entered = state.entered;
    drop(state);
    h.step();
    let painted = h.painted_text().join("\n");
    let create_before_ready = requests.try_recv().ok();
    let stopped = h.app.run("ai.stop", json!({}));
    gate.0.lock().unwrap().released = true;
    gate.1.notify_all();
    assert!(entered, "real captured context worker must be paused at its record read");
    assert!(painted.contains("Submitting") || painted.contains("Preparing"), "{painted}");
    assert!(create_before_ready.is_none(), "complete context must verify before backend creation");
    assert!(stopped.is_ok(), "Stop must respond while context I/O is paused: {stopped:?}");
    assert!(h.settle(Duration::from_secs(15)));
    assert!(requests.try_recv().is_err(), "cancelled history cannot create a late request");
    assert!(h.app.session.companion_task_photo().is_none());
    assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.);
    assert_eq!(memory.get("companion/state.v1.backup.json"), Some(source));
    h.app.session.close_library().unwrap();
}
