use super::*;
use crate::headless::Headless;
use lightcraft_catalog::MemStore;
use lightcraft_engine::library::LibraryStores;

fn history(messages: &[Value]) -> Headless {
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores { catalog: Box::new(MemStore::new()), files: Box::new(MemStore::new()), dir: "reading-21".into(), on_disk: false },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    session.save_companion_tail(photo, None, messages, &[], &[], &json!({"schema_version":2})).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        if h.app.run("ai.status", json!({})).unwrap()["historyLoading"] == false {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    for _ in 0..4 {
        h.step();
    }
    h
}

#[test]
fn assistant_markdown_renders_saved_and_streaming_text_without_rewriting_history() {
    let source = "### Light and shadow\n\nKeep **detail** and *natural colors*.\n\n> Preserve the mood.\n\n---\n\n1. Inspect the highlights\n2. Adjust gently\n\n| Area | Advice |\n| --- | --- |\n| Sky | Preserve texture |\n\n![Preview](https://example.com/preview.jpg)";
    let messages = vec![
        json!({"role":"user","text":"### My literal request","local_message_id":"markdown-user"}),
        json!({"role":"system","text":"### Literal status","local_message_id":"markdown-system"}),
        json!({"role":"assistant","text":source,"local_message_id":"markdown-assistant"}),
    ];
    let mut h = history(&messages);
    let painted = h.painted_text().join(" ");
    for expected in [
        "### My literal request",
        "### Literal status",
        "Light and shadow",
        "detail",
        "natural colors",
        "Preserve the mood.",
        "Area",
        "Advice",
        "Preserve texture",
        "Preview",
    ] {
        assert!(painted.contains(expected), "missing {expected}: {painted}");
    }
    assert!(!painted.contains("### Light") && !painted.contains("![Preview]"));
    let photo = h.app.session.active().unwrap();
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 3).unwrap().records, messages);
    h.app.ai.conversations.get_mut(&photo).unwrap().pending.insert("stream-markdown".into(), "### Arriving\n\n**still typing".into());
    for _ in 0..3 {
        h.step();
    }
    assert!(h.painted_text().join(" ").contains("Arriving"));
    h.app.ai.conversations.get_mut(&photo).unwrap().pending.insert("stream-markdown".into(), "### Arriving\n\n**complete**".into());
    for _ in 0..3 {
        h.step();
    }
    let painted = h.painted_text().join(" ");
    assert!(painted.contains("complete") && !painted.contains("### Arriving") && !painted.contains("**complete**"));
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 3).unwrap().records, messages);
}

#[test]
fn assistant_markdown_bilingual_panel_fixture() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let text = if language == crate::i18n::Locale::ZhHans {
            "### 上下两处亮点的视觉拉扯\n\n画面最抓人的是那棵嫩叶与地面被照亮的花丛。可以让其中一方成为**主角**，另一方作为氛围衬托。\n\n### 高光质感与暗部通透度\n\n适度保留亮部纹理，同时让树林中的层次更清晰。\n\n---\n\n### 可以尝试的后期与复拍思路\n\n1. **压住高光**：稍微降低高光与白色色阶。\n2. **理顺视觉引导**：适度提亮局部树干。\n\n> 下次取景时，可以尝试更低的机位。\n\n| 区域 | 调整方向 |\n| --- | --- |\n| 花丛 | 保留高光纹理 |\n| 树林 | 提升暗部层次 |"
        } else {
            "### Competing highlights\n\nThe young leaves and illuminated flowers both attract attention. Choose one as the **main subject**, and let the other support the atmosphere.\n\n### Highlight texture and shadow detail\n\nPreserve bright texture while making the woodland layers easier to read.\n\n---\n\n### Editing and shooting ideas\n\n1. **Control highlights**: gently lower highlights and whites.\n2. **Guide attention**: slightly lift a few tree trunks.\n\n> Next time, try a lower camera position.\n\n| Area | Direction |\n| --- | --- |\n| Flowers | Preserve highlight texture |\n| Woodland | Lift shadow detail |"
        };
        let mut h = history(&[
            json!({"role":"user","text":if language == crate::i18n::Locale::ZhHans { "请点评这张照片，给我一些构图和后期调整建议。" } else { "Please review this photo and suggest composition and editing improvements." },"local_message_id":"request-fixture"}),
            json!({"role":"assistant","text":text,"local_message_id":"markdown-fixture"}),
        ]);
        h.app.ui.language = language;
        for _ in 0..4 {
            h.step();
        }
        if let Ok(dir) = std::env::var("UX24_SCREENSHOT_DIR") {
            h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
            h.app.services.png = Some(Box::new(|image| {
                lightcraft_engine::export::encode_image(
                    image,
                    &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                )
                .unwrap()
            }));
            let reply =
                h.request("ui.screenshot", json!({"path":format!("{dir}/markdown-{}.png",language.code()),"headless":true}), Duration::from_secs(10));
            assert_eq!(reply["ok"], true, "{reply}");
        }
        assert!(!h.painted_text().join(" ").contains("###"));
    }
}

