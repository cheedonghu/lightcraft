//! Historical comparison through real archive and caller-facing navigation.
use super::*;
use crate::{Services, headless::Headless};
use lightcraft_catalog::MemStore;
use lightcraft_catalog::{FsStore, Store};
use lightcraft_engine::{Session, companion::EditorState, library::LibraryStores};
use std::sync::{Condvar, Mutex};

fn request(h: &mut Headless, requests: &mpsc::Receiver<network::Request>, suffix: &str) -> network::Request {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        h.step();
        if let Ok(request) = requests.try_recv() {
            assert!(request.path.ends_with(suffix), "unexpected {}", request.path);
            return request;
        }
        assert!(Instant::now() < deadline, "missing actual {suffix}");
        std::thread::yield_now();
    }
}

#[test]
fn actual_crop_execution_comparison_uses_sent_and_published_previews_after_later_edits() {
    for missing in [false, true] {
        actual_crop_case(missing);
    }
}
fn actual_crop_case(missing: bool) {
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "actual-comparison".into(), on_disk: false },
            true,
        )
        .unwrap();
    let path = if missing {
        let path = std::env::temp_dir().join(format!("lc-ux19-original-{}.png", LocalRoundId::new().unwrap().as_str()));
        let bytes = lightcraft_engine::export::encode_image(
            &lightcraft_raster::Rgba8::filled(320, 200, [96, 128, 160, 255]),
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
        )
        .unwrap();
        std::fs::write(&path, bytes).unwrap();
        let text = path.to_string_lossy().into_owned();
        let (loader, probe) = lightcraft_engine::files::fs_hooks();
        session.media.file_loader = Some(loader);
        session.media.file_probe = Some(probe);
        lightcraft_engine::import::import(&mut session, std::slice::from_ref(&text), lightcraft_engine::import::ImportMode::Add).unwrap();
        let id = session.catalog.photos().find(|photo| photo.source == lightcraft_catalog::Source::File { path: text.clone() }).unwrap().id;
        session.execute("library.select", &json!({"ids":[id.0],"active":id.0})).unwrap();
        Some(text)
    } else {
        None
    };
    let photo = session.active().unwrap();
    let (connection, requests, events) = network::test_connection();
    let mut app = LightcraftApp::new(session, Services::default());
    app.ai.next_connection = Some(connection);
    app.ui.settings.gpu = false;
    let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    for _ in 0..4 {
        h.step();
    }
    if let Some(path) = &path {
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.app.session.media.availability.exists(path) != Some(true) {
            assert!(Instant::now() < deadline);
            h.step();
            std::thread::yield_now();
        }
    }
    h.app.run("ai.start", json!({"prompt":"Crop and mirror naturally."})).unwrap();
    let creation = request(&mut h, &requests, "/v1/rounds");
    let phase = |name: &str| {
        events.send(network::Event::Server { kind:"round.state".into(),data:json!({"round_id":"actual-crop","payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"library_generation":creation.identity.scope.library_generation,"photo_id":photo.0},"state":name}}) }).unwrap()
    };
    phase("awaiting_input");
    let input = request(&mut h, &requests, "/input");
    phase("awaiting_initial_execution");
    let operation = json!({"type":"set_crop","rect":{"x0":0.15,"y0":0.2,"x1":0.85,"y1":0.8},"angle":8.0,"flip_h":true,"flip_v":false});
    events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"actual-crop","stage":"initial","payload":{"message_id":"crop-proposal","model_call_id":1,"tool_calls":[{"id":"crop-tool","name":"apply_edit_plan","arguments":{"operations":[operation]}}]}})}).unwrap();
    events.send(network::Event::Server {kind:"edit.plan".into(),data:json!({"round_id":"actual-crop","payload":{"plan_id":"crop-plan","tool_call_id":"crop-tool","capability_version":"lightcraft-v2","expected_revision":h.app.session.photo_revision(photo),"reason":"Keep the subject and balance the composition.","operations":[operation]}})}).unwrap();
    let execution = request(&mut h, &requests, "/execution");
    assert_eq!(execution.body["outcome"], "succeeded");
    phase("awaiting_correction_execution");
    let correction = json!({"type":"set_crop","rect":{"x0":0.25,"y0":0.1,"x1":0.75,"y1":0.9},"angle":0.0,"flip_h":false,"flip_v":true});
    events.send(network::Event::Server {kind:"message.completed".into(),data:json!({"round_id":"actual-crop","stage":"review","payload":{"message_id":"correction-proposal","model_call_id":2,"tool_calls":[{"id":"correction-tool","name":"apply_edit_plan","arguments":{"operations":[correction]}}]}})}).unwrap();
    events.send(network::Event::Server {kind:"edit.plan".into(),data:json!({"round_id":"actual-crop","payload":{"plan_id":"correction-plan","tool_call_id":"correction-tool","capability_version":"lightcraft-v2","expected_revision":if missing {0} else {h.app.session.photo_revision(photo)},"reason":"Refine composition.","operations":[correction]}})}).unwrap();
    let execution = if missing {
        // An obsolete correction revision is rejected before rendering and
        // ends this local task via Stop; it has no execution JPEG to pair.
        request(&mut h, &requests, "/stop");
        execution
    } else {
        let correction_execution = request(&mut h, &requests, "/execution");
        assert_eq!(correction_execution.body["outcome"], "succeeded");
        correction_execution
    };
    if !missing {
        h.app.run("ai.compare", json!({"local_round_id":creation.identity.local_round_id})).unwrap();
    }
    events
        .send(network::Event::Server { kind: "round.ended".into(), data: json!({"round_id":"actual-crop","payload":{"terminal_state":"completed"}}) })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.app.run("ai.status", json!({})).unwrap()["busy"] != false {
        assert!(Instant::now() < deadline);
        h.step();
    }
    if !missing {
        assert_eq!(
            h.app.run("ai.status", json!({})).unwrap()["task"]["unread"],
            true,
            "a historical canvas must not consume a newly completed task reminder without showing its conversation"
        );
        h.app.run("ai.returnCurrent", json!({})).unwrap();
        h.app.run("ai.bottom", json!({})).unwrap();
        for _ in 0..5 {
            h.step();
        }
        super::panel_tests::reachable(&mut h, &format!("button:aiProcess:{}", creation.identity.local_round_id.as_str()), 1.0);
        h.request("ui.clickWidget", json!({"id":format!("button:aiProcess:{}", creation.identity.local_round_id.as_str())}), Duration::from_secs(5));
        let preview = format!("image:aiAttachment:{}-correction:0", creation.identity.local_round_id.as_str());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !h.app.widgets.iter().any(|(id, _)| id == &preview) {
            assert!(Instant::now() < deadline, "the actually saved current-tail correction JPEG must remain viewable before a cold history reload");
            h.step();
            assert!(
                !h.painted_text().iter().any(|text| text == tr("Historical preview unavailable")),
                "a valid just-published preview must not be routed through a legacy/base-prefix reader"
            );
        }
        let shadow = h.app.headless_screenshot(&h.view.ctx, true).unwrap();
        let center = h.app.widgets.iter().find(|(id, _)| id == &preview).unwrap().1.center();
        let actual = shadow.pixels[center.y as usize * shadow.size[0] + center.x as usize];
        let bytes = Base64::decode_vec(execution.body["preview"]["data_base64"].as_str().unwrap()).unwrap();
        let decoded = lightcraft_codecs::decode(&bytes, lightcraft_codecs::DecodeOptions { max_size: Some((640, 640)), max_pixels: 640 * 640 })
            .unwrap()
            .to_srgb8();
        let raw = decoded.as_bytes();
        let index = ((decoded.height / 2) * decoded.width + decoded.width / 2) as usize * 4;
        for (got, wanted) in [actual.r(), actual.g(), actual.b()].into_iter().zip(&raw[index..index + 3]) {
            assert!(got.abs_diff(*wanted) <= 12, "native screenshot shadow must preserve the actually sent current-tail JPEG: {actual:?}");
        }
    }
    h.app.run("crop.reset", json!({})).unwrap();
    h.app.run("develop.set", json!({"control":"light.exposure","value":0.8})).unwrap();
    let current = h.app.session.develop_of(photo).unwrap();
    h.app.run("ai.compare", json!({"local_round_id":creation.identity.local_round_id})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["comparison"]["loading"] == false {
            assert_eq!(status["comparison"]["before"]["width"], input.body["preview"]["width"]);
            assert_eq!(status["comparison"]["before"]["height"], input.body["preview"]["height"]);
            assert_eq!(status["comparison"]["after"]["width"], execution.body["preview"]["width"]);
            assert_eq!(status["comparison"]["after"]["height"], execution.body["preview"]["height"]);
            assert_ne!(
                (&input.body["preview"]["width"], &input.body["preview"]["height"]),
                (&execution.body["preview"]["width"], &execution.body["preview"]["height"]),
                "actual crop must change the captured full-frame composition"
            );
            break;
        }
        assert!(Instant::now() < deadline, "{status}");
    }
    assert_eq!(h.app.session.develop_of(photo).unwrap(), current);
    let saved = h.app.session.companion_messages_page(photo, 0, 32).unwrap().records;
    let painted = h.paint();
    for (id, sent) in [("image:aiCompareBefore", &input.body), ("image:aiCompareAfter", &execution.body)] {
        let bytes = Base64::decode_vec(sent["preview"]["data_base64"].as_str().unwrap()).unwrap();
        assert!(
            saved.iter().flat_map(|m| m["images"].as_array().into_iter().flatten()).any(|image| h
                .app
                .session
                .read_companion_historical_preview(photo, image)
                .is_ok_and(|stored| stored == bytes)),
            "the exact actual transmitted JPEG must remain archived"
        );
        let decoded = lightcraft_codecs::decode(&bytes, lightcraft_codecs::DecodeOptions { max_size: Some((640, 640)), max_pixels: 640 * 640 })
            .unwrap()
            .to_srgb8();
        let raw = decoded.as_bytes();
        let pixel = ((decoded.height / 2) * decoded.width + decoded.width / 2) as usize * 4;
        let rect = h.app.widgets.iter().find(|(key, _)| key == id).unwrap().1;
        let point = rect.center();
        let actual = painted.pixels[point.y as usize * painted.size[0] + point.x as usize];
        for (got, wanted) in [actual.r(), actual.g(), actual.b()].into_iter().zip(&raw[pixel..pixel + 3]) {
            assert!(got.abs_diff(*wanted) <= 12, "the actual sent/published JPEG must supply visible historical pixels");
        }
    }
    h.app.run("ai.returnCurrent", json!({})).unwrap();
    assert_eq!(h.app.session.develop_of(photo).unwrap(), current);
    if let Some(path) = &path {
        std::fs::remove_file(path).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while h.app.session.media.availability.exists(path) != Some(false) {
            assert!(Instant::now() < deadline);
            h.step();
            std::thread::yield_now();
        }
        let other = h.app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
        h.app.run("library.select", json!({"ids":[other.0],"active":other.0})).unwrap();
    } else {
        h.app.run("photo.delete", json!({"ids":[photo.0]})).unwrap();
    }
    let selected = h.app.session.active().unwrap();
    assert_ne!(selected, photo);
    let selected_effect = h.app.session.develop_of(selected).unwrap();
    h.app.run("ai.task", json!({})).unwrap();
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["readonlyPhoto"], photo.0);
    h.app.run("ai.compare", json!({"local_round_id":creation.identity.local_round_id})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.app.run("ai.status", json!({})).unwrap()["comparison"]["loading"] != false {
        assert!(Instant::now() < deadline);
        h.step();
        std::thread::yield_now();
    }
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["comparison"]["before"]["available"], true, "Recently Deleted does not invalidate an owned saved historical preview");
    assert_eq!(status["comparison"]["after"]["available"], true);
    assert!(h.app.run("ai.start", json!({})).is_err());
    assert!(h.app.run("ai.undo", json!({"photo":photo.0,"local_round_id":creation.identity.local_round_id})).is_err());
    assert!(h.app.run("develop.set", json!({"ids":[photo.0],"values":{"light.exposure":0.3}})).is_err());
    assert_eq!(h.app.session.active(), Some(selected));
    assert_eq!(h.app.session.develop_of(selected).unwrap(), selected_effect);
    h.app.run("ai.task", json!({})).unwrap();
    h.step();
    assert!(
        h.app.run("ai.status", json!({})).unwrap()["comparison"].is_null(),
        "explicit task navigation leaves its historical preview even when an unavailable task does not change selection"
    );
    h.app.run("ai.compare", json!({"local_round_id":creation.identity.local_round_id})).unwrap();
    let captured = h.app.run("ai.clear", json!({})).unwrap();
    let cleared=h.app.run("ai.clear",json!({"confirm":true,"photo":captured["photo"],"library_generation":captured["libraryGeneration"],"expected_epoch":captured["expectedEpoch"]})).unwrap();
    assert_eq!(cleared["cleared"], true);
    h.step();
    assert!(h.app.run("ai.status", json!({})).unwrap()["comparison"].is_null(), "confirmed new-epoch clear must retire the old historical view");
    assert_eq!(h.app.session.develop_of(photo).unwrap(), current);
}

