//! A native, single-round Photo Companion prototype. Only the local backend knows the model.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use lightcraft_catalog::PhotoId;
use lightcraft_develop::DevelopSettings;
use lightcraft_engine::media::RenderJob;
use serde_json::{Value, json};

use crate::LightcraftApp;

#[derive(Default)]
pub struct State {
    open: bool,
    consent: bool,
    prompt: String,
    pub text: String,
    job: Option<Job>,
    last: Option<Checkpoint>,
    serial: u64,
    worker_alive: Option<Arc<AtomicBool>>,
}

struct Checkpoint {
    photo: PhotoId,
    generation: u64,
    original: Arc<DevelopSettings>,
    expected: Arc<DevelopSettings>,
    content: String,
}

struct Job {
    checkpoint: Checkpoint,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Message>,
    started: Instant,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

enum Message {
    Applied { settings: Arc<DevelopSettings>, text: String, ack: mpsc::Sender<bool> },
    Finished(Result<String, String>),
}

impl State {
    pub fn busy_photo(&self) -> Option<PhotoId> {
        self.job.as_ref().map(|j| j.checkpoint.photo)
    }

    fn worker_busy(&self) -> bool {
        self.worker_alive.as_ref().is_some_and(|alive| alive.load(Ordering::Relaxed))
    }
}

struct WorkerAlive(Arc<AtomicBool>);
impl Drop for WorkerAlive {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

pub fn panel(app: &mut LightcraftApp, ui: &mut egui::Ui, photo: PhotoId) {
    if !app.ai.open && !matches!(app.ui.right, crate::state::RightPanel::Edit) {
        return;
    }
    egui::Frame::NONE.inner_margin(egui::Margin::same(16)).show(ui, |ui| {
        let response = ui.button("AI 点评并优化");
        crate::widgets::register(ui.ctx(), "button:aiOpen", response.rect);
        if response.clicked() {
            let _ = app.run("ai.open", json!({}));
        }
        if !app.ai.open {
            return;
        }
        ui.label("理解照片并自然优化。本次仅调整曝光，保留其他编辑。");
        let input =
            ui.add_enabled(app.ai.job.is_none(), egui::TextEdit::multiline(&mut app.ai.prompt).desired_rows(2).hint_text("例如：让画面自然明亮一些"));
        crate::widgets::register(ui.ctx(), "field:aiPrompt", input.rect);
        if app.ai.job.is_none() {
            let response = ui.checkbox(&mut app.ai.consent, "同意将压缩预览发送给云端模型；原始照片留在本机");
            crate::widgets::register(ui.ctx(), "checkbox:aiConsent", response.rect);
            let start = ui.add_enabled(app.ai.consent && !app.ai.worker_busy() && !cfg!(target_arch = "wasm32"), egui::Button::new("点评并优化"));
            crate::widgets::register(ui.ctx(), "button:aiStart", start.rect);
            if start.clicked()
                && let Err(e) = app.run("ai.start", json!({}))
            {
                app.ai.text = e;
            }
            if app.ai.worker_busy() {
                ui.label("正在结束上一轮处理…");
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
            if app.ai.last.as_ref().is_some_and(|c| c.photo == photo) {
                let undo = ui.button("撤回本轮");
                crate::widgets::register(ui.ctx(), "button:aiUndo", undo.rect);
                if undo.clicked()
                    && let Err(e) = app.run("ai.undo", json!({}))
                {
                    app.ai.text = e;
                }
            }
        } else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("正在分析、渲染或复查…");
                let stop = ui.button("停止生成");
                crate::widgets::register(ui.ctx(), "button:aiStop", stop.rect);
                if stop.clicked() {
                    let _ = app.run("ai.stop", json!({}));
                }
            });
        }
        if !app.ai.text.is_empty() {
            ui.label(&app.ai.text);
        }
    });
}

pub fn command(app: &mut LightcraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    match id {
        "ai.open" => {
            app.ai.open = true;
            Some(Ok(Value::Null))
        }
        "ai.status" => Some(Ok(
            json!({"busy":app.ai.job.is_some(),"workerBusy":app.ai.worker_busy(),"photo":app.ai.busy_photo().map(|p|p.0),"text":app.ai.text}),
        )),
        "ai.start" => {
            if params.get("consent").and_then(Value::as_bool) == Some(true) {
                app.ai.consent = true;
            }
            if let Some(prompt) = params.get("prompt").and_then(Value::as_str) {
                app.ai.prompt = prompt.into();
            }
            Some(start(app))
        }
        "ai.stop" => {
            if let Some(job) = app.ai.job.take() {
                app.ai.last = Some(clone_checkpoint(&job.checkpoint));
            }
            app.ai.text.push_str("\n已停止生成，保留最近一次完整结果。");
            Some(Ok(Value::Null))
        }
        "ai.undo" => Some(undo(app)),
        _ => None,
    }
}