#[test]
fn chat_requests_align_right_and_replies_left_without_role_labels() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let request = if language == crate::i18n::Locale::ZhHans { "请点评这张照片。" } else { "Review this photo." };
        let reply = if language == crate::i18n::Locale::ZhHans { "光线自然，保留层次。" } else { "Keep the natural light." };
        let messages = vec![
            json!({"role":"user","text":request,"local_message_id":"aligned-user"}),
            json!({"role":"assistant","text":reply,"local_message_id":"aligned-assistant"}),
        ];
        let mut h = history(&messages);
        h.app.ui.language = language;
        for width in [1000.0, 1400.0] {
            h.size.x = width;
            for _ in 0..5 {
                h.step();
            }
            let painted = h.painted_text();
            for removed in ["You", "Assistant", "你", "助手", "Ready for your request", "可以输入要求"] {
                assert!(!painted.iter().any(|text| text == removed), "unexpected label: {removed}");
            }
            let rect = |literal: &str| {
                h.view
                    .painted_shapes()
                    .iter()
                    .find_map(|shape| match shape {
                        egui::epaint::Shape::Text(text) if text.galley.text() == literal => Some(text.visual_bounding_rect()),
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("missing chat text: {literal}"))
            };
            let user = rect(request);
            let assistant = rect(reply);
            let pane = h.app.widgets.iter().find(|(id, _)| id == "ai:messages").unwrap().1;
            assert!(user.left() > assistant.left() + pane.width() * 0.1, "user={user:?}, assistant={assistant:?}, pane={pane:?}");
            let bubble = h
                .view
                .painted_shapes()
                .iter()
                .find_map(|shape| match shape {
                    egui::epaint::Shape::Rect(rect) if rect.corner_radius == egui::CornerRadius::same(12) && rect.rect.contains_rect(user) => {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .unwrap();
            let right_anchor = h
                .view
                .painted_shapes()
                .iter()
                .find_map(|shape| match shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == request => {
                        assert_eq!(text.galley.job.halign, egui::Align::Max);
                        Some(text.pos.x)
                    }
                    _ => None,
                })
                .unwrap();
            // Use the layout anchor: Chinese full stops have blank space after their ink.
            assert!((bubble.right() - 10.0 - right_anchor).abs() <= 1.0, "request anchor {right_anchor} in {bubble:?}");
            assert!(user.right() <= pane.right() && assistant.right() <= pane.right());
            assert!(user.bottom() < assistant.top(), "message order must remain chronological");
            assert!(h.app.ai.prompt.is_empty(), "opening history must not prefill a request");
        }
        let photo = h.app.session.active().unwrap();
        assert_eq!(h.app.session.companion_messages_page(photo, 0, 2).unwrap().records, messages);
    }
}

#[test]
fn chat_preview_appears_above_right_aligned_text_for_portrait_and_landscape_photos() {
    for (width, height) in [(80, 120), (160, 80)] {
        let rgba = lightcraft_raster::Rgba8 { width, height, data: vec![[70, 120, 90, 255]; (width * height) as usize] };
        let jpeg = lightcraft_engine::export::encode_image(
            &rgba,
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Jpeg, ..Default::default() },
        )
        .unwrap();
        let request = "Please review my photo.\nAnd its composition.";
        let message = json!({"role":"user","text":request,"local_message_id":"aligned-preview","images":[{"mime_type":"image/jpeg","data":Base64::encode_string(&jpeg)}]});
        let mut h = history(&[message]);
        let deadline = Instant::now() + Duration::from_secs(10);
        let image = loop {
            h.step();
            if let Some((_, rect)) = h.app.widgets.iter().find(|(id, _)| id == "image:aiAttachment:aligned-preview:0") {
                break *rect;
            }
            assert!(Instant::now() < deadline, "chat preview did not load");
            std::thread::yield_now();
        };
        let text = h
            .view
            .painted_shapes()
            .iter()
            .find_map(|shape| match shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == request => {
                    assert_eq!(text.galley.job.halign, egui::Align::Max, "all lines must align right");
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap();
        // Glyph bearings can inset visible ink by one pixel from the label's layout edge.
        assert!((image.right() - text.right()).abs() <= 1.5, "text={text:?}, image={image:?}");
        assert!(image.bottom() <= text.top(), "the photo must appear before the request: image={image:?}, text={text:?}");
        assert!((image.width() / image.height() - width as f32 / height as f32).abs() < 0.01);
        super::redesign_tests::screenshots(&mut h, &format!("photo-before-text-{width}x{height}"));
    }
}

#[test]
fn saved_history_displays_markdown_and_folds_tool_payload_without_changing_records() {
    let messages = vec![
        json!({"role":"user","text":"My request","local_message_id":"u"}),
        json!({"role":"assistant","text":"**Exposure**\n- Preserve sky\n1. Keep detail","local_message_id":"a","tool_calls":[{"id":"t","name":"get_current_state","arguments":"{\"private_payload\":12345}"}]}),
        json!({"role":"tool","text":"{\"secret_result\":67890}","tool_call_id":"t","local_message_id":"t"}),
        json!({"role":"system","text":"A saved system notice","local_message_id":"s"}),
        json!({"role":"user","display_kind":"execution_feedback","text":"Actual execution feedback: {\"outcome\":\"succeeded\",\"actual_state\":{\"private_settings\":987}}","local_message_id":"e"}),
    ];
    let mut h = history(&messages);
    let text = h.painted_text().join("\n");
    for expected in ["System", "Execution feedback", "Exposure", "Preserve sky", "Keep detail", "A saved system notice", "Edits applied."] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(!text.contains("**Exposure**"));
    assert!(text.contains("1."));
    assert!(!text.contains("private_payload") && !text.contains("secret_result") && !text.contains("private_settings"));
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiTool:a"}), Duration::from_secs(10))["ok"], true);
    h.step();
    let expanded = h.painted_text().join("\n");
    assert!(expanded.contains("Read photo settings"));
    assert!(!expanded.contains("private_payload") && !expanded.contains("get_current_state"));
    let photo = h.app.session.active().unwrap();
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 5).unwrap().records, messages);
}

#[test]
fn expanded_tool_activity_never_paints_prompts_arguments_or_raw_results_in_either_language() {
    let messages = vec![
        json!({"role":"assistant","text":"Checking the photo.","local_message_id":"tool-summary-call","tool_calls":[
            {"id":"skill-call","name":"skill","arguments":"{\"skill\":\"private_argument_marker\"}"},
            {"id":"unknown-call","function":{"name":"private_tool_name_marker","arguments":{"prompt":"private_object_argument_marker"}}}
        ]}),
        json!({"role":"tool","local_message_id":"tool-summary-result","tool_call_id":"skill-call","text":"{\"content\":\"private_prompt_marker: internal photography guidance\"}"}),
        json!({"role":"tool","local_message_id":"tool-summary-unknown","tool_call_id":"unknown-call","text":"private_result_marker: arbitrary tool output"}),
        json!({"role":"assistant","local_message_id":"public-answer","text":"Keep the natural colors."}),
    ];
    for locale in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let mut h = history(&messages);
        h.app.ui.language = locale;
        for _ in 0..4 {
            h.step();
        }
        for id in ["tool-summary-call", "tool-summary-result", "tool-summary-unknown"] {
            let reply = h.request("ui.clickWidget", json!({"id":format!("button:aiTool:{id}")}), Duration::from_secs(10));
            assert_eq!(reply["ok"], true, "{reply}");
            for _ in 0..4 {
                h.step();
            }
            assert!(!h.painted_text().join("\n").contains("private_"), "expanded tool details must remain a user-facing summary");
        }
        let text = h.painted_text().join("\n");
        for label in if locale == crate::i18n::Locale::ZhHans {
            ["读取摄影指导", "调用照片工具", "工具返回内容已记录。"]
        } else {
            ["Read photography guidance", "Use a photo tool", "Tool response recorded."]
        } {
            assert!(text.contains(label), "missing {label}: {text}");
        }
        assert!(text.contains("Keep the natural colors."));
        super::redesign_tests::screenshots(&mut h, &format!("tool-summaries-{}", locale.code()));
        let photo = h.app.session.active().unwrap();
        assert_eq!(h.app.session.companion_messages_page(photo, 0, 4).unwrap().records, messages, "model context must retain original tool data");
    }
}