#[derive(Default)]
struct PreviewGate {
    entered: bool,
    released: bool,
    used: bool,
}
struct PausedPreviews {
    native: FsStore,
    gate: Arc<(Mutex<PreviewGate>, Condvar)>,
}
impl PausedPreviews {
    fn observe(&self, name: &str) {
        if !name.contains("/previews/") {
            return;
        }
        let (mutex, changed) = &*self.gate;
        let mut gate = mutex.lock().unwrap();
        if gate.used {
            return;
        }
        gate.used = true;
        gate.entered = true;
        changed.notify_all();
        while !gate.released {
            gate = changed.wait(gate).unwrap();
        }
    }
}
impl Store for PausedPreviews {
    fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.observe(name);
        self.native.read(name)
    }
    fn read_limited(&mut self, name: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.observe(name);
        self.native.read_limited(name, max)
    }
    fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.observe(name);
        self.native.read_range(name, offset, length)
    }
    fn source_snapshot(&mut self, name: &str) -> std::io::Result<Option<Box<dyn lightcraft_catalog::SourceSnapshot>>> {
        self.native.source_snapshot(name)
    }
    fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.native.write_atomic(name, bytes)
    }
    fn write_atomic_with(&mut self, name: &str, fill: &mut dyn FnMut(&mut dyn std::io::Write) -> std::io::Result<()>) -> std::io::Result<u64> {
        self.native.write_atomic_with(name, fill)
    }
    fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.native.append(name, bytes)
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
    fn background_writer(&self) -> Option<Box<dyn Store>> {
        Some(Box::new(Self { native: FsStore::open(self.native.dir()).ok()?, gate: self.gate.clone() }))
    }
    fn migration_writer(&self) -> Option<Box<dyn Store>> {
        self.background_writer()
    }
}

