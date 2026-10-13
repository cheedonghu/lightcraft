//! Exercise library/exit through caller commands, widgets and varying filesystem/transport adapters.
use super::*;
use crate::headless::Headless;
use lightcraft_catalog::{MemStore, Store};
use lightcraft_engine::library::LibraryStores;

#[derive(Clone, Default)]
struct Files {
    inner: MemStore,
    fail: Arc<AtomicBool>,
}
impl Store for Files {
    fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.inner.read(name)
    }
    fn read_limited(&mut self, name: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.inner.read_limited(name, max)
    }
    fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.inner.read_range(name, offset, length)
    }
    fn remove(&mut self, name: &str) -> std::io::Result<()> {
        self.inner.remove(name)
    }
    fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        if self.fail.load(Ordering::Relaxed) && name.starts_with("companion/") {
            return Err(std::io::Error::other("fixture drive unavailable"));
        }
        self.inner.write_atomic(name, bytes)
    }
    fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.append(name, bytes)
    }
    fn truncate(&mut self, name: &str, length: u64) -> std::io::Result<()> {
        self.inner.truncate(name, length)
    }
    fn describe(&self) -> String {
        "lifecycle fixture".into()
    }
}
fn client(files: Files) -> (Headless, mpsc::Receiver<network::Request>, mpsc::SyncSender<network::Event>, Arc<AtomicBool>) {
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(files), dir: "lifecycle".into(), on_disk: false }, true)
        .unwrap();
    let mut app = LightcraftApp::new(session, crate::Services::default());
    let (connection, requests, events) = network::test_connection();
    let retired = connection.alive.clone();
    app.ai.next_connection = Some(connection);
    let mut h = Headless::new(app, [1200.0, 800.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    h.step();
    h.step();
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), Duration::from_secs(10))["ok"], true);
    (h, requests, events, retired)
}
fn queued(h: &mut Headless, requests: &mpsc::Receiver<network::Request>, events: &mpsc::SyncSender<network::Event>) -> network::Request {
    assert_eq!(h.app.run("ai.start", json!({"prompt":"keep this request"})).unwrap()["started"], true);
    let request = requests.recv_timeout(Duration::from_secs(10)).unwrap();
    events.send(network::Event::Server{kind:"round.state".into(),data:json!({"round_id":format!("lifecycle-{}",request.submission),"payload":{"submission_seq":request.submission,"photo_ref":{"photo_id":request.identity.scope.photo.0,"library_generation":request.identity.scope.library_generation},"state":"queued"}})}).unwrap();
    h.step();
    h.step();
    request
}
#[test]
fn leaving_library_cancels_new_creation_before_a_previous_photo_save_can_fail() {
    let files = Files::default();
    let (mut h, requests, events, _retired) = client(files.clone());
    let a = h.app.session.active().unwrap();
    let b = h.app.session.catalog.photos().find(|p| p.id != a).unwrap().id;
    queued(&mut h, &requests, &events);
    h.app.run("ai.stop", json!({})).unwrap();
    assert!(requests.try_recv().unwrap().path.ends_with("/stop"));
    h.app.run("library.select", json!({"ids":[b.0],"active":b.0})).unwrap();
    h.app.run("ai.start", json!({"prompt":"new creation must stop"})).unwrap();
    let creation = requests.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(creation.path, "/v1/rounds");
    let settings = h.app.session.develop_of(b).unwrap();
    files.fail.store(true, Ordering::Relaxed);
    assert!(before_library_change(&mut h.app).is_err());
    assert!(creation.cancel.load(Ordering::Relaxed), "save error must never leave the newer request permitted to upload");
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["busy"], false);
    assert_eq!(h.app.session.develop_of(b).unwrap(), settings);
}

#[test]
fn menu_quit_with_hidden_ai_cancels_and_offers_unsaved_retry() {
    let files = Files::default();
    let (mut h, requests, events, _retired) = client(files.clone());
    let creation = queued(&mut h, &requests, &events);
    h.app.run("panel.edit", json!({})).unwrap();
    h.step();
    files.fail.store(true, Ordering::Relaxed);
    let response = h.request("ui.menu.invoke", json!({"command":"app.quit"}), Duration::from_secs(10));
    assert_eq!(response["ok"], true, "{response}");
    assert!(creation.cancel.load(Ordering::Relaxed), "menu quit must cancel even when the AI panel is hidden");
    assert!(!h.quit_requested(), "unsaved records require the existing exit choice");
    h.step();
    h.step();
    let prompt = h.request("ui.inspect", json!({}), Duration::from_secs(10))["result"]["quitPrompt"].clone();
    assert!(prompt.as_str().unwrap().contains("fixture drive unavailable"), "{prompt}");
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:quitCancel"}), Duration::from_secs(10))["ok"], true);
    assert!(!h.quit_requested());
    assert!(creation.cancel.load(Ordering::Relaxed));
    h.request("ui.menu.invoke", json!({"command":"app.quit"}), Duration::from_secs(10));
    h.step();
    h.step();
    files.fail.store(false, Ordering::Relaxed);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:quitRetry"}), Duration::from_secs(10))["ok"], true);
    h.step();
    h.step();
    assert!(h.quit_requested());
    assert!(h.app.session.unsaved().is_none());
}