#[test]
fn upward_reading_survives_stream_completion_until_explicit_back_to_bottom() {
    let messages:Vec<_>=(0..32).map(|n|json!({"role":"assistant","text":format!("Saved paragraph {n}\n{}","Readable history sentence. ".repeat(12)),"local_message_id":format!("history-{n}")})).collect();
    let mut h = history(&messages);
    let photo = h.app.session.active().unwrap();
    let (connection, requests, events) = network::test_connection();
    h.app.ai.connection = Some(connection); // varying external transport adapter
    h.app.ai.generation = h.app.session.library_generation();
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), Duration::from_secs(10))["ok"], true);
    h.app.run("ai.start", json!({"prompt":"Read this photo"})).unwrap();
    let creation = native_history_tests::outgoing(&mut h, &requests, "/v1/rounds");
    events.send(network::Event::Server{kind:"round.state".into(),data:json!({"round_id":"reading-round","payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":creation.identity.scope.library_generation},"state":"queued"}})}).unwrap();
    h.step();
    let rect = h.app.widgets.iter().find(|(id, _)| id == "ai:messages").unwrap().1;
    assert!(h.app.session.companion_editor(photo).unwrap().follow_tail, "fixture begins following");
    assert!(
        h.app.session.companion_editor(photo).unwrap().scroll_offset > 1000.0,
        "fixture must begin at a genuinely scrolled history tail, not an unscrolled loading shell: {:?}",
        h.app.session.companion_editor(photo).unwrap()
    );
    for _ in 0..1 {
        h.app.synthetic.push(egui::Event::PointerMoved(rect.center()));
        h.step();
        h.app.synthetic.push(egui::Event::MouseWheel {
            phase: egui::TouchPhase::Move,
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 250.0),
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    assert!(!h.app.session.companion_editor(photo).unwrap().follow_tail, "one upward event must immediately stop following before inertia settles");
    let mut stable = 0;
    let mut previous = h.app.session.companion_editor(photo).unwrap().scroll_offset;
    for _ in 0..120 {
        h.step();
        let current = h.app.session.companion_editor(photo).unwrap().scroll_offset;
        stable = if (current - previous).abs() < 0.1 { stable + 1 } else { 0 };
        previous = current;
        if stable >= 4 {
            break;
        }
    }
    assert!(stable >= 4, "actual wheel inertia must settle before measuring a forced stream/terminal move");
    let before = h.app.session.companion_editor(photo).unwrap();
    assert!(!before.follow_tail);
    assert!(h.app.widgets.iter().any(|(id, _)| id == "button:aiBackToBottom"), "upward reading must offer explicit return");
    for (kind, payload) in [
        ("text.delta", json!({"message_id":"answer","delta":"A streaming answer. ".repeat(200)})),
        ("message.completed", json!({"message_id":"answer","tool_calls":[]})),
        ("round.ended", json!({"terminal_state":"completed"})),
    ] {
        events.send(network::Event::Server { kind: kind.into(), data: json!({"round_id":"reading-round","payload":payload}) }).unwrap();
        for _ in 0..3 {
            h.step();
        }
    }
    assert_eq!(h.app.run("ai.status", json!({})).unwrap()["busy"], false, "the real terminal event must end the round");
    let after = h.app.session.companion_editor(photo).unwrap();
    assert!(!after.follow_tail, "terminal completion cannot reclaim follow intent");
    assert!(
        (after.scroll_offset - before.scroll_offset).abs() < 2.0,
        "stream/terminal moved reading position: {} → {}",
        before.scroll_offset,
        after.scroll_offset
    );
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiBackToBottom"}), Duration::from_secs(10))["ok"], true);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        if h.app.run("ai.status", json!({})).unwrap()["historyLoading"] == false {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    for _ in 0..5 {
        h.step();
    }
    assert!(h.app.session.companion_editor(photo).unwrap().follow_tail);
    assert!(!h.app.widgets.iter().any(|(id, _)| id == "button:aiBackToBottom"));
}