#[test]
fn native_comparison_io_never_blocks_frames_or_navigation_and_cannot_reappear_after_return() {
    let directory = std::env::temp_dir().join(format!("lc-ux19-read-{}", LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir(&directory).unwrap();
    let mut session = Session::new().with_fs();
    session.open_library(directory.clone(), true).unwrap();
    let photo = session.active().unwrap();
    let other = session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    let round = LocalRoundId::new().unwrap();
    let before_id = format!("{}-user", round.as_str());
    let after_id = format!("{}-initial", round.as_str());
    let original = session.develop_of(photo).unwrap();
    let bytes = lightcraft_engine::export::encode_image(
        &lightcraft_raster::Rgba8::filled(64, 32, [210, 40, 40, 255]),
        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Jpeg, quality: 75, ..Default::default() },
    )
    .unwrap();
    let image = json!({"mime_type":"image/jpeg","data":Base64::encode_string(&bytes)});
    session.save_companion_conversation(photo,json!({"schema_version":2,"messages":[
        {"role":"user","text":"Stored before","local_round_id":round,"local_message_id":before_id,"display_kind":"request","complete_state":"complete","images":[image]},
        {"role":"user","text":"Stored after","local_round_id":round,"local_message_id":after_id,"display_kind":"execution_feedback","complete_state":"complete","images":[image]}
    ],"rounds":[],"round_records":[{"local_round_id":round,"photo_id":photo.0,"before_preview_message_id":before_id,"original":original,"local_terminal":"completed","stages":[{"stage":"initial","before_state":original,"applied_state":original,"locally_published":true,"preview_message_id":after_id}]}]})).unwrap();
    session.close_library().unwrap();
    drop(session);
    let gate = Arc::new((Mutex::new(PreviewGate::default()), Condvar::new()));
    let gate_ui = gate.clone();
    let worker_directory = directory.clone();
    let (sent, received) = mpsc::sync_channel(1);
    let ui = std::thread::spawn(move || {
        let mut session = Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(FsStore::open(&worker_directory).unwrap()),
                    files: Box::new(PausedPreviews { native: FsStore::open(&worker_directory).unwrap(), gate: gate_ui.clone() }),
                    dir: worker_directory,
                    on_disk: true,
                },
                false,
            )
            .unwrap();
        let mut app = LightcraftApp::new(session, Services::default());
        app.ui.settings.gpu = false;
        let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
        // Leave AI Tools closed so its attachment decoder cannot consume the
        // physical comparison gate before the viewer requests a preview.
        h.app.run("ai.compare", json!({"local_round_id":round})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !gate_ui.0.lock().unwrap().entered && Instant::now() < deadline {
            h.step();
            std::thread::yield_now();
        }
        assert!(gate_ui.0.lock().unwrap().entered);
        h.app.run("develop.set", json!({"ids":[other.0],"values":{"light.exposure":0.6}})).unwrap();
        h.app.run("ai.returnCurrent", json!({})).unwrap();
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        let navigated = status["comparison"].is_null() && status["comparisonWorkerBusy"] == true;
        h.app.run("ai.compare", json!({"local_round_id":round})).unwrap();
        h.step();
        assert_eq!(
            h.app.run("ai.status", json!({})).unwrap()["comparison"]["loading"],
            true,
            "a replacement view must wait for the actual old reader"
        );
        assert!(!h.app.widgets.iter().any(|(id, _)| id == "image:aiCompareBefore"));
        sent.send(navigated).unwrap();
        while !gate_ui.0.lock().unwrap().released && Instant::now() < deadline {
            h.step();
            std::thread::yield_now();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.app.run("ai.status", json!({})).unwrap()["comparisonWorkerBusy"] == true && Instant::now() < deadline {
            h.step();
            std::thread::yield_now();
        }
        assert_eq!(
            h.app.run("ai.status", json!({})).unwrap()["comparisonWorkerBusy"],
            false,
            "actual old native reader must retire after physical release"
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.app.run("ai.status", json!({})).unwrap()["comparison"]["loading"] != false && Instant::now() < deadline {
            h.step();
            std::thread::yield_now();
        }
        assert_eq!(h.app.run("ai.status", json!({})).unwrap()["comparison"]["before"]["width"], 64);
        h.app.run("ai.returnCurrent", json!({})).unwrap();
        h.step();
        assert!(h.app.run("ai.status", json!({})).unwrap()["comparison"].is_null());
        assert!(!h.app.widgets.iter().any(|(id, _)| id == "image:aiCompareBefore" || id == "image:aiCompareAfter"));
        assert_eq!(h.app.session.develop_of(other).unwrap().light.exposure, 0.6);
        h.app.session.close_library().unwrap();
    });
    let (mutex, changed) = &*gate;
    let state = mutex.lock().unwrap();
    let (state, _) = changed.wait_timeout_while(state, Duration::from_secs(10), |state| !state.entered).unwrap();
    let entered = state.entered;
    drop(state);
    let navigation = if entered { received.recv_timeout(Duration::from_secs(1)) } else { Err(mpsc::RecvTimeoutError::Timeout) };
    // Release real IO and join even on the red path; the test must not leave a
    // blocked native reader holding the temporary library after it fails.
    mutex.lock().unwrap().released = true;
    changed.notify_all();
    ui.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match std::fs::remove_dir_all(&directory) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty && Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => panic!("temporary native library cleanup failed: {error}"),
        }
    }
    assert!(entered, "the actual preview read must have reached the hardware gate");
    assert!(matches!(navigation, Ok(true)), "public frames/navigation waited for paused native preview IO: {navigation:?}");
}