fn clone_checkpoint(c: &Checkpoint) -> Checkpoint {
    Checkpoint { photo: c.photo, generation: c.generation, original: c.original.clone(), expected: c.expected.clone(), content: c.content.clone() }
}

pub fn blocks_edit(app: &LightcraftApp, command: &str, params: &Value) -> bool {
    let Some(target) = app.ai.busy_photo() else {
        return false;
    };
    if matches!(command, "edit.undo" | "edit.redo" | "history.undo" | "history.redo") {
        return true;
    }
    let safe = command.starts_with("view.")
        || matches!(
            command,
            "library.select"
                | "library.next"
                | "library.previous"
                | "library.selectAll"
                | "library.selectNone"
                | "library.selectBy"
                | "library.sort"
                | "library.source"
                | "library.clearFilter"
                | "library.filter"
                | "library.info"
                | "photo.info"
                | "photo.inspect"
                | "develop.get"
                | "develop.controls"
                | "app.quit"
        );
    !safe && (app.session.active() == Some(target) || app.session.targets(params).contains(&target))
}

fn start(app: &mut LightcraftApp) -> Result<Value, String> {
    if cfg!(target_arch = "wasm32") {
        return Err("本次验证仅支持桌面版。".into());
    }
    if !app.ai.consent {
        return Err("请先同意发送压缩预览。".into());
    }
    if app.ai.job.is_some() {
        return Err("已有照片正在处理，请先停止或等待完成。".into());
    }
    if app.ai.worker_busy() {
        return Err("上一轮正在结束，请稍候。".into());
    }
    let photo = app.session.active().ok_or("请先选择一张照片。")?;
    // Finish a short slider gesture, never hold an interaction across network waits.
    app.session.end_interaction().map_err(|e| e.to_string())?;
    let original = app.session.develop_of(photo).ok_or("照片不存在。")?;
    let job = app.session.export_job(photo, 640, 640, lightcraft_pipeline::OutputSpace::Srgb, lightcraft_pipeline::OutputDepth::U8)?;
    let generation = app.session.library_generation();
    let content = app.session.catalog.photo(photo).map(|p| lightcraft_engine::media::content_key(p)).ok_or("照片不存在。")?;
    let prompt = if app.ai.prompt.trim().is_empty() { "请点评这张照片，并自然优化曝光。".to_string() } else { app.ai.prompt.clone() };
    if prompt.len() > 4096 {
        return Err("请求过长。".into());
    }
    app.ai.serial = app.ai.serial.saturating_add(1);
    let round = format!("{}-{}-{}", std::process::id(), photo.0, app.ai.serial);
    let address = std::env::var("PHOTO_COMPANION_ADDR").unwrap_or_else(|_| "127.0.0.1:18080".into());
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let (tx, rx) = mpsc::channel();
    let alive = Arc::new(AtomicBool::new(true));
    let worker_alive = WorkerAlive(alive.clone());
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::Builder::new()
        .name("photo-companion".into())
        .spawn(move || {
            let _alive = worker_alive;
            let result = roundtrip(job, &address, &round, &prompt, &worker_cancel, &tx);
            if result.is_err() {
                worker_cancel.store(true, Ordering::Relaxed);
            }
            let _ = tx.send(Message::Finished(result));
        })
        .map_err(|e| format!("无法启动 AI：{e}"))?;
    #[cfg(target_arch = "wasm32")]
    let _ = (job, address, round, prompt, worker_cancel, tx, worker_alive);
    app.ai.job = Some(Job {
        checkpoint: Checkpoint { photo, generation, original: original.clone(), expected: original, content },
        cancel,
        rx,
        started: Instant::now(),
    });
    app.ai.worker_alive = Some(alive);
    app.ai.last = None;
    app.ai.text = "正在准备预览…".into();
    Ok(json!({"started":true,"photo":photo.0}))
}

fn current(app: &LightcraftApp, c: &Checkpoint) -> bool {
    app.session.library_generation() == c.generation
        && app.session.catalog.photo(c.photo).is_some_and(|p| !p.deleted && lightcraft_engine::media::content_key(p) == c.content)
        && app.session.develop_of(c.photo).is_some_and(|d| *d == *c.expected)
}