struct ObservedFiles {
    files: Box<dyn lightcraft_catalog::Store>,
    reads: Arc<std::sync::Mutex<BTreeMap<String, usize>>>,
    owned: bool,
    gate: Option<Arc<PreviewGate>>,
}
struct PreviewGate {
    once: AtomicBool,
    started: mpsc::SyncSender<()>,
    release: std::sync::Mutex<mpsc::Receiver<()>>,
}
impl ObservedFiles {
    fn observe(&self, name: &str) {
        if name.contains("/previews/") {
            *self.reads.lock().unwrap().entry(name.into()).or_default() += 1;
            if let Some(gate) = &self.gate
                && gate.once.swap(false, Ordering::Relaxed)
            {
                gate.started.send(()).unwrap();
                gate.release.lock().unwrap().recv_timeout(Duration::from_secs(15)).unwrap();
            }
        }
    }
}
impl lightcraft_catalog::Store for ObservedFiles {
    fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.observe(name);
        self.files.read(name)
    }
    fn read_limited(&mut self, name: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.observe(name);
        self.files.read_limited(name, max)
    }
    fn read_range(&mut self, name: &str, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
        self.observe(name);
        self.files.read_range(name, offset, len)
    }
    fn write_atomic(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.files.write_atomic(name, bytes)
    }
    fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.files.append(name, bytes)
    }
    fn remove(&mut self, name: &str) -> std::io::Result<()> {
        self.files.remove(name)
    }
    fn truncate(&mut self, name: &str, len: u64) -> std::io::Result<()> {
        self.files.truncate(name, len)
    }
    fn describe(&self) -> String {
        self.files.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.files.retention_namespace()
    }
    fn conservative_retention(&self) -> bool {
        self.files.conservative_retention()
    }
    fn source_snapshot(&mut self, name: &str) -> std::io::Result<Option<Box<dyn lightcraft_catalog::SourceSnapshot>>> {
        self.files.source_snapshot(name)
    }
    fn copy_source_atomic(&mut self, name: &str, source: &mut dyn lightcraft_catalog::SourceSnapshot) -> std::io::Result<()> {
        self.files.copy_source_atomic(name, source)
    }
    fn replace_source_atomic(&mut self, name: &str, expected: &lightcraft_catalog::SourceIdentity, bytes: &[u8]) -> std::io::Result<()> {
        self.files.replace_source_atomic(name, expected, bytes)
    }
    fn replace_source_atomic_authorized(
        &mut self,
        name: &str,
        expected: &lightcraft_catalog::SourceIdentity,
        bytes: &[u8],
        authorized: &dyn Fn() -> bool,
    ) -> std::io::Result<()> {
        self.files.replace_source_atomic_authorized(name, expected, bytes, authorized)
    }
    fn migration_writer(&self) -> Option<Box<dyn lightcraft_catalog::Store>> {
        self.files.migration_writer().map(|files| {
            Box::new(Self { files, reads: self.reads.clone(), owned: self.owned, gate: self.gate.clone() }) as Box<dyn lightcraft_catalog::Store>
        })
    }
    fn background_writer(&self) -> Option<Box<dyn lightcraft_catalog::Store>> {
        if !self.owned {
            return None;
        }
        self.files.background_writer().map(|files| {
            Box::new(Self { files, reads: self.reads.clone(), owned: true, gate: self.gate.clone() }) as Box<dyn lightcraft_catalog::Store>
        })
    }
}
#[test]
fn native_previews_load_for_visible_rows_and_missing_preview_is_local() {
    use lightcraft_catalog::{FsStore, Store};
    let dir = std::env::temp_dir().join(format!("lightcraft-reading-21-{}", LocalRoundId::new().unwrap().as_str()));
    let reads = Arc::new(std::sync::Mutex::new(BTreeMap::new()));
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(ObservedFiles { files: Box::new(FsStore::open(&dir).unwrap()), reads: reads.clone(), owned: true, gate: None }),
                dir: dir.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let messages:Vec<_>=(0..32).map(|n|{
        let rgba=lightcraft_raster::Rgba8{width:32,height:24,data:vec![[n as u8*7,30,150,255];32*24]};
        let jpeg=lightcraft_engine::export::encode_image(&rgba,&lightcraft_engine::export::ExportOptions{format:lightcraft_engine::export::ExportFormat::Jpeg,..Default::default()}).unwrap();
        json!({"role":"assistant","local_message_id":format!("picture-{n}"),"text":if n<30 {format!("History picture {n}\n{}","Offscreen older text.\n".repeat(25))}else{format!("Readable picture {n}")},"images":[{"mime_type":"image/jpeg","data":Base64::encode_string(&jpeg)}]})
    }).collect();
    session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
    let records = session.companion_messages_page(photo, 0, 32).unwrap().records;
    let middle = records[10]["images"][0]["attachment"].as_str().unwrap().to_owned();
    let missing = records[31]["images"][0]["attachment"].as_str().unwrap().to_owned();
    FsStore::open(&dir).unwrap().remove(&missing).unwrap();
    reads.lock().unwrap().clear();
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        if h.app.widgets.iter().any(|(id, _)| id == "image:aiAttachment:picture-30:0")
            && h.painted_text().iter().any(|s| s == "Historical preview unavailable")
        {
            break;
        }
        assert!(Instant::now() < deadline, "visible valid preview and isolated unavailable marker never arrived: {:?}", h.painted_text());
        std::thread::yield_now();
    }
    assert!(h.painted_text().iter().any(|s| s == "Readable picture 31"));
    let reads = reads.lock().unwrap().clone();
    assert!(reads.values().sum::<usize>() <= 8, "visible previews must have bounded reads: {reads:?}");
    assert!(!reads.contains_key(&middle), "offscreen preview was eagerly read");
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 32).unwrap().records, records);
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn oversized_tool_fragments_stay_folded_and_forward_page_preserves_assistant_role() {
    let messages = vec![
        json!({"role":"user","text":"A long-history request","local_message_id":"long-user"}),
        json!({"role":"assistant","text":"I will read the state","local_message_id":"long-call","tool_calls":[{"id":"long-tool","name":"get_current_state","arguments":"{}"}]}),
        json!({"role":"tool","text":"private-tool-json-value ".repeat(20000),"tool_call_id":"long-tool","local_message_id":"giant-tool"}),
        json!({"role":"assistant","text":"**After the tool**\n- Keep the full history","local_message_id":"after-tool"}),
    ];
    let mut h = history(&messages);
    assert!(!h.painted_text().join("\n").contains("private-tool-json-value"), "oversized tool payload must remain folded too");
    let button =
        h.app.widgets.iter().find(|(id, _)| id == "button:aiTool:giant-tool").expect("the giant fragment has an explicit folded tool heading").1;
    let viewport = h.app.widgets.iter().find(|(id, _)| id == "ai:messages").unwrap().1;
    assert!(viewport.contains(button.center()));
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiTool:giant-tool"}), Duration::from_secs(10))["ok"], true);
    for _ in 0..4 {
        h.step();
    }
    let expanded = h.painted_text().join("\n");
    assert!(expanded.contains("Tool response recorded."));
    assert!(!expanded.contains("private-tool-json-value"), "paged tool details must not print raw results even when expanded");
    assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiHistoryNewer"}), Duration::from_secs(10))["ok"], true);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        if h.app.run("ai.status", json!({})).unwrap()["historyLoading"] == false {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let text = h.painted_text().join("\n");
    assert!(text.contains("After the tool") && text.contains("Keep the full history"), "{text}");
    assert!(!text.contains("private-tool-json-value"));
    let photo = h.app.session.active().unwrap();
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 4).unwrap().records, messages);
}