#[test]
fn historical_round_comparison_keeps_full_composition_and_returns_without_replaying_effects() {
    archived_comparison(false);
}

#[test]
fn bounded_store_comparison_preserves_the_same_pair_and_navigation() {
    archived_comparison(true);
}

// A real second Store adapter: bounded snapshots work, but no owned handle can
// be transferred to a native producer. It exercises the GUI scan continuation.
struct BoundedStore(MemStore);
impl Store for BoundedStore {
    fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.0.read(name)
    }
    fn read_limited(&mut self, name: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.0.read_limited(name, max)
    }
    fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.0.read_range(name, offset, length)
    }
    fn source_snapshot(&mut self, name: &str) -> std::io::Result<Option<Box<dyn lightcraft_catalog::SourceSnapshot>>> {
        self.0.source_snapshot(name)
    }
    fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.0.write_atomic(name, bytes)
    }
    fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.0.append(name, bytes)
    }
    fn truncate(&mut self, name: &str, length: u64) -> std::io::Result<()> {
        self.0.truncate(name, length)
    }
    fn remove(&mut self, name: &str) -> std::io::Result<()> {
        self.0.remove(name)
    }
    fn retention_namespace(&self) -> Option<String> {
        self.0.retention_namespace()
    }
    fn describe(&self) -> String {
        "bounded comparison store".into()
    }
}