fn commit_one(app: &mut LightcraftApp, photo: PhotoId, settings: &DevelopSettings, label: &str) -> Result<(), String> {
    // A targeted op deliberately bypasses Auto Sync and the current selection.
    let op = app.session.develop_op(photo, settings.clone(), label).ok_or("照片不存在。")?;
    app.session.commit(label, op).map_err(|e| e.to_string())
}

fn undo(app: &mut LightcraftApp) -> Result<Value, String> {
    if app.ai.job.is_some() {
        return Err("请先停止生成。".into());
    }
    let c = app.ai.last.as_ref().ok_or("没有可撤回的修图轮次。")?;
    if !current(app, c) {
        return Err("照片在本轮之后已改变，请使用编辑历史恢复。".into());
    }
    let (photo, original) = (c.photo, c.original.clone());
    commit_one(app, photo, &original, "Restore AI round")?;
    app.ai.last = None;
    app.ai.text.push_str("\n本轮修改已撤回。");
    Ok(Value::Null)
}

pub fn poll(app: &mut LightcraftApp, ctx: &egui::Context) {
    let Some(mut job) = app.ai.job.take() else {
        return;
    };
    if job.started.elapsed() > Duration::from_secs(300) {
        app.ai.text.push_str("\n本轮超时，保留最近完整结果。");
        app.ai.last = Some(clone_checkpoint(&job.checkpoint));
        return;
    }
    loop {
        let message = match job.rx.try_recv() {
            Ok(message) => message,
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                app.ai.text.push_str("\nAI 处理已结束，保留最近完整结果。");
                app.ai.last = Some(clone_checkpoint(&job.checkpoint));
                return;
            }
        };
        match message {
            Message::Applied { settings, text, ack } => {
                if !current(app, &job.checkpoint) || job.cancel.load(Ordering::Relaxed) {
                    let _ = ack.send(false);
                    app.ai.text.push_str("\n照片或图库已改变，迟到结果未执行。");
                    app.ai.last = Some(clone_checkpoint(&job.checkpoint));
                    return;
                }
                if let Err(e) = commit_one(app, job.checkpoint.photo, &settings, "AI exposure") {
                    let _ = ack.send(false);
                    app.ai.text.push_str(&format!("\n{e}"));
                    return;
                }
                if ack.send(true).is_err() {
                    let _ = commit_one(app, job.checkpoint.photo, &job.checkpoint.expected, "Restore expired AI plan");
                    app.ai.text.push_str("\n本轮已结束，方案未保留。");
                    app.ai.last = Some(clone_checkpoint(&job.checkpoint));
                    return;
                }
                job.checkpoint.expected = settings;
                app.ai.text = if app.ai.text == "正在准备预览…" { text } else { format!("{}\n{}", app.ai.text, text) };
            }
            Message::Finished(result) => {
                match result {
                    Ok(text) => app.ai.text.push_str(&format!("\n{text}")),
                    Err(e) => app.ai.text.push_str(&format!("\n处理未完成：{e}")),
                }
                app.ai.last = Some(clone_checkpoint(&job.checkpoint));
                return;
            }
        }
    }
    app.ai.job = Some(job);
    ctx.request_repaint_after(Duration::from_millis(50));
}

fn render_preview(mut job: RenderJob, settings: Arc<DevelopSettings>) -> Result<Vec<u8>, String> {
    job.settings = settings;
    job.cache = None;
    job.view_cache = None;
    job.stages = None;
    let output = job.run().rendered?;
    let options = lightcraft_engine::export::ExportOptions {
        format: lightcraft_engine::export::ExportFormat::Jpeg,
        quality: 75,
        metadata: lightcraft_engine::export::MetadataPolicy::None,
        ..Default::default()
    };
    let bytes = lightcraft_engine::export::encode_image(&output.image, &options)?;
    if bytes.len() > 128 * 1024 {
        return Err("预览超过本次验证的大小限制。".into());
    }
    Ok(bytes)
}

fn check_cancel(cancel: &AtomicBool, start: Instant) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("已停止生成。".into());
    }
    if start.elapsed() > Duration::from_secs(300) {
        return Err("本轮超时。".into());
    }
    Ok(())
}