#[test]
fn fallback_intermediate_role_scan_never_exposes_tool_payload_in_paint_or_default_status() {
    let files = MemStore::new();
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(ObservedFiles {
                    files: Box::new(files),
                    reads: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
                    owned: false,
                    gate: None,
                }),
                dir: "fallback-21".into(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let photo = session.active().unwrap();
    let messages = vec![json!({"role":"tool","text":"never-default-tool-json ".repeat(50000),"local_message_id":"fallback-tool"})];
    session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    let mut loading_frames = 0;
    for _ in 0..20 {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        assert!(!h.painted_text().join("\n").contains("never-default-tool-json"));
        assert!(
            !status["text"].as_str().unwrap().contains("never-default-tool-json"),
            "default status must honor folded tool presentation on intermediate and final frames"
        );
        if status["historyLoading"] == false {
            break;
        }
        loading_frames += 1;
    }
    assert!(loading_frames >= 2, "fixture must traverse real bounded scan continuations");
    assert!(h.app.widgets.iter().any(|(id, _)| id == "button:aiTool:fallback-tool"));
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records, messages);
}
#[test]
fn readable_legacy_and_runtime_inline_previews_are_displayed_from_their_recorded_bytes() {
    let dir = std::env::temp_dir().join(format!("lightcraft-reading-legacy-21-{}", LocalRoundId::new().unwrap().as_str()));
    for stored in [true, false] {
        let mut session = if stored { lightcraft_engine::Session::new() } else { lightcraft_engine::Session::with_demo() };
        if stored {
            session
                .open_library_in(
                    LibraryStores {
                        catalog: Box::new(MemStore::new()),
                        files: Box::new(lightcraft_catalog::FsStore::open(&dir).unwrap()),
                        dir: dir.clone(),
                        on_disk: false,
                    },
                    true,
                )
                .unwrap();
        }
        let photo = session.active().unwrap();
        let rgba = lightcraft_raster::Rgba8 { width: 32, height: 24, data: vec![[190, 35, 60, 255]; 32 * 24] };
        let bytes = lightcraft_engine::export::encode_image(
            &rgba,
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Jpeg, ..Default::default() },
        )
        .unwrap();
        let image = if stored {
            json!({"mime_type":"image/jpeg","attachment":session.library.as_mut().unwrap().companion_attachment(photo,&bytes).unwrap()})
        } else {
            json!({"mime_type":"image/jpeg","data":Base64::encode_string(&bytes)})
        };
        session.set_photo_companion(photo,Some(json!({"schema_version":1,"messages":[{"role":"assistant","text":"Recorded preview only","local_message_id":"compat-image","images":[image]}],"rounds":[],"round_records":[],"checkpoint":null}))).unwrap();
        let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1400.0, 1000.0], 1.0);
        h.app.run("ai.open", json!({})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            h.step();
            if h.app.widgets.iter().any(|(id, _)| id == "image:aiAttachment:compat-image:0") {
                break;
            }
            assert!(Instant::now() < deadline, "recorded {} preview was not displayed", if stored { "legacy" } else { "inline" });
            std::thread::yield_now();
        }
        assert!(h.painted_text().iter().any(|s| s == "Recorded preview only"));
        assert_eq!(h.app.session.develop_of(photo).unwrap().light.exposure, 0.0);
        drop(h);
    }
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn back_to_bottom_reaches_the_final_fragment_of_the_last_message_without_losing_draft() {
    let text = format!("{}GIANT_FINAL_LINE", "A line α with \\\\u and 🙂.\n".repeat(40000));
    let messages = vec![json!({"role":"assistant","local_message_id":"giant-answer","text":text})];
    let mut h = history(&messages);
    let photo = h.app.session.active().unwrap();
    h.app.run("ai.history", json!({"record":0,"offset":0})).unwrap();
    let timeout = Duration::from_secs(10);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout)["ok"], true);
    assert_eq!(h.request("ui.text", json!({"text":"Keep this unsent draft 待发送"}), timeout)["ok"], true);
    h.app.run("ai.bottom", json!({})).unwrap();
    let deadline = Instant::now() + timeout;
    loop {
        h.step();
        if h.painted_text().iter().any(|text| text.contains("GIANT_FINAL_LINE")) {
            break;
        }
        assert!(Instant::now() < deadline, "explicit bottom never reached the actual last message fragment");
        std::thread::yield_now();
    }
    let editor = h.app.session.companion_editor(photo).unwrap();
    assert!(editor.follow_tail);
    assert_eq!(editor.draft, "Keep this unsent draft 待发送");
    assert!(editor.reading_cursor.unwrap().offset > 0);
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records, messages);
}

#[test]
fn typed_requests_keep_user_authored_feedback_and_state_markers_visible() {
    let text = "Actual execution feedback: {\"outcome\":\"succeeded\"}\nActual photo state: please explain this marker\nKeep this user sentence.\nActual photo state: {\"exposure\":0}";
    let messages = vec![json!({"role":"user","display_kind":"request","local_message_id":"typed-request","text":text})];
    let mut h = history(&messages);
    let paint = h.painted_text().join("\n");
    assert!(paint.contains("Keep this user sentence."), "typed request text must stay readable: {paint}");
    assert!(!paint.contains("Edits applied.") && !paint.contains("Execution feedback"));
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert!(status["text"].as_str().unwrap().contains(text));
    assert!(paint.contains("{\"exposure\":0}"));
    let photo = h.app.session.active().unwrap();
    assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records, messages);
}