#[test]
fn historical_comparison_never_pairs_a_valid_other_photo_attachment() {
    for bounded in [false, true] {
        let files = MemStore::new();
        let mut session = Session::new();
        session
            .open_library_in(
                LibraryStores {
                    catalog: Box::new(MemStore::new()),
                    files: if bounded { Box::new(BoundedStore(files)) } else { Box::new(files) },
                    dir: "comparison-ownership".into(),
                    on_disk: false,
                },
                true,
            )
            .unwrap();
        let photo = session.active().unwrap();
        let other = session.catalog.photos().find(|p| p.id != photo).unwrap().id;
        let bytes = lightcraft_engine::export::encode_image(
            &lightcraft_raster::Rgba8::filled(64, 32, [80, 160, 200, 255]),
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Jpeg, quality: 75, ..Default::default() },
        )
        .unwrap();
        let image = json!({"mime_type":"image/jpeg","data":Base64::encode_string(&bytes)});
        session
            .save_companion_conversation(
                other,
                json!({"schema_version":2,"messages":[{"role":"user","text":"another photo","images":[image]}],"rounds":[],"round_records":[]}),
            )
            .unwrap();
        let foreign = session.companion_messages_page(other, 0, 1).unwrap().records[0]["images"][0].clone();
        let round = LocalRoundId::new().unwrap();
        let before = format!("{}-user", round.as_str());
        let after = format!("{}-initial", round.as_str());
        let original = session.develop_of(photo).unwrap();
        let mut archive = json!({"schema_version":2,"messages":[
            {"role":"user","text":"own before","local_round_id":round,"local_message_id":before,"complete_state":"complete","display_kind":"request","images":[image]},
            {"role":"user","text":"own after","local_round_id":round,"local_message_id":after,"complete_state":"complete","display_kind":"execution_feedback","images":[image]}
        ],"rounds":[],"round_records":[{"local_round_id":round,"photo_id":photo.0,"before_preview_message_id":before,"stages":[{"stage":"initial","locally_published":true,"before_state":original,"applied_state":original,"preview_message_id":after}]}]});
        session.save_companion_conversation(photo, archive.clone()).unwrap();
        archive["messages"][1]["images"][0] = foreign;
        assert!(session.save_companion_conversation(photo, archive).unwrap_err().to_string().contains("cross-photo or invalid record reference"));
        let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
        h.app.run("ai.compare", json!({"local_round_id":round})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while h.app.run("ai.status", json!({})).unwrap()["comparison"]["loading"] != false {
            assert!(Instant::now() < deadline);
            h.step();
            std::thread::yield_now();
        }
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(status["comparison"]["before"]["available"], true);
        assert_eq!(
            status["comparison"]["after"]["available"], true,
            "refusing a foreign reference must retain the previous complete pair (bounded={bounded})"
        );
    }
}