#[test]
fn confirmed_unsaved_exit_never_authorizes_another_submission_after_transport_retirement() {
    let files = Files::default();
    let (mut h, requests, events, retired) = client(files.clone());
    let creation = queued(&mut h, &requests, &events);
    files.fail.store(true, Ordering::Relaxed);
    h.request("ui.menu.invoke", json!({"command":"app.quit"}), Duration::from_secs(10));
    h.step();
    h.step();
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:quitAnyway"}), Duration::from_secs(10))["ok"], true);
    assert!(h.quit_requested());
    assert!(creation.cancel.load(Ordering::Relaxed));
    // Vary the external transport: its workers have now actually retired.
    retired.store(false, Ordering::Relaxed);
    files.fail.store(false, Ordering::Relaxed);
    let (next, outgoing, _events) = network::test_connection();
    h.app.ai.next_connection = Some(next);
    assert!(h.app.run("ai.start", json!({"prompt":"must not leave this app"})).is_err(), "a confirmed exit is irreversible upload cancellation");
    assert!(outgoing.try_recv().is_err());
    assert!(crate::panels::notices::may_close(&mut h.app));
}

#[test]
fn host_shutdown_fallback_cancels_ai_even_without_normal_close_preparation() {
    const CHILD: &str = "UX15_SHUTDOWN_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ai_companion::lifecycle_tests::host_shutdown_fallback_cancels_ai_even_without_normal_close_preparation",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "shutdown caller failed: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let (mut h, requests, events, _retired) = client(Files::default());
    let creation = queued(&mut h, &requests, &events);
    let before = h.app.session.develop_of(creation.identity.scope.photo).unwrap();
    h.app.shutdown(Duration::ZERO);
    assert!(creation.cancel.load(Ordering::Relaxed), "host fallback must cancel content before its render wait");
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["busy"], false);
    assert_eq!(h.app.session.develop_of(creation.identity.scope.photo).unwrap(), before);
    assert!(h.app.run("ai.start", json!({})).is_err());
}

struct NativeLibraries(std::path::PathBuf);
impl NativeLibraries {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("lightcraft-ux15-{}", LocalRoundId::new().unwrap().as_str())))
    }
    fn path(&self, name: &str) -> std::path::PathBuf {
        self.0.join(name)
    }
    fn open(&self, name: &str) -> lightcraft_engine::Session {
        let mut session = lightcraft_engine::Session::new().with_fs();
        session.open_library(self.path(name), true).unwrap();
        session
    }
}
impl Drop for NativeLibraries {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn successful_library_switch_saves_jobless_drafts_and_waits_for_old_transport_retirement() {
    use lightcraft_engine::companion::EditorState;
    let libraries = NativeLibraries::new();
    let mut second = libraries.open("B");
    let shared = second.active().unwrap();
    second.close_library().unwrap();
    drop(second);
    let session = libraries.open("A");
    assert_eq!(session.active().unwrap(), shared);
    let mut app = LightcraftApp::new(session, crate::Services::default());
    let (old, requests, events) = network::test_connection();
    let retired = old.alive.clone();
    app.ai.next_connection = Some(old);
    let mut h = Headless::new(app, [1200.0, 800.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    h.step();
    h.step();
    h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), Duration::from_secs(10));
    let creation = queued(&mut h, &requests, &events);
    let jobless = h.app.session.catalog.photos().find(|p| p.id != shared).unwrap().id;
    h.app
        .session
        .set_companion_editor(
            jobless,
            EditorState {
                draft: "A unsubmitted draft without any round 草稿".into(), scroll_offset: 17.0, follow_tail: false, ..Default::default()
            },
        )
        .unwrap();
    h.app.run("app.openLibrary", json!({"path":libraries.path("B")})).unwrap();
    assert!(creation.cancel.load(Ordering::Relaxed));
    assert_eq!(h.app.session.active().unwrap(), shared);
    let (next, outgoing, _next_events) = network::test_connection();
    h.app.ai.next_connection = Some(next);
    assert!(
        h.app.run("ai.start", json!({"prompt":"B fresh request"})).is_err(),
        "a live old transport cannot be overtaken after a library generation changes"
    );
    assert!(outgoing.try_recv().is_err());
    retired.store(false, Ordering::Relaxed);
    h.app.run("ai.start", json!({"prompt":"B fresh request"})).unwrap();
    let fresh = outgoing.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_ne!(fresh.identity.local_round_id, creation.identity.local_round_id);
    assert_ne!(fresh.identity.scope.library_generation, creation.identity.scope.library_generation);
    h.app.run("app.openLibrary", json!({"path":libraries.path("A")})).unwrap();
    let editor = h.app.session.companion_editor(jobless).unwrap();
    assert_eq!(editor.draft, "A unsubmitted draft without any round 草稿");
    assert_eq!(editor.scroll_offset, 17.0);
    assert!(!editor.follow_tail);
    let text = serde_json::to_string(&h.app.session.companion_messages_page(shared, 0, 32).unwrap().records).unwrap();
    assert!(text.contains("keep this request") && !text.contains("B fresh request"), "{text}");
}