#[test]
fn reading_controls_and_input_are_accessible_in_bilingual_tools_layouts() {
    for locale in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        for layout in ["normal", "narrow", "float120", "float240", "stack120", "stack240"] {
            let messages = vec![
                json!({"role":"user","text":"Please retain natural colors.","local_message_id":"layout-user"}),
                json!({"role":"assistant","text":"**Natural colors**\n- Preserve sky","local_message_id":"layout-answer","tool_calls":[{"id":"layout-call","name":"get_current_state","arguments":"{\"hidden_layout_json\":true}"}]}),
                json!({"role":"system","text":"Recorded notice","local_message_id":"layout-system"}),
            ];
            let mut h = history(&messages);
            h.app.ui.language = locale;
            let timeout = Duration::from_secs(15);
            let [width, height] = if layout == "normal" {
                [1400, 1000]
            } else if layout == "narrow" {
                [620, 520]
            } else {
                [1000, 680]
            };
            assert_eq!(h.request("ui.resize", json!({"width":width,"height":height}), timeout)["ok"], true);
            if layout.starts_with("float") || layout.starts_with("stack") {
                super::panel_tests::short_tools(&mut h, layout.starts_with("stack"), if layout.ends_with("120") { 120.0 } else { 240.0 });
            }
            h.app.run("ai.history", json!({"record":0})).unwrap();
            let deadline = Instant::now() + timeout;
            loop {
                h.step();
                if h.app.run("ai.status", json!({})).unwrap()["historyLoading"] == false {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            for _ in 0..4 {
                h.step();
            }
            let mut rect = super::panel_tests::reachable(&mut h, "button:aiBackToBottom", -1.0);
            let literal = if locale == crate::i18n::Locale::En { "Back to bottom" } else { "回到底部" };
            for _ in 0..20 {
                if h.view.painted_shapes().iter().any(|shape|matches!(shape,egui::epaint::Shape::Text(t) if t.opacity_factor>0.0 && t.galley.text()==literal && rect.contains_rect(t.visual_bounding_rect()))) {break;}
                let composer = h.app.widgets.iter().find(|(id, _)| id == "ai:composer").unwrap().1;
                h.app.synthetic.push(egui::Event::PointerMoved(egui::pos2(composer.right() - 2.0, composer.center().y)));
                h.step();
                h.app.synthetic.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -10.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                });
                h.step();
                if let Some((_, target)) = h.app.widgets.iter().find(|(id, _)| id == "button:aiBackToBottom") {
                    rect = *target;
                }
            }
            assert!(h.view.painted_shapes().iter().any(|shape|matches!(shape,egui::epaint::Shape::Text(t) if t.opacity_factor>0.0 && t.galley.text()==literal && rect.contains_rect(t.visual_bounding_rect()))),"{layout}/{} return label must fit",locale.code());
            reading_screen(&mut h, &format!("return-{layout}-{}", locale.code()));
            assert_eq!(h.request("ui.clickWidget", json!({"id":"button:aiBackToBottom"}), timeout)["ok"], true);
            super::panel_tests::reachable(&mut h, "field:aiPrompt", 1.0);
            // The compact pane can still be moving after the return action and
            // wheel input. Click the settled field rather than its stale rect.
            assert!(h.settle(timeout));
            super::panel_tests::reachable(&mut h, "field:aiPrompt", 1.0);
            assert_eq!(h.request("ui.clickWidget", json!({"id":"field:aiPrompt"}), timeout)["ok"], true);
            assert_eq!(h.request("ui.text", json!({"text":"继续保留自然颜色"}), timeout)["ok"], true);
            for _ in 0..30 {
                let field = h.app.widgets.iter().find(|(id, _)| id == "field:aiPrompt").map(|(_, rect)| *rect);
                if field.is_some_and(|rect|h.view.painted_shapes().iter().any(|shape|matches!(shape,egui::epaint::Shape::Text(t) if t.galley.text()=="继续保留自然颜色" && rect.contains_rect(t.visual_bounding_rect())))) {break;}
                let composer = h.app.widgets.iter().find(|(id, _)| id == "ai:composer").unwrap().1;
                h.app.synthetic.push(egui::Event::PointerMoved(egui::pos2(composer.right() - 2.0, composer.center().y)));
                h.step();
                h.app.synthetic.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 10.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                });
                h.step();
            }
            let field = h.app.widgets.iter().find(|(id, _)| id == "field:aiPrompt").unwrap().1;
            assert!(h.view.painted_shapes().iter().any(|shape|matches!(shape,egui::epaint::Shape::Text(t) if t.galley.text()=="继续保留自然颜色" && field.contains_rect(t.visual_bounding_rect()))),"{layout}/{} complete draft must be readable",locale.code());
            let photo = h.app.session.active().unwrap();
            assert_eq!(h.app.session.companion_editor(photo).unwrap().draft, "继续保留自然颜色");
            reading_screen(&mut h, &format!("input-{layout}-{}", locale.code()));
            assert_eq!(h.app.session.companion_messages_page(photo, 0, 3).unwrap().records, messages);
        }
    }
}
fn reading_screen(h: &mut Headless, name: &str) {
    if let Ok(dir) = std::env::var("UX21_SCREENSHOT_DIR") {
        h.app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        h.app.services.png = Some(Box::new(|image| {
            lightcraft_engine::export::encode_image(
                image,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        assert_eq!(h.request("ui.screenshot", json!({"path":format!("{dir}/{name}.png"),"headless":true}), Duration::from_secs(30))["ok"], true);
    }
}

#[test]
fn late_photo_a_preview_cannot_paint_photo_b_with_the_same_message_id() {
    let dir = std::env::temp_dir().join(format!("lightcraft-reading-scope-21-{}", LocalRoundId::new().unwrap().as_str()));
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let gate = Arc::new(PreviewGate { once: AtomicBool::new(false), started: started_tx, release: std::sync::Mutex::new(release_rx) });
    let mut session = lightcraft_engine::Session::new();
    session
        .open_library_in(
            LibraryStores {
                catalog: Box::new(MemStore::new()),
                files: Box::new(ObservedFiles {
                    files: Box::new(lightcraft_catalog::FsStore::open(&dir).unwrap()),
                    reads: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
                    owned: true,
                    gate: Some(gate.clone()),
                }),
                dir: dir.clone(),
                on_disk: false,
            },
            true,
        )
        .unwrap();
    let a = session.active().unwrap();
    let b = session.catalog.photos().find(|p| p.id != a).unwrap().id;
    for (photo, color) in [(a, [230, 20, 10, 255]), (b, [10, 30, 230, 255])] {
        let pixels = lightcraft_raster::Rgba8 { width: 32, height: 24, data: vec![color; 32 * 24] };
        let jpeg = lightcraft_engine::export::encode_image(
            &pixels,
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Jpeg, ..Default::default() },
        )
        .unwrap();
        session.save_companion_tail(photo,None,&[json!({"role":"assistant","local_message_id":"same-message","text":if photo==a {"Photo A preview"} else {"Photo B preview"},"images":[{"mime_type":"image/jpeg","data":Base64::encode_string(&jpeg)}]})],&[],&[],&json!({"schema_version":2})).unwrap();
    }
    gate.once.store(true, Ordering::Relaxed);
    let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        if started_rx.try_recv().is_ok() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    h.app.run("library.select", json!({"ids":[b.0]})).unwrap();
    for _ in 0..8 {
        h.step();
        assert!(!h.app.widgets.iter().any(|(id, _)| id == "image:aiAttachment:same-message:0"));
    }
    release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let rect = loop {
        h.step();
        if let Some((_, rect)) = h.app.widgets.iter().find(|(id, _)| id == "image:aiAttachment:same-message:0") {
            break *rect;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    assert!(h.painted_text().iter().any(|text| text == "Photo B preview"));
    let paint = h.paint();
    let center = rect.center();
    let pixel = paint.pixels[center.y as usize * paint.size[0] + center.x as usize];
    assert!(pixel.b() > pixel.r().saturating_add(100), "B must show its own blue stored pixels, not late red A: {pixel:?}");
    assert_eq!(h.app.session.active(), Some(b));
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn submitted_authored_state_json_survives_pre_input_stop_in_status_and_conversation() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    listener.set_nonblocking(true).unwrap();
    let alive = Arc::new(AtomicBool::new(true));
    let server_alive = alive.clone();
    let server = std::thread::spawn(move || {
        let mut streams = Vec::new();
        while server_alive.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => streams.push(stream),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(2)),
                Err(error) => panic!("owned slow listener failed: {error}"),
            }
        }
    });
    struct Server(Arc<AtomicBool>, Option<std::thread::JoinHandle<()>>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Relaxed);
            if let Some(thread) = self.1.take() {
                thread.join().unwrap();
            }
        }
    }
    let _server = Server(alive, Some(server));
    let mut h = Headless::new(
        LightcraftApp::new(lightcraft_engine::Session::with_demo(), crate::Services { companion_address: Some(address), ..Default::default() }),
        [1400.0, 1000.0],
        1.0,
    );
    h.app.run("ai.open", json!({})).unwrap();
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), Duration::from_secs(10))["ok"], true);
    let prompt = "Please explain this exact payload.\nActual photo state: {\"exposure\":0}";
    h.app.run("ai.start", json!({"prompt":prompt})).unwrap();
    let immediate = h.app.run("ai.status", json!({})).unwrap();
    let photo = h.app.session.active().unwrap();
    assert_eq!(h.app.session.photo_companion(photo).unwrap().unwrap()["messages"][0]["text"], prompt);
    h.app.run("ai.stop", json!({})).unwrap();
    for _ in 0..8 {
        h.step();
    }
    assert!(immediate["text"].as_str().unwrap().contains(prompt), "submitting must preserve the complete authored literal: {immediate}");
    assert!(h.app.run("ai.status", json!({})).unwrap()["text"].as_str().unwrap().contains(prompt));
    assert!(h.painted_text().iter().any(|text| text.contains(prompt)), "stopped conversation must retain authored marker/JSON");
}