fn exposure_plan(response: &Value, round: &str, photo: PhotoId, stage: &str) -> Result<Option<f64>, String> {
    if response.get("round_id").and_then(Value::as_str) != Some(round)
        || response.get("photo_id").and_then(Value::as_str) != Some(&photo.0.to_string())
    {
        return Err("后端结果属于其他照片或轮次。".into());
    }
    let plan = response.get("plan").ok_or("后端缺少方案字段。")?;
    if plan.is_null() {
        if stage == "initial" {
            return Err("没有收到初次编辑方案。".into());
        }
        return Ok(None);
    }
    if plan.get("id").and_then(Value::as_str) != Some(&format!("{round}/{stage}")) {
        return Err("后端方案身份不匹配。".into());
    }
    let values = plan.get("values").and_then(Value::as_object).ok_or("方案参数无效。")?;
    let exposure =
        values.get("light.exposure").and_then(Value::as_f64).filter(|n| n.is_finite() && (-5.0..=5.0).contains(n)).ok_or("曝光参数无效。")?;
    if values.len() != 1 {
        return Err("方案包含未允许的编辑参数。".into());
    }
    Ok(Some(exposure))
}

fn roundtrip(job: RenderJob, address: &str, round: &str, prompt: &str, cancel: &AtomicBool, tx: &mpsc::Sender<Message>) -> Result<String, String> {
    let start = Instant::now();
    check_cancel(cancel, start)?;
    let photo = job.photo;
    let mut settings = job.settings.clone();
    let mut preview = render_preview(job.clone(), settings.clone())?;
    let mut history = vec![json!({"stage":"start","text":prompt,"exposure":settings.light.exposure,"preview":preview})];
    let mut execution = Value::Null;
    for stage in ["initial", "review"] {
        check_cancel(cancel, start)?;
        let request = json!({"round_id":round,"photo_id":photo.0.to_string(),"stage":stage,"prompt":prompt,"preview":preview,"settings":{"light.exposure":settings.light.exposure},"history":history,"execution":execution});
        let response = post(address, &request, cancel, start)?;
        check_cancel(cancel, start)?;
        let text = response.get("text").and_then(Value::as_str).ok_or("后端缺少点评说明。")?.to_string();
        let Some(exposure) = exposure_plan(&response, round, photo, stage)? else {
            return Ok(text);
        };
        let mut proposed = (*settings).clone();
        proposed.light.exposure = exposure;
        let proposed = Arc::new(proposed);
        // Render before committing: any render error keeps the last successful photo settings.
        let rendered = render_preview(job.clone(), proposed.clone())?;
        check_cancel(cancel, start)?;
        let (ack_tx, ack_rx) = mpsc::channel();
        tx.send(Message::Applied { settings: proposed.clone(), text: text.clone(), ack: ack_tx }).map_err(|_| "客户端已关闭。")?;
        loop {
            check_cancel(cancel, start)?;
            match ack_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(true) => break,
                Ok(false) | Err(mpsc::RecvTimeoutError::Disconnected) => return Err("编辑结果未被客户端接受。".into()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        settings = proposed;
        preview = rendered;
        history.push(json!({"stage":stage,"text":text,"exposure":settings.light.exposure,"preview":preview}));
        execution = json!({"plan_id":format!("{round}/{stage}"),"success":true});
    }
    let request = json!({"round_id":round,"photo_id":photo.0.to_string(),"stage":"complete","prompt":prompt,"preview":preview,"settings":{"light.exposure":settings.light.exposure},"history":history,"execution":execution});
    let response = post(address, &request, cancel, start)?;
    if exposure_plan(&response, round, photo, "complete")?.is_some() {
        return Err("本轮不允许再次修正。".into());
    }
    Ok(response.get("text").and_then(Value::as_str).unwrap_or("本轮完成。").to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn post(address: &str, input: &Value, cancel: &AtomicBool, start: Instant) -> Result<Value, String> {
    use std::io::{Read, Write};
    let address: std::net::SocketAddr = address.parse().map_err(|_| "本地 AI 服务地址无效。")?;
    if !address.ip().is_loopback() {
        return Err("此原型仅连接本机 AI 服务。".into());
    }
    let data = serde_json::to_vec(input).map_err(|e| e.to_string())?;
    if data.len() > 4 * 1024 * 1024 {
        return Err("本轮输入超过大小限制。".into());
    }
    let mut socket =
        std::net::TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|_| "无法连接本地 AI 服务，请先启动 Go 后端。")?;
    socket.set_read_timeout(Some(Duration::from_millis(500))).map_err(|e| e.to_string())?;
    socket.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    let header = format!(
        "POST /prototype/turn HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        data.len()
    );
    socket.write_all(header.as_bytes()).and_then(|_| socket.write_all(&data)).map_err(|e| e.to_string())?;
    let call_started = Instant::now();
    let mut response = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        check_cancel(cancel, start)?;
        if call_started.elapsed() > Duration::from_secs(125) {
            return Err("模型调用超时。".into());
        }
        match socket.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                response.extend_from_slice(buffer.get(..n).ok_or("读取失败。")?);
                if response.len() > 128 * 1024 {
                    return Err("后端回复过大。".into());
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(format!("AI 连接中断：{e}")),
        }
    }
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").ok_or("后端 HTTP 回复无效。")?;
    let headers = std::str::from_utf8(response.get(..split).ok_or("HTTP 头无效。")?).map_err(|_| "HTTP 头无效。")?;
    let body = response.get(split.saturating_add(4)..).ok_or("HTTP 正文无效。")?;
    if !headers.lines().next().is_some_and(|line| line.starts_with("HTTP/1.1 200 ")) {
        return Err(String::from_utf8_lossy(body).chars().take(400).collect());
    }
    if headers.to_ascii_lowercase().contains("transfer-encoding:") {
        return Err("后端返回了不支持的传输格式。".into());
    }
    serde_json::from_slice(body).map_err(|_| "后端 JSON 回复无效。".into())
}

#[cfg(target_arch = "wasm32")]
fn post(_: &str, _: &Value, _: &AtomicBool, _: Instant) -> Result<Value, String> {
    Err("仅支持桌面版。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightcraft_catalog::{Op, Source};

    fn app() -> LightcraftApp {
        LightcraftApp::new(lightcraft_engine::Session::with_demo(), crate::Services::default())
    }

    fn checkpoint(app: &LightcraftApp, photo: PhotoId) -> Checkpoint {
        let settings = app.session.develop_of(photo).unwrap();
        Checkpoint {
            photo,
            generation: app.session.library_generation(),
            original: settings.clone(),
            expected: settings,
            content: lightcraft_engine::media::content_key(app.session.catalog.photo(photo).unwrap()),
        }
    }

    fn pending(app: &mut LightcraftApp, photo: PhotoId, exposure: f64) -> (mpsc::Sender<Message>, mpsc::Receiver<bool>) {
        let original = app.session.develop_of(photo).unwrap();
        let mut settings = (*original).clone();
        settings.light.exposure = exposure;
        let (tx, rx) = mpsc::channel();
        let (ack, ack_rx) = mpsc::channel();
        app.ai.job = Some(Job { checkpoint: checkpoint(app, photo), cancel: Arc::new(AtomicBool::new(false)), rx, started: Instant::now() });
        tx.send(Message::Applied { settings: Arc::new(settings), text: "点评".into(), ack }).unwrap();
        (tx, ack_rx)
    }

    #[test]
    fn plans_reject_wrong_targets_and_extra_controls() {
        let base = json!({"round_id":"r","photo_id":"1","plan":{"id":"r/initial","values":{"light.exposure":0.7}}});
        assert_eq!(exposure_plan(&base, "r", PhotoId(1), "initial").unwrap(), Some(0.7));
        for value in [
            json!({"round_id":"other","photo_id":"1","plan":null}),
            json!({"round_id":"r","photo_id":"2","plan":null}),
            json!({"round_id":"r","photo_id":"1","plan":{"id":"r/initial","values":{"light.exposure":0.7,"light.contrast":1}}}),
            json!({"round_id":"r","photo_id":"1","plan":{"id":"r/review","values":{"light.exposure":6}}}),
        ] {
            assert!(exposure_plan(&value, "r", PhotoId(1), "initial").is_err());
        }
    }

    #[test]
    fn switching_photos_and_auto_sync_do_not_retarget_ai() {
        let mut app = app();
        let ids = app.session.visible_cloned();
        let target = ids[0];
        let other = ids[1];
        let before = app.session.develop_of(target).unwrap();
        let other_before = app.session.develop_of(other).unwrap();
        app.session.execute("library.select", &json!({"ids":[target.0,other.0],"active":other.0})).unwrap();
        app.session.auto_sync = true;
        let (_tx, ack) = pending(&mut app, target, 0.7);
        poll(&mut app, &egui::Context::default());
        assert!(ack.recv().unwrap());
        assert_eq!(app.session.develop_of(target).unwrap().light.exposure, 0.7);
        assert_eq!(*app.session.develop_of(other).unwrap(), *other_before);
        let job = app.ai.job.take().unwrap();
        app.ai.last = Some(clone_checkpoint(&job.checkpoint));
        drop(job);
        undo(&mut app).unwrap();
        assert_eq!(*app.session.develop_of(target).unwrap(), *before);
    }

    #[test]
    fn stopped_and_expired_pending_plans_never_apply() {
        for expired in [false, true] {
            let mut app = app();
            let photo = app.session.visible_cloned()[0];
            let original = app.session.develop_of(photo).unwrap();
            let (_tx, _ack) = pending(&mut app, photo, 0.7);
            if expired {
                app.ai.job.as_mut().unwrap().started = Instant::now() - Duration::from_secs(301)
            } else {
                command(&mut app, "ai.stop", &json!({})).unwrap().unwrap();
            }
            poll(&mut app, &egui::Context::default());
            assert_eq!(*app.session.develop_of(photo).unwrap(), *original);
            assert!(app.ai.job.is_none());
        }
    }

    #[test]
    fn changed_content_or_deleted_photo_rejects_old_result() {
        for deleted in [false, true] {
            let mut app = app();
            let photo = app.session.visible_cloned()[0];
            let original = app.session.develop_of(photo).unwrap();
            let (_tx, ack) = pending(&mut app, photo, 0.7);
            let op = if deleted {
                Op::SetDeleted { id: photo, deleted: true }
            } else {
                Op::SetFile { id: photo, file_name: "replaced.jpg".into(), source: Source::File { path: "replaced.jpg".into() } }
            };
            app.session.catalog.apply(op).unwrap();
            poll(&mut app, &egui::Context::default());
            assert!(!ack.recv().unwrap());
            assert_eq!(*app.session.develop_of(photo).unwrap(), *original);
            assert!(app.ai.job.is_none());
        }
    }

    #[test]
    fn expired_worker_ack_preserves_previous_complete_result() {
        let mut app = app();
        let photo = app.session.visible_cloned()[0];
        let mut first = (*app.session.develop_of(photo).unwrap()).clone();
        first.light.exposure = 0.7;
        commit_one(&mut app, photo, &first, "initial AI result").unwrap();
        let (_tx, ack) = pending(&mut app, photo, 0.4);
        drop(ack);
        poll(&mut app, &egui::Context::default());
        assert_eq!(app.session.develop_of(photo).unwrap().light.exposure, 0.7);
    }

    #[test]
    fn rendering_failure_does_not_change_photo() {
        let mut app = app();
        let photo = app.session.visible_cloned()[0];
        let original = app.session.develop_of(photo).unwrap();
        let mut job = app.session.export_job(photo, 64, 64, lightcraft_pipeline::OutputSpace::Srgb, lightcraft_pipeline::OutputDepth::U8).unwrap();
        job.source = app.session.media.origin_ref(&Source::File { path: "missing-photo-companion-fixture.jpg".into() }, 64);
        let (tx, _rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);
        assert!(roundtrip(job, "127.0.0.1:1", "r", "natural", &cancel, &tx).is_err());
        assert_eq!(*app.session.develop_of(photo).unwrap(), *original);
    }

    #[test]
    fn edits_pause_but_navigation_and_other_photos_remain_available() {
        let mut app = app();
        let ids = app.session.visible_cloned();
        let photo = ids[0];
        app.session.execute("library.select", &json!({"ids":[photo.0]})).unwrap();
        let (_tx, _ack) = pending(&mut app, photo, 0.7);
        for command in ["photo.rotateLeft", "curve.set", "geometry.set", "denoise.toggle", "preset.apply", "ui.tool"] {
            assert!(blocks_edit(&app, command, &json!({})), "{command}")
        }
        for command in ["library.next", "library.previous", "view.detail", "photo.inspect"] {
            assert!(!blocks_edit(&app, command, &json!({})), "{command}")
        }
        app.session.execute("library.select", &json!({"ids":[ids[1].0]})).unwrap();
        assert!(!blocks_edit(&app, "develop.set", &json!({})));
        assert!(blocks_edit(&app, "develop.set", &json!({"ids":[photo.0]})));
    }

    #[test]
    fn stopped_worker_blocks_another_start_until_exit() {
        let mut app = app();
        app.ai.consent = true;
        app.ai.worker_alive = Some(Arc::new(AtomicBool::new(true)));
        assert!(start(&mut app).is_err());
        assert!(app.ai.job.is_none());
    }
}
