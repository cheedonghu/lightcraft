//! Exercise library/exit through caller commands, widgets and varying filesystem/transport adapters.
use super::*;
#[test]
fn cold_saved_unfinished_round_is_reported_without_resuming_or_granting_old_permission() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        for (background, padding, native) in
            [(false, 0, false), (false, 2 * 1024 * 1024, false), (true, 2 * 1024 * 1024, false), (true, 2 * 1024 * 1024, true)]
        {
            let libraries = NativeLibraries::new();
            let directory = libraries.path("saved-interruption");
            let (catalog, mut files) = (MemStore::new(), MemStore::new());
            files.background = background;
            let stores = || LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: if native { Box::new(lightcraft_catalog::FsStore::open(&directory).unwrap()) } else { Box::new(files.clone()) },
                dir: directory.clone(),
                on_disk: false,
            };
            let mut session = lightcraft_engine::Session::new();
            session.open_library_in(stores(), true).unwrap();
            let photo = session.active().unwrap();
            let round = LocalRoundId::new().unwrap();
            let settings = session.develop_of(photo).unwrap();
            session.save_companion_conversation(photo, json!({"schema_version":2,"messages":[{"role":"user","text":"Reliable saved request"}],"rounds":[],"round_records":[{"local_round_id":round,"photo_id":photo.0,"request_text":"Reliable saved request","submission_seq":1,"connection_generation":1,"runtime_library_generation":session.library_generation(),"phase":"queued","original":settings,"stages":[],"opaque_extension":"x".repeat(padding)}]})).unwrap();
            session
                .set_companion_editor(
                    photo,
                    lightcraft_engine::companion::EditorState { draft: "Leave this draft unsent".into(), ..Default::default() },
                )
                .unwrap();
            session.close_library().unwrap();
            drop(session);
            let mut cold = lightcraft_engine::Session::new();
            cold.open_library_in(stores(), true).unwrap();
            let generation = cold.library_generation();
            let mut app = LightcraftApp::new(cold, crate::Services::default());
            app.ui.language = language;
            app.ui.settings.gpu = false;
            app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
            app.services.png = Some(Box::new(|image| {
                lightcraft_engine::export::encode_image(
                    image,
                    &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                )
                .unwrap()
            }));
            let (connection, outgoing, _events) = network::test_connection();
            app.ai.next_connection = Some(connection);
            let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
            h.app.run("ai.open", json!({})).unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                h.step();
                let status = h.app.run("ai.status", json!({})).unwrap();
                if status["task"]["terminal"] == "interrupted" {
                    break;
                }
                assert!(Instant::now() < deadline, "verified saved activity was not displayed: {status}");
                std::thread::yield_now();
            }
            for _ in 0..4 {
                h.step();
            }
            let status = h.app.run("ai.status", json!({})).unwrap();
            assert_eq!(status["task"]["terminal"], "interrupted", "only verified saved activity can produce this fact: {status}");
            assert_eq!(status["task"]["localRoundId"], round.as_str());
            assert_eq!(status["task"]["photo"], photo.0);
            assert_eq!(status["task"]["libraryGeneration"], generation);
            assert_eq!(status["busy"], false);
            assert_eq!(status["task"]["unread"], false, "saved interrupted activity is not a newly produced result");
            let notice = match language {
                crate::i18n::Locale::En => "The previous task was interrupted. It was not resumed.",
                _ => "上次任务已中断，未自动继续。",
            };
            assert!(h.painted_text().iter().any(|text| text == notice));
            assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiTaskEntry"), "open Tools header owns the task strip");
            let title = if language == crate::i18n::Locale::ZhHans { "AI 助手" } else { "AI Assistant" };
            assert!(h.painted_text().iter().any(|text| text == title));
            assert!(outgoing.try_recv().is_err(), "loading a saved task never creates a backend request");
            assert!(h.app.session.verify_round_revert(photo, &round).is_err());
            let lease = h
                .app
                .session
                .acquire_companion_lease(photo, LocalRoundId::new().unwrap())
                .expect("display-only recovery does not hold an editing lease");
            drop(lease);
            assert_eq!(h.app.session.develop_of(photo).unwrap(), settings);
            assert_eq!(h.app.session.companion_editor(photo).unwrap().draft, "Leave this draft unsent");
            if native && let Ok(output) = std::env::var("LIGHTCRAFT_UX15_SCREENSHOTS") {
                let path = std::path::Path::new(&output).join(format!("interrupted-{}.png", language.code()));
                let response = h.request("ui.screenshot", json!({"path":path,"headless":true}), Duration::from_secs(10));
                assert_eq!(response["ok"], true, "{response}");
            }
        }
    }
}

use crate::headless::Headless;
use lightcraft_catalog::{MemStore, Store};
use lightcraft_engine::library::LibraryStores;

#[test]
fn malformed_saved_round_identity_never_invents_an_interrupted_task() {
    for background in [false, true] {
        for variant in ["empty", "foreign", "terminal", "unknown-phase", "missing-proof", "superseded"] {
            let (catalog, mut files) = (MemStore::new(), MemStore::new());
            files.background = background;
            let stores = || LibraryStores {
                catalog: Box::new(catalog.clone()),
                files: Box::new(files.clone()),
                dir: "unknown-activity".into(),
                on_disk: false,
            };
            let mut session = lightcraft_engine::Session::new();
            session.open_library_in(stores(), true).unwrap();
            let photo = session.active().unwrap();
            let mut record = json!({"local_round_id":LocalRoundId::new().unwrap(),"photo_id":photo.0,"request_text":"Readable unknown activity","submission_seq":1,"connection_generation":1,"runtime_library_generation":session.library_generation(),"phase":"queued","stages":[]});
            let mut records = Vec::new();
            match variant {
                "empty" => record["local_round_id"] = json!(""),
                "foreign" => record["photo_id"] = json!(session.catalog.photos().find(|candidate| candidate.id != photo).unwrap().id.0),
                "terminal" => record["local_terminal"] = json!("completed"),
                "unknown-phase" => record["phase"] = json!("not-a-known-activity"),
                "missing-proof" => {
                    record.as_object_mut().unwrap().remove("submission_seq");
                }
                "superseded" => {
                    records.extend((0..129).map(|_| record.clone()));
                    record["local_terminal"] = json!("completed");
                }
                _ => {}
            }
            records.push(record);
            session
                .save_companion_conversation(
                    photo,
                    json!({"schema_version":2,"messages":[{"role":"user","text":"Readable unknown activity"}],"rounds":[],"round_records":records}),
                )
                .unwrap();
            session.close_library().unwrap();
            drop(session);
            let mut cold = lightcraft_engine::Session::new();
            cold.open_library_in(stores(), true).unwrap();
            let mut app = LightcraftApp::new(cold, crate::Services::default());
            app.ui.settings.gpu = false;
            let (connection, outgoing, _events) = network::test_connection();
            app.ai.next_connection = Some(connection);
            let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
            h.app.run("ai.open", json!({})).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                h.step();
                if h.app.run("ai.status", json!({})).unwrap()["historyLoading"] == false {
                    break;
                }
                assert!(Instant::now() < deadline, "saved history read did not retire");
                std::thread::yield_now();
            }
            for _ in 0..8 {
                h.step();
            }
            let status = h.app.run("ai.status", json!({})).unwrap();
            assert!(status["task"].is_null(), "unknown identity cannot be claimed as reliable previous activity: {status}");
            assert!(h.painted_text().iter().any(|text| text == "Readable unknown activity"));
            assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiTaskEntry"));
            assert!(outgoing.try_recv().is_err());
        }
    }
}

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