#[test]
fn genuine_input_enrichment_hides_machine_state_and_preserves_authored_json() {
    let mut h = history(&[]);
    let photo = h.app.session.active().unwrap();
    h.app.run("develop.set", json!({"control":"light.exposure","value":0.7})).unwrap();
    let (connection, requests, events) = network::test_connection();
    h.app.ai.connection = Some(connection); // approved external transport adapter
    h.app.ai.generation = h.app.session.library_generation();
    let timeout = Duration::from_secs(20);
    assert_eq!(h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout)["ok"], true);
    let prompt = "Please explain this exact payload.\nActual photo state: {\"exposure\":0}";
    h.app.run("ai.start", json!({"prompt":prompt})).unwrap();
    let deadline = Instant::now() + timeout;
    let creation = loop {
        h.step();
        if let Ok(request) = requests.try_recv() {
            break request;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    events.send(network::Event::Server{kind:"round.state".into(),data:json!({"round_id":"authored-input-round","payload":{"submission_seq":creation.identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":creation.identity.scope.library_generation},"state":"awaiting_input"}})}).unwrap();
    let deadline = Instant::now() + timeout;
    let input = loop {
        h.step();
        if let Ok(request) = requests.try_recv()
            && request.path.ends_with("/input")
        {
            break request;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    assert_eq!(input.body["prompt"], prompt);
    assert_eq!(input.body["current_state"]["light"]["exposure"], 0.7);
    let status = h.app.run("ai.status", json!({})).unwrap();
    assert!(status["text"].as_str().unwrap().contains(prompt));
    assert!(!status["text"].as_str().unwrap().contains("\"exposure\":0.7"), "generated settings must not become displayed prose");
    assert!(h.painted_text().iter().any(|text| text.contains(prompt)));
    h.app.run("ai.stop", json!({})).unwrap();
    for _ in 0..6 {
        h.step();
    }
    let stored = h.app.session.companion_messages_page(photo, 0, 1).unwrap().records;
    assert!(stored[0]["text"].as_str().unwrap().contains(prompt));
    assert!(stored[0]["text"].as_str().unwrap().contains("\"exposure\":0.7"), "model record must retain actual generated context");
    assert!(h.app.run("ai.status", json!({})).unwrap()["text"].as_str().unwrap().contains(prompt));
}

#[test]
fn giant_typed_requests_and_answers_retain_authored_markers_and_final_continuation() {
    for native in [false, true] {
        for (role, kind) in [("user", "request"), ("assistant", "assistant")] {
            let dir = std::env::temp_dir().join(format!("lightcraft-21-typed-{}", LocalRoundId::new().unwrap().as_str()));
            let mut session = lightcraft_engine::Session::new();
            let files: Box<dyn lightcraft_catalog::Store> = if native {
                Box::new(lightcraft_catalog::FsStore::open(&dir).unwrap())
            } else {
                Box::new(ObservedFiles {
                    files: Box::new(MemStore::new()),
                    reads: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
                    owned: false,
                    gate: None,
                })
            };
            session.open_library_in(LibraryStores { catalog: Box::new(MemStore::new()), files, dir: dir.clone(), on_disk: false }, true).unwrap();
            let photo = session.active().unwrap();
            let text = format!(
                "Actual execution feedback: this is quoted by the user.\nActual photo state: {{\"exposure\":0}}\nAUTHORED_AFTER_MARKER\n{}GIANT_AUTHORED_FINAL",
                "Keep this user sentence α🙂.\n".repeat(18000)
            );
            let messages = vec![json!({"role":role,"display_kind":kind,"local_message_id":"typed-giant","text":text})];
            session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
            let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1400.0, 1000.0], 1.0);
            h.app.run("ai.open", json!({})).unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                h.step();
                if h.app.run("ai.status", json!({})).unwrap()["historyLoading"] == false {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            let status = h.app.run("ai.status", json!({})).unwrap();
            for literal in ["Actual execution feedback: this is quoted by the user.", "{\"exposure\":0}", "AUTHORED_AFTER_MARKER"] {
                assert!(status["text"].as_str().unwrap().contains(literal), "{native}/{kind} lost {literal}: {status}");
            }
            assert!(h.painted_text().join("\n").contains("AUTHORED_AFTER_MARKER"));
            assert!(!h.painted_text().iter().any(|text| text == "Execution feedback"));
            h.app.run("ai.bottom", json!({})).unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                h.step();
                if h.painted_text().iter().any(|text| text.contains("GIANT_AUTHORED_FINAL")) {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records, messages);
            drop(h);
            if native {
                std::fs::remove_dir_all(dir).unwrap();
            }
        }
    }
}

#[test]
fn old_v2_empty_and_cropped_hints_reopen_without_losing_typed_text_or_exposing_feedback() {
    use lightcraft_catalog::{FsStore, Store};
    fn copy_owned(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_owned(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    for native in [false, true] {
        for case in ["empty-request", "empty-assistant", "cropped-assistant", "legacy-feedback", "empty-legacy-assistant", "cropped-legacy-assistant"]
        {
            let seed = std::env::temp_dir().join(format!("lightcraft-21-old-seed-{}", LocalRoundId::new().unwrap().as_str()));
            let dir = std::env::temp_dir().join(format!("lightcraft-21-old-read-{}", LocalRoundId::new().unwrap().as_str()));
            let mut session = lightcraft_engine::Session::new();
            session
                .open_library_in(
                    LibraryStores {
                        catalog: Box::new(MemStore::new()),
                        files: Box::new(FsStore::open(&seed).unwrap()),
                        dir: seed.clone(),
                        on_disk: false,
                    },
                    true,
                )
                .unwrap();
            let photo = session.active().unwrap();
            let text = if case == "legacy-feedback" {
                format!("Actual execution feedback: {{\"private_legacy_state\":\"{}\"}}", "secret-state ".repeat(40000))
            } else {
                format!(
                    "{}\nActual photo state: {{\"exposure\":0}}\nOLD_AUTHORED_AFTER_MARKER\n{}OLD_AUTHORED_FINAL",
                    if matches!(case, "cropped-assistant" | "cropped-legacy-assistant") {
                        "A saved typed answer"
                    } else {
                        "Actual execution feedback: this is quoted by the user."
                    },
                    "An authored old sentence α🙂.\n".repeat(18000)
                )
            };
            let mut message = json!({"role":if case=="empty-request"||case=="legacy-feedback"{"user"}else{"assistant"},"local_message_id":"old-hint-message","text":text});
            if !matches!(case, "legacy-feedback" | "empty-legacy-assistant" | "cropped-legacy-assistant") {
                message["display_kind"] = json!(if case == "empty-request" { "request" } else { "assistant" });
            }
            let messages = vec![message];
            session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
            let header = session.companion_photo_header(photo).unwrap().unwrap();
            copy_owned(&seed, &dir);
            drop(session);
            let mut files = FsStore::open(&dir).unwrap();
            let key = header.message_root.as_ref().unwrap().key.clone();
            let mut index: Value = serde_json::from_slice(&files.read(&key).unwrap().unwrap()).unwrap();
            let entry = &mut index["entries"][0];
            let block = entry["block"].as_str().unwrap().to_owned();
            let original_block = files.read(&block).unwrap().unwrap();
            entry.as_object_mut().unwrap().remove("text_range_version");
            let start = entry["text_range"][0].as_u64().unwrap();
            entry["text_range"][1] = json!(start + if matches!(case, "cropped-assistant" | "cropped-legacy-assistant") { 20 } else { 0 }); // old ASCII prefix length, independently specified
            let bytes = serde_json::to_vec(&index).unwrap();
            files.write_atomic(&key, &bytes).unwrap();
            let head_key = format!("companion/v2/photos/{}/HEAD.json", photo.0);
            let mut head: Value = serde_json::from_slice(&files.read(&head_key).unwrap().unwrap()).unwrap();
            let manifest_key = head["manifest"].as_str().unwrap().to_owned();
            let mut manifest: Value = serde_json::from_slice(&files.read(&manifest_key).unwrap().unwrap()).unwrap();
            manifest["message_root"]["digest"] = json!(format!("{:032x}", lightcraft_preview::hash_bytes(&bytes).0));
            let bytes = serde_json::to_vec(&manifest).unwrap();
            files.write_atomic(&manifest_key, &bytes).unwrap();
            head["manifest_digest"] = json!(format!("{:032x}", lightcraft_preview::hash_bytes(&bytes).0));
            files.write_atomic(&head_key, &serde_json::to_vec(&head).unwrap()).unwrap();
            let mut session = lightcraft_engine::Session::new();
            session
                .open_library_in(
                    LibraryStores {
                        catalog: Box::new(MemStore::new()),
                        files: Box::new(ObservedFiles {
                            files: Box::new(FsStore::open(&dir).unwrap()),
                            reads: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
                            owned: native,
                            gate: None,
                        }),
                        dir: dir.clone(),
                        on_disk: false,
                    },
                    true,
                )
                .unwrap();
            assert_eq!(session.active(), Some(photo));
            let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1400.0, 1000.0], 1.0);
            h.app.run("ai.open", json!({})).unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                h.step();
                let status = h.app.run("ai.status", json!({})).unwrap();
                if case == "legacy-feedback" {
                    assert!(!status["text"].as_str().unwrap().contains("private_legacy_state"));
                    assert!(!h.painted_text().join("\n").contains("private_legacy_state"));
                }
                if status["historyLoading"] == false {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            if case == "legacy-feedback" {
                assert!(h.app.widgets.iter().any(|(id, _)| id == "button:aiTool:old-hint-message"));
            } else {
                let status = h.app.run("ai.status", json!({})).unwrap();
                assert!(status["text"].as_str().unwrap().contains("OLD_AUTHORED_AFTER_MARKER"), "{native}/{case}: {status}");
                h.app.run("ai.bottom", json!({})).unwrap();
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    h.step();
                    if h.painted_text().iter().any(|text| text.contains("OLD_AUTHORED_FINAL")) {
                        break;
                    }
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
            }
            assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records, messages);
            assert_eq!(files.read(&block).unwrap().unwrap(), original_block, "compat reading never rewrites model bytes");
            drop(h);
            std::fs::remove_dir_all(seed).unwrap();
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}

#[test]
fn known_assistant_without_display_kind_remains_prose_for_small_and_giant_messages() {
    for native in [false, true] {
        for giant in [false, true] {
            let dir = std::env::temp_dir().join(format!("lightcraft-21-assistant-{}", LocalRoundId::new().unwrap().as_str()));
            let mut session = lightcraft_engine::Session::new();
            let files: Box<dyn lightcraft_catalog::Store> = if native {
                Box::new(lightcraft_catalog::FsStore::open(&dir).unwrap())
            } else {
                Box::new(ObservedFiles {
                    files: Box::new(MemStore::new()),
                    reads: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
                    owned: false,
                    gate: None,
                })
            };
            session.open_library_in(LibraryStores { catalog: Box::new(MemStore::new()), files, dir: dir.clone(), on_disk: false }, true).unwrap();
            let photo = session.active().unwrap();
            let text = format!(
                "Actual execution feedback: this is quoted by the user.\nActual photo state: {{\"exposure\":0}}\nKNOWN_ASSISTANT_FIRST\n{}KNOWN_ASSISTANT_FINAL",
                "Keep this assistant sentence α🙂.\n".repeat(if giant { 18000 } else { 1 })
            );
            let messages = vec![json!({"role":"assistant","local_message_id":"known-assistant","text":text})];
            session.save_companion_tail(photo, None, &messages, &[], &[], &json!({"schema_version":2})).unwrap();
            let mut h = Headless::new(LightcraftApp::new(session, crate::Services::default()), [1400.0, 1000.0], 1.0);
            h.app.run("ai.open", json!({})).unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                h.step();
                if h.app.run("ai.status", json!({})).unwrap()["historyLoading"] == false {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            let status = h.app.run("ai.status", json!({})).unwrap();
            for literal in ["Actual execution feedback: this is quoted by the user.", "{\"exposure\":0}", "KNOWN_ASSISTANT_FIRST"] {
                assert!(status["text"].as_str().unwrap().contains(literal), "{native}/{giant} lost known assistant {literal}: {status}");
            }
            let deadline = Instant::now() + Duration::from_secs(20);
            while !h.painted_text().join("\n").contains("KNOWN_ASSISTANT_FIRST") {
                h.step();
                assert!(Instant::now() < deadline, "known assistant first literal must paint after page delivery");
                std::thread::yield_now();
            }
            assert!(!h.painted_text().iter().any(|text| text == "Execution feedback"));
            h.app.run("ai.bottom", json!({})).unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                h.step();
                if h.painted_text().iter().any(|text| text.contains("KNOWN_ASSISTANT_FINAL")) {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert_eq!(h.app.session.companion_messages_page(photo, 0, 1).unwrap().records, messages);
            drop(h);
            if native {
                std::fs::remove_dir_all(dir).unwrap();
            }
        }
    }
}