fn archived_comparison(bounded: bool) {
    let catalog = MemStore::new();
    let files = MemStore::new();
    let mut session = Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(catalog),
                files: if bounded { Box::new(BoundedStore(files.clone())) } else { Box::new(files.clone()) },
                dir: "comparison".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let round = LocalRoundId::new().unwrap();
    let original = session.develop_of(photo).unwrap();
    let mut actual = (*original).clone();
    actual.light.exposure = 0.35;
    let image = |width, height, color| {
        let bytes = lightcraft_engine::export::encode_image(
            &lightcraft_raster::Rgba8::filled(width, height, color),
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Jpeg, quality: 75, ..Default::default() },
        )
        .unwrap();
        json!({"mime_type":"image/jpeg","data":Base64::encode_string(&bytes)})
    };
    let before_id = format!("{}-user", round.as_str());
    let after_id = format!("{}-initial", round.as_str());
    session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[
                {"role":"user","text":format!("Saved request\n{}", "A long saved reading line.\n".repeat(120)),"local_round_id":round,"local_message_id":before_id,"display_kind":"request","complete_state":"complete","images":[image(640,320,[210,40,40,255])]},
                {"role":"user","text":"Saved complete execution","local_round_id":round,"local_message_id":after_id,"display_kind":"execution_feedback","complete_state":"complete","images":[image(320,640,[40,40,210,255])]}
            ],"rounds":[],"round_records":[{"local_round_id":round,"photo_id":photo.0,"before_preview_message_id":before_id,"original":original,"recent_complete_result":actual,"recent_complete_hash":actual.hash64(),"local_terminal":"completed","stages":[{"stage":"initial","plan_id":"saved-initial","reason":"Natural exposure","before_state":original,"applied_state":actual,"locally_published":true,"execution_outcome":"succeeded","preview_message_id":after_id}]}]}),
        )
        .unwrap();
    session.set_companion_editor(photo, EditorState { draft: "Keep the current unsent draft".into(), ..Default::default() }).unwrap();
    session.execute("develop.set", &json!({"control":"light.exposure","value":0.8})).unwrap();
    session.persist().unwrap();
    let current = session.develop_of(photo).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    h.app.run("ai.compare", json!({"local_round_id":round})).unwrap();
    if bounded {
        // Publication may advance while the captured comparison still has
        // bounded reads outstanding. Its old graph must remain readable.
        let header = h.app.session.companion_photo_header(photo).unwrap().unwrap();
        let mut messages = h.app.session.companion_messages_page(photo, 0, 2).unwrap().records;
        let record = h.app.session.companion_round(photo, round.as_str()).unwrap().unwrap();
        messages.push(json!({"role":"assistant","text":"A later saved response while historical reads are pending."}));
        h.app
            .session
            .save_companion_conversation(photo, json!({"schema_version":2,"messages":messages,"rounds":[],"round_records":[record]}))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while h.app.session.collect_companion(photo, 32).unwrap().pending {
            assert!(Instant::now() < deadline);
        }
        assert!(
            h.app.session.companion_messages_bounded(&header, 0, 1, 256 * 1024).is_ok(),
            "the captured comparison graph must survive later publication and actual reclamation"
        );
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["comparison"]["loading"] == false {
            assert_eq!(status["comparison"]["localRoundId"], round.as_str());
            assert_eq!(status["comparison"]["before"]["width"], 640);
            assert_eq!(status["comparison"]["before"]["height"], 320);
            assert_eq!(status["comparison"]["after"]["width"], 320);
            assert_eq!(status["comparison"]["after"]["height"], 640);
            break;
        }
        assert!(Instant::now() < deadline, "paired stored previews never became readable: {status}");
    }
    assert_eq!(
        h.app.run("ai.start", json!({"prompt":"cannot send from historical mode"})).unwrap_err(),
        tr("Return to the selected photo before starting a new AI task.")
    );
    assert_eq!(h.app.session.develop_of(photo).unwrap(), current);
    assert!(h.app.widgets.iter().any(|(id, _)| id == "image:aiCompareBefore"));
    assert!(h.app.widgets.iter().any(|(id, _)| id == "image:aiCompareAfter"));
    let shadow = h.app.headless_screenshot(&h.view.ctx, true).unwrap();
    for (id, expected) in [("image:aiCompareBefore", [210u8, 40, 40]), ("image:aiCompareAfter", [40u8, 40, 210])] {
        let center = h.app.widgets.iter().find(|(key, _)| key == id).unwrap().1.center();
        let pixel = shadow.pixels[center.y as usize * shadow.size[0] + center.x as usize];
        for (got, wanted) in [pixel.r(), pixel.g(), pixel.b()].into_iter().zip(expected) {
            assert!(got.abs_diff(wanted) <= 3, "native screenshot shadow must preserve captured JPEG pixels: {pixel:?}");
        }
    }
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        h.app.ui.language = language;
        for (name, width, height) in [("normal", 1400, 1000), ("narrow", 620, 520)] {
            assert_eq!(h.request("ui.resize", json!({"width":width,"height":height}), Duration::from_secs(5))["ok"], true);
            for _ in 0..4 {
                h.step();
            }
            let painted = h.paint();
            for (id, expected, aspect) in [("image:aiCompareBefore", [210u8, 40, 40], 2.0f32), ("image:aiCompareAfter", [40u8, 40, 210], 0.5f32)] {
                let rect = h.app.widgets.iter().find(|(key, _)| key == id).unwrap().1;
                assert!(
                    (rect.width() / rect.height() - aspect).abs() < 0.01,
                    "{language:?}/{name}: full historical composition must fit without clipping"
                );
                let center = rect.center();
                let color = painted.pixels[center.y as usize * painted.size[0] + center.x as usize];
                for (actual, wanted) in [color.r(), color.g(), color.b()].into_iter().zip(expected) {
                    assert!(
                        actual.abs_diff(wanted) <= 3,
                        "{language:?}/{name}: original JPEG pixels are required, not a placeholder or today's photo: {color:?}"
                    );
                }
            }
            assert_eq!(h.app.widgets.iter().filter(|(id, _)| id == "button:aiReturnCurrent").count(), 1, "one stable visible return target");
            assert!(h.painted_text().iter().any(|text| text == tr("Before this round")));
            assert!(h.painted_text().iter().any(|text| text == tr("After this round")));
            if let Ok(output) = std::env::var("UX19_SCREENSHOT_DIR") {
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
                        json!({"path":format!("{output}/comparison-{name}-{}.png",language.code()),"headless":true}),
                        Duration::from_secs(10)
                    )["ok"],
                    true
                );
            }
        }
    }
    h.app.ui.language = crate::i18n::Locale::En;
    h.request("ui.resize", json!({"width":1400,"height":1000}), Duration::from_secs(5));
    assert_eq!(h.app.run("view.fullScreenPreview", json!({})).unwrap()["fullscreen"], true);
    h.step();
    assert!(
        h.app.widgets.iter().any(|(id, _)| id == "image:aiCompareBefore"),
        "fullscreen must show the captured historical before, not today's loupe"
    );
    assert!(h.app.widgets.iter().any(|(id, _)| id == "image:aiCompareAfter"));
    assert_eq!(h.app.run("view.fullScreenPreview", json!({})).unwrap()["fullscreen"], false);
    h.app.run("ai.returnCurrent", json!({})).unwrap();
    h.step();
    assert!(h.app.run("ai.status", json!({})).unwrap()["comparison"].is_null());
    assert_eq!(h.app.session.companion_editor(photo).unwrap().draft, "Keep the current unsent draft");
    assert_eq!(h.app.session.develop_of(photo).unwrap(), current);

    if h.app.widgets.iter().any(|(id, _)| id == &format!("button:aiProcess:{}", round.as_str())) {
        h.request("ui.clickWidget", json!({"id":format!("button:aiProcess:{}",round.as_str())}), Duration::from_secs(5));
        for _ in 0..5 {
            h.step();
        }
    }
    // A second caller path is the visible card, with a real upward-reading
    // gesture and freshly typed multiline draft rather than default metadata.
    let timeout = Duration::from_secs(5);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout)["ok"], true);
    assert_eq!(h.request("ui.key", json!({"key":"A","cmd":true}), timeout)["ok"], true);
    assert_eq!(h.request("ui.text", json!({"text":"Keep every fresh draft line.\n".repeat(20)}), timeout)["ok"], true);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"ai:messages"}), timeout)["ok"], true);
    assert_eq!(h.request("ui.scroll", json!({"dy":-5000.0}), timeout)["ok"], true);
    assert_eq!(h.request("ui.scroll", json!({"dy":35.0}), timeout)["ok"], true);
    let button = format!("button:aiCompare:{}", round.as_str());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !h.app.widgets.iter().any(|(id, rect)| id == &button && rect.height() >= 12.0) {
        assert!(Instant::now() < deadline, "the paired round card has no visible historical comparison action");
        h.request("ui.clickWidget", json!({"id":"ai:messages"}), timeout);
        h.request("ui.scroll", json!({"dy":-100.0}), timeout);
        h.step();
    }
    for _ in 0..5 {
        h.step();
    }
    let editor = h.app.session.companion_editor(photo).unwrap();
    assert!(!editor.follow_tail, "actual upward reading must remain intentional");
    assert!(editor.scroll_offset > 0.0, "the fixture must exercise a real scrolled reading position");
    assert_eq!(h.request("ui.clickWidget", json!({"id":button}), timeout)["ok"], true);
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.app.run("ai.status", json!({})).unwrap()["comparison"]["loading"] != false {
        assert!(Instant::now() < deadline);
        h.step();
    }
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiStart"));
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "field:aiPrompt"));
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiReturnCurrent"}), timeout)["ok"], true);
    h.step();
    assert_eq!(h.app.session.companion_editor(photo).unwrap(), editor);
    assert_eq!(h.app.session.develop_of(photo).unwrap(), current);
    h.app.run("ai.compare", json!({"local_round_id":round})).unwrap();
    h.app.run("library.select", json!({"ids":[photo.0],"active":photo.0})).unwrap();
    h.step();
    assert!(
        h.app.run("ai.status", json!({})).unwrap()["comparison"].is_null(),
        "an explicit current-photo selection must leave historical mode even when the selection IDs are unchanged"
    );
    let messages = h.app.session.companion_messages_page(photo, 0, 2).unwrap().records;
    let key = messages[1]["images"][0]["attachment"].as_str().unwrap();
    files.set(key, b"damaged after JPEG".to_vec());
    h.app.run("ai.compare", json!({"local_round_id":round})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.app.run("ai.status", json!({})).unwrap()["comparison"]["loading"] != false {
        assert!(Instant::now() < deadline);
        h.step();
        std::thread::yield_now();
    }
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert_eq!(status["comparison"]["before"]["available"], true, "a damaged after must not discard its verified before");
    assert_eq!(status["comparison"]["after"]["available"], false);
    assert!(h.app.widgets.iter().any(|(id, _)| id == "image:aiCompareBefore"));
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "image:aiCompareAfter"), "today's current photo must never substitute for a missing after");
    assert!(h.painted_text().iter().any(|text| text == tr("Historical preview unavailable")));
    assert_eq!(h.app.session.develop_of(photo).unwrap(), current);
    h.app.run("ai.returnCurrent", json!({})).unwrap();
    let record = h.app.session.companion_round(photo, round.as_str()).unwrap().unwrap();
    let mut complete = json!({"schema_version":2,"messages":messages,"rounds":[],"round_records":[record]});
    complete["messages"][1]["images"] = json!([image(320, 640, [40, 40, 210, 255])]);
    for case in ["incomplete", "duplicate", "other_round"] {
        let mut archive = complete.clone();
        match case {
            "incomplete" => archive["messages"][1]["complete_state"] = json!("pending"),
            "duplicate" => {
                let duplicate = archive["messages"][1].clone();
                archive["messages"].as_array_mut().unwrap().push(duplicate);
            }
            _ => archive["messages"][1]["local_round_id"] = json!(LocalRoundId::new().unwrap()),
        }
        h.app.session.save_companion_conversation(photo, archive).unwrap();
        h.app.run("ai.compare", json!({"local_round_id":round})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while h.app.run("ai.status", json!({})).unwrap()["comparison"]["loading"] != false {
            assert!(Instant::now() < deadline);
            h.step();
            std::thread::yield_now();
        }
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert_eq!(status["comparison"]["before"]["available"], true, "{case}: independently readable before remains");
        assert_eq!(status["comparison"]["after"]["available"], false, "{case}: no pairing from an incomplete, ambiguous or foreign round identity");
        h.app.run("ai.returnCurrent", json!({})).unwrap();
        assert_eq!(h.app.session.develop_of(photo).unwrap(), current);
    }
}
