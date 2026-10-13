//! Photo-bound conversations and the native Photo Companion round protocol.
#[cfg(test)]
#[path = "ai_companion/admission_tests.rs"]
mod admission_tests;
mod archive;
#[cfg(test)]
#[path = "ai_companion/disconnect_tests.rs"]
mod disconnect_tests;
#[cfg(test)]
#[path = "ai_companion/identity_tests.rs"]
mod identity_tests;
#[cfg(test)]
#[path = "ai_companion/library_tests.rs"]
mod library_tests;
mod network;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use crate::i18n::tr;
use base64ct::{Base64, Encoding};
use lightcraft_catalog::PhotoId;
use lightcraft_develop::DevelopSettings;
use lightcraft_engine::companion::{LeaseToken, LocalRoundId, PhotoScope, RoundIdentity};
use lightcraft_engine::media::{RenderJob, RenderResult};
use serde_json::{Value, json};

use crate::LightcraftApp;

pub struct State {
    open: bool,
    consent: bool,
    prompt: String,
    pub text: String,
    generation: u64,
    serial: u64,
    connection_generation: u64,
    connection: Option<network::Connection>,
    #[cfg(test)]
    next_connection: Option<network::Connection>,
    conversations: BTreeMap<PhotoId, Conversation>,
    jobs: BTreeMap<PhotoId, Job>,
    workers: BTreeMap<PhotoId, Arc<AtomicBool>>,
    tx: mpsc::SyncSender<WorkerMessage>,
    rx: mpsc::Receiver<WorkerMessage>,
}
impl Default for State {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(8);
        Self {
            open: false,
            consent: false,
            prompt: String::new(),
            text: String::new(),
            generation: 0,
            serial: 0,
            connection_generation: 0,
            connection: None,
            #[cfg(test)]
            next_connection: None,
            conversations: BTreeMap::new(),
            jobs: BTreeMap::new(),
            workers: BTreeMap::new(),
            tx,
            rx,
        }
    }
}
#[derive(Default)]
struct Conversation {
    scope: Option<PhotoScope>,
    messages: Vec<Value>,
    rounds: Vec<Value>,
    pending: BTreeMap<String, String>,
    notice: String,
    checkpoint: Option<Checkpoint>,
}
#[derive(Clone)]
struct Checkpoint {
    photo: PhotoId,
    generation: u64,
    original: Arc<DevelopSettings>,
    expected: Arc<DevelopSettings>,
    revision: u64,
    content: String,
    eligible: bool,
}
struct Job {
    lease: LeaseToken,
    identity: RoundIdentity,
    round: String,
    state: String,
    prompt: String,
    user_message: usize,
    history: Vec<Value>,
    checkpoint: Checkpoint,
    cancel: Arc<AtomicBool>,
    snapshot: Option<Arc<Snapshot>>,
    plans: BTreeMap<String, Value>,
    stopped: bool,
    inflight_plan: Option<String>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
struct Snapshot {
    result: RenderResult,
    preview: Value,
    context: lightcraft_engine::companion::CandidateContext,
}
enum WorkerMessage {
    Prepared { identity: RoundIdentity, plan: Option<Value>, settings: Arc<DevelopSettings>, result: Result<Snapshot, String> },
}
struct WorkerAlive(Arc<AtomicBool>);
impl Drop for WorkerAlive {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

impl State {
    pub fn is_busy(&self, photo: PhotoId) -> bool {
        self.jobs.get(&photo).is_some_and(|j| !j.stopped)
    }
    fn worker_busy(&self, photo: PhotoId) -> bool {
        self.workers.get(&photo).is_some_and(|a| a.load(Ordering::Relaxed))
    }
    fn send(&self, path: String, body: Value, submission: u64, round: String) -> Result<(), String> {
        let identity = self
            .jobs
            .values()
            .find(|j| (submission != 0 && j.identity.submission_seq == submission) || (!round.is_empty() && j.round == round))
            .map(|j| j.identity.clone())
            .ok_or("AI 请求归属已结束。")?;
        self.enqueue(path, body, submission, round, identity)
    }
    fn enqueue(&self, path: String, body: Value, submission: u64, round: String, identity: RoundIdentity) -> Result<(), String> {
        self.connection
            .as_ref()
            .ok_or("AI 事件连接尚未建立。")?
            .requests
            .try_send(network::Request { path, body, submission, round, identity })
            .map_err(|_| "AI 请求交付积压，请稍候；未自动重试。".into())
    }
    fn connect(&mut self) -> Result<network::Connection, String> {
        #[cfg(test)]
        if let Some(connection) = self.next_connection.take() {
            return Ok(connection);
        }
        network::connect(&std::env::var("PHOTO_COMPANION_ADDR").unwrap_or_else(|_| "127.0.0.1:18080".into()))
    }
    fn retire_disconnected_jobs(&mut self, app: &mut LightcraftApp) -> Result<(), String> {
        let photos: Vec<_> = self
            .jobs
            .iter()
            .filter_map(|(photo, job)| (job.identity.connection_generation == self.connection_generation).then_some(*photo))
            .collect();
        // The receiver may still contain Closed. Drop that connection only after
        // retaining its ownership list; no old route can outlive replacement.
        self.connection = None;
        for photo in &photos {
            self.finish(*photo, "AI 事件连接已断开，保留最近完整结果。");
        }
        // Cancel every old job even if one conversation cannot be saved. Never
        // establish a replacement connection when any of this saving fails.
        let mut failure = None;
        for photo in photos {
            if let Err(error) = self.save(app, photo) {
                failure.get_or_insert(error);
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    pub(crate) fn admission(&self, app: &LightcraftApp, explicit: Option<PhotoId>) -> Result<PhotoId, String> {
        if cfg!(target_arch = "wasm32") {
            return Err(tr("AI Companion is currently available on desktop only.").into());
        }
        if !self.consent {
            return Err(tr("Please consent to sending compressed previews first.").into());
        }
        if !self.jobs.is_empty() || self.workers.values().any(|a| a.load(Ordering::Relaxed)) || app.session.companion_task_photo().is_some() {
            return Err(tr("Another AI task is active; stop it or wait for completion.").into());
        }
        let photo = app.session.active().ok_or_else(|| tr("Select exactly one photo to start an AI task.").to_owned())?;
        if app.session.selection.ids.as_slice() != [photo] {
            return Err(tr("Select exactly one photo to start an AI task.").into());
        }
        if explicit.is_some_and(|id| id != photo) {
            return Err(tr("The requested photo must match the single selected photo.").into());
        }
        let p = app.session.catalog.photo(photo).ok_or_else(|| tr("This photo is unavailable for AI processing.").to_owned())?;
        if p.deleted {
            return Err(tr("This photo is unavailable for AI processing.").into());
        }
        if let lightcraft_catalog::Source::File { path } = &p.source {
            match app.session.media.availability.exists(path) {
                Some(true) => {}
                Some(false) => return Err(tr("This photo is unavailable for AI processing.").into()),
                None => return Err(tr("Checking photo availability. Please try again.").into()),
            }
        }
        Ok(photo)
    }
    #[cfg(test)]
    fn start(&mut self, app: &mut LightcraftApp) -> Result<Value, String> {
        self.start_for(app, None)
    }
    fn start_for(&mut self, app: &mut LightcraftApp, explicit: Option<PhotoId>) -> Result<Value, String> {
        if self.connection.as_ref().is_none_or(|c| !c.alive.load(Ordering::Relaxed)) {
            self.retire_disconnected_jobs(app)?;
        }
        let photo = self.admission(app, explicit)?;
        app.session.end_interaction().map_err(|e| e.to_string())?;
        let original = app.session.develop_of(photo).ok_or("照片不存在。")?;
        let content = app.session.catalog.photo(photo).map(|p| lightcraft_engine::media::content_key(p)).ok_or("照片不存在。")?;
        let prompt = if self.prompt.trim().is_empty() { "请点评这张照片，并自然优化。".into() } else { self.prompt.clone() };
        if prompt.len() > 4096 {
            return Err("请求过长。".into());
        }
        if self.connection.is_none() {
            self.connection_generation = self.connection_generation.checked_add(1).ok_or("AI 连接编号耗尽，请重新启动客户端。")?;
            self.connection = Some(self.connect()?);
        }
        self.serial = self.serial.checked_add(1).ok_or("AI 提交编号耗尽，请重新启动客户端。")?;
        let generation = app.session.library_generation();
        let revision = app.session.photo_revision(photo);
        let identity = RoundIdentity {
            scope: PhotoScope { library_generation: generation, photo },
            local_round_id: LocalRoundId::new()?,
            submission_seq: self.serial,
            connection_generation: self.connection_generation,
        };
        let lease = app.session.acquire_companion_lease(photo, identity.local_round_id.clone()).map_err(|e| tr(&e.to_string()).to_owned())?;
        let history = self.history(app, photo)?;
        self.enqueue(
            "/v1/rounds".into(),
            json!({"submission_seq":self.serial,"photo_ref":{"library_generation":generation,"photo_id":photo.0},"photo_revision":revision}),
            self.serial,
            String::new(),
            identity.clone(),
        )?;
        self.jobs.insert(
            photo,
            Job {
                lease,
                identity: identity.clone(),
                round: String::new(),
                state: "submitting".into(),
                prompt: prompt.clone(),
                user_message: self.conversations.get(&photo).map(|c| c.messages.len()).unwrap_or(0),
                history,
                checkpoint: Checkpoint { photo, generation, original: original.clone(), expected: original, revision, content, eligible: true },
                cancel: Arc::new(AtomicBool::new(false)),
                snapshot: None,
                plans: BTreeMap::new(),
                stopped: false,
                inflight_plan: None,
            },
        );
        let conversation = self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
        conversation.messages.push(json!({"role":"user","text":prompt,"local_round_id":identity.local_round_id}));
        conversation.notice = "正在提交…".into();
        self.save(app, photo)?;
        Ok(json!({"started":true,"photo":photo.0,"localRoundId":identity.local_round_id}))
    }
    fn spawn(&mut self, app: &mut LightcraftApp, photo: PhotoId, plan: Option<Value>, settings: Arc<DevelopSettings>) -> Result<(), String> {
        if self.worker_busy(photo) || self.workers.values().filter(|a| a.load(Ordering::Relaxed)).count() >= 4 {
            return Err("本机渲染资源忙，请稍后重新发起。".into());
        }
        let job = self.jobs.get(&photo).ok_or("AI 轮次不存在。")?;
        let mut render = app.session.candidate_job(photo, settings.clone(), 640)?;
        render.request_id = job.identity.submission_seq;
        let identity = job.identity.clone();
        let cancel = job.cancel.clone();
        let tx = self.tx.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let guard = WorkerAlive(alive.clone());
        std::thread::Builder::new()
            .name("photo-companion-render".into())
            .spawn(move || {
                let _guard = guard;
                let result = render_snapshot(render, &cancel);
                let message = WorkerMessage::Prepared { identity, plan, settings, result };
                // At most four workers and eight result slots; never wait on a stopped UI.
                if tx.try_send(message).is_err() {
                    cancel.store(true, Ordering::Relaxed);
                }
            })
            .map_err(|_| "无法启动 AI 渲染。")?;
        self.workers.insert(photo, alive);
        Ok(())
    }
    fn stop(&mut self, photo: PhotoId) -> Result<Value, String> {
        let round = if let Some(job) = self.jobs.get_mut(&photo) {
            job.stopped = true;
            job.cancel.store(true, Ordering::Relaxed);
            job.round.clone()
        } else {
            return Ok(Value::Null);
        };
        if !round.is_empty() {
            if self.send(format!("/v1/rounds/{round}/stop"), json!({"reason":"user_stop"}), 0, round).is_err() {
                self.connection = None;
            }
        } else {
            // Keep the submission identity until its admission event supplies
            // the round id. A new request cannot overtake its cancellation.
            if let Some(conversation) = self.conversations.get_mut(&photo) {
                conversation.pending.clear();
                close_pending_tools(&mut conversation.messages);
                conversation.notice = "已停止生成，正在确认取消提交。".into();
            }
            return Ok(Value::Null);
        }
        self.finish(photo, "已停止生成，保留最近一次完整结果。");
        Ok(Value::Null)
    }
    fn finish(&mut self, photo: PhotoId, notice: &str) {
        if let Some(job) = self.jobs.remove(&photo)
            && let Some(conversation) = self.conversations.get_mut(&photo)
        {
            if !job.plans.is_empty() {
                conversation.checkpoint = Some(job.checkpoint.clone());
            }
            conversation.pending.clear();
            close_pending_tools(&mut conversation.messages);
            conversation.notice = notice.into();
        }
    }
    fn server(&mut self, app: &mut LightcraftApp, ctx: &egui::Context, kind: &str, data: &Value) -> Result<(), String> {
        let round = data.get("round_id").and_then(Value::as_str).ok_or("AI 事件缺少轮次。")?;
        let payload = data.get("payload").ok_or("AI 事件缺少内容。")?;
        let photo = if kind == "round.state" {
            let seq = payload.get("submission_seq").and_then(Value::as_u64).ok_or("AI 提交关联无效。")?;
            let Some(photo) = self.jobs.iter().find_map(|(p, j)| {
                (j.identity.submission_seq == seq
                    && j.identity.scope.library_generation == app.session.library_generation()
                    && j.identity.connection_generation == self.connection_generation)
                    .then_some(*p)
            }) else {
                return Ok(());
            };
            let job = self.jobs.get_mut(&photo).ok_or("AI 轮次已结束。")?;
            if payload.pointer("/photo_ref/photo_id").and_then(Value::as_u64) != Some(photo.0)
                || payload.pointer("/photo_ref/library_generation").and_then(Value::as_u64) != Some(job.checkpoint.generation)
                || (!job.round.is_empty() && job.round != round)
            {
                return Err("AI 状态属于其他照片。".into());
            }
            job.round = round.into();
            if job.stopped {
                if self.send(format!("/v1/rounds/{round}/stop"), json!({"reason":"user_stop"}), 0, round.into()).is_err() {
                    self.connection = None;
                }
                self.finish(photo, "已停止生成，保留最近一次完整结果。");
                self.save(app, photo)?;
                return Ok(());
            }
            photo
        } else {
            let Some(photo) = self.jobs.iter().find_map(|(p, j)| (j.round == round).then_some(*p)) else {
                return Ok(());
            };
            photo
        };
        if self.jobs.get(&photo).is_none_or(|j| {
            j.identity.scope != PhotoScope { library_generation: app.session.library_generation(), photo }
                || j.identity.connection_generation != self.connection_generation
        }) {
            return Ok(());
        }
        if self.jobs.get(&photo).is_none_or(|j| !current(app, &j.checkpoint) || j.stopped) {
            let _ = self.stop(photo);
            return Ok(());
        }
        match kind {
            "round.state" => {
                let state = payload.get("state").and_then(Value::as_str).ok_or("AI 状态无效。")?.to_owned();
                let job = self.jobs.get_mut(&photo).ok_or("AI 轮次已结束。")?;
                let old = std::mem::replace(&mut job.state, state.clone());
                if state == "awaiting_input" && old != state {
                    let settings = job.checkpoint.expected.clone();
                    self.spawn(app, photo, None, settings)?;
                }
            }
            "text.delta" => {
                let id = payload.get("message_id").and_then(Value::as_str).ok_or("AI 消息编号缺失。")?;
                let delta = payload.get("delta").and_then(Value::as_str).ok_or("AI 消息内容缺失。")?;
                if delta.len() > 8192 {
                    return Err("AI 文字增量过大。".into());
                }
                let message = self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?.pending.entry(id.into()).or_default();
                if message.len().saturating_add(delta.len()) > 1024 * 1024 {
                    return Err("AI 文字超过限制。".into());
                }
                message.push_str(delta);
            }
            "message.completed" => {
                let id = payload.get("message_id").and_then(Value::as_str).ok_or("AI 消息编号缺失。")?;
                let local_round_id = self.jobs.get(&photo).ok_or("AI 轮次已结束。")?.identity.local_round_id.clone();
                let conversation = self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
                let text = conversation.pending.remove(id).unwrap_or_default();
                let calls = payload.get("tool_calls").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| json!([]));
                if !calls.is_array() {
                    return Err("AI 工具消息无效。".into());
                }
                conversation.messages.push(json!({"role":"assistant","text":text,"tool_calls":calls,"local_round_id":local_round_id}));
                self.save(app, photo)?;
            }
            "tool.request" => {
                self.tool(app, photo, payload)?;
            }
            "edit.plan" => {
                let plan_id = payload.get("plan_id").and_then(Value::as_str).ok_or("AI 方案编号缺失。")?;
                if let Some(previous) = self.jobs.get(&photo).and_then(|j| j.plans.get(plan_id)).cloned() {
                    return self.send(format!("/v1/rounds/{round}/execution"), previous, 0, round.into());
                }
                let job = self.jobs.get(&photo).ok_or("AI 轮次已结束。")?;
                if job.inflight_plan.as_deref() == Some(plan_id) {
                    return Ok(());
                }
                if job.inflight_plan.is_some()
                    || job.plans.len() >= 2
                    || !matches!(job.state.as_str(), "awaiting_initial_execution" | "awaiting_correction_execution")
                {
                    return Err("AI 方案阶段或次数不匹配。".into());
                }
                if payload.get("expected_revision").and_then(Value::as_u64) != Some(job.checkpoint.revision)
                    || payload.get("capability_version").and_then(Value::as_str) != Some("lightcraft-v2")
                {
                    return Err("AI 方案与当前照片状态不匹配。".into());
                }
                let snapshot = job.snapshot.as_ref().ok_or("当前预览未就绪。")?;
                let candidate = lightcraft_engine::companion::candidate(&job.checkpoint.expected, payload, &snapshot.context)?;
                let local_round_id = job.identity.local_round_id.clone();
                self.jobs.get_mut(&photo).ok_or("AI 轮次已结束。")?.inflight_plan = Some(plan_id.into());
                if let Some(id) = payload.get("tool_call_id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    self.conversations
                        .get_mut(&photo)
                        .ok_or("AI 会话不存在。")?
                        .messages
                        .push(json!({"role":"tool","text":"plan_proposed; await actual client execution feedback","tool_call_id":id,"local_round_id":local_round_id}));
                }
                self.spawn(app, photo, Some(payload.clone()), Arc::new(candidate))?;
            }
            "round.ended" => {
                let state = payload.get("terminal_state").and_then(Value::as_str).unwrap_or("failed");
                let reason = payload.get("reason_code").and_then(Value::as_str).unwrap_or("unknown");
                let trace = data.get("trace_id").and_then(Value::as_str).unwrap_or("-");
                let notice = if state == "completed" {
                    "本轮完成。".to_owned()
                } else if reason == "context_full" {
                    format!("上下文满了，本轮已结束；未自动删减或重试。traceId={trace}")
                } else {
                    format!("处理未完成：{reason}; traceId={trace}。保留最近一次完整结果。")
                };
                self.finish(photo, &notice);
                self.save(app, photo)?;
            }
            _ => return Err("AI 事件类型不支持。".into()),
        }
        ctx.request_repaint();
        Ok(())
    }
    fn tool(&mut self, app: &mut LightcraftApp, photo: PhotoId, payload: &Value) -> Result<(), String> {
        let job = self.jobs.get(&photo).ok_or("AI 轮次已结束。")?;
        let request = payload.get("tool_request_id").and_then(Value::as_str).ok_or("AI 工具编号缺失。")?;
        if payload.get("expected_revision").and_then(Value::as_u64) != Some(job.checkpoint.revision) {
            return Err("AI 读取版本不匹配。".into());
        }
        let snapshot = job.snapshot.as_ref().ok_or("实际照片预览尚未就绪。")?;
        let name = payload.get("tool_name").and_then(Value::as_str).ok_or("AI 工具名称缺失。")?;
        let mut images = json!([]);
        let text = match name {
            "get_photo_state" => json!({"photo_revision":job.checkpoint.revision,"settings":job.checkpoint.expected}),
            "list_edit_controls" => {
                json!({"controls":lightcraft_engine::companion::control_list(&job.checkpoint.expected,&snapshot.context.info),"edit_schemas":lightcraft_engine::companion::operation_schemas(&job.checkpoint.expected),"frame_id":snapshot.context.frame_id})
            }
            "get_current_preview" => {
                images = json!([{"mime_type":"image/jpeg","data":snapshot.preview["data_base64"]}]);
                json!({"photo_revision":job.checkpoint.revision,"width":snapshot.preview["width"],"height":snapshot.preview["height"],"frame_id":snapshot.context.frame_id})
            }
            "get_photo_histogram" => {
                let rendered = snapshot.result.rendered.as_ref().map_err(Clone::clone)?;
                let (shadow, highlight) = rendered.histogram.clipping();
                json!({"photo_revision":job.checkpoint.revision,"space":"display_srgb","histogram":rendered.histogram,"mean_luma":rendered.histogram.mean_luma(),"shadow_clipped":shadow,"highlight_clipped":highlight})
            }
            "list_manual_masks" => {
                json!({"masks":job.checkpoint.expected.masks.iter().filter(|m|lightcraft_engine::companion::is_manual(m)).map(|m|json!({"ref":format!("mask-{}",m.id),"mask":m})).collect::<Vec<_>>(),"frame_id":snapshot.context.frame_id,"coordinate_space":"native_normalized"})
            }
            "sample_photo_color" => {
                let args = payload.get("args").ok_or("取样参数缺失。")?;
                lightcraft_engine::companion::sample(&snapshot.context, &job.checkpoint.expected, args)?
            }
            _ => return Err("AI 请求了未开放的工具。".into()),
        };
        let result = json!({"text":text.to_string(),"images":images});
        let round = job.round.clone();
        let revision = job.checkpoint.revision;
        let local_round_id = job.identity.local_round_id.clone();
        self.send(
            format!("/v1/rounds/{round}/tool-results"),
            json!({"tool_request_id":request,"photo_revision":revision,"outcome":"succeeded","result":result}),
            0,
            round,
        )?;
        let id = payload.get("tool_call_id").and_then(Value::as_str).ok_or("AI 工具关联缺失。")?;
        self.conversations
            .get_mut(&photo)
            .ok_or("AI 会话不存在。")?
            .messages
            .push(json!({"role":"tool","text":result["text"],"images":result["images"],"tool_call_id":id,"local_round_id":local_round_id}));
        self.save(app, photo)
    }
    fn prepared(&mut self, app: &mut LightcraftApp, ctx: &egui::Context, message: WorkerMessage) -> Result<(), String> {
        let WorkerMessage::Prepared { identity, plan, settings, result } = message;
        let photo = identity.scope.photo;
        let seq = identity.submission_seq;
        let Some(job) = self.jobs.get(&photo) else {
            return Ok(());
        };
        if job.identity != identity
            || identity.scope.library_generation != app.session.library_generation()
            || identity.connection_generation != self.connection_generation
        {
            return Ok(());
        }
        if job.cancel.load(Ordering::Relaxed) || !current(app, &job.checkpoint) {
            self.stop(photo)?;
            return Ok(());
        }
        if self.connection.as_ref().is_none_or(|connection| !connection.alive.load(Ordering::Relaxed)) {
            return Err("AI 事件连接已结束，未发布迟到候选。".into());
        }
        let snapshot = match result {
            Ok(mut s) => {
                app.session.accept(&s.result);
                s.result.loaded = None;
                Arc::new(s)
            }
            Err(error) => {
                if let Some(plan) = &plan {
                    let round = job.round.clone();
                    self.send(
                        format!("/v1/rounds/{round}/execution"),
                        json!({"plan_id":plan["plan_id"],"outcome":"failed","photo_revision":job.checkpoint.revision,"reason_code":"render_failed"}),
                        0,
                        round,
                    )?;
                }
                self.finish(photo, &format!("处理未完成：{error}。保留最近完整结果。"));
                return self.save(app, photo);
            }
        };
        let round = job.round.clone();
        let revision = job.checkpoint.revision;
        if let Some(plan) = plan {
            let rendered = snapshot.result.rendered.as_ref().map_err(Clone::clone)?;
            let op = app.session.develop_op(photo, (*settings).clone(), "AI edit plan").ok_or("照片不存在。")?;
            app.session.commit_companion(&job.lease, &identity.local_round_id, job.checkpoint.revision, op).map_err(|e| e.to_string())?;
            app.session.accept(&snapshot.result);
            app.renderer.publish_companion(ctx, photo, snapshot.result.key, rendered, app.session.active() == Some(photo));
            let new_revision = app.session.photo_revision(photo);
            let feedback = json!({"plan_id":plan["plan_id"],"outcome":"succeeded","photo_revision":new_revision,"actual_state":settings,"preview":snapshot.preview});
            let job = self.jobs.get_mut(&photo).ok_or("AI 轮次已结束。")?;
            job.checkpoint.expected = settings.clone();
            job.checkpoint.revision = new_revision;
            let plan_id = plan.get("plan_id").and_then(Value::as_str).ok_or("AI 方案编号缺失。")?.to_owned();
            job.plans.insert(plan_id.clone(), feedback.clone());
            job.snapshot = Some(snapshot.clone());
            job.inflight_plan = None;
            let checkpoint = job.checkpoint.clone();
            let conversation = self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
            conversation.checkpoint = Some(checkpoint.clone());
            conversation.messages.push(json!({"local_round_id":identity.local_round_id,"role":"user","text":format!("Actual execution feedback: {}",json!({"outcome":"succeeded","actual_state":settings,"photo_revision":new_revision})),"images":[{"mime_type":"image/jpeg","data":snapshot.preview["data_base64"]}]}));
            conversation.rounds.push(
                json!({"local_round_id":identity.local_round_id,"round_id":round,"plan_id":plan_id,"original":checkpoint.original,"actual_state":settings,"operations":plan["operations"]}),
            );
            // A failed POST retains the actual visible result; never roll back on network failure.
            self.send(format!("/v1/rounds/{round}/execution"), feedback, 0, round)?;
            self.save(app, photo)?;
        } else {
            let job = self.jobs.get_mut(&photo).ok_or("AI 轮次已结束。")?;
            job.snapshot = Some(snapshot.clone());
            let input = json!({"input_id":format!("input-{seq}"),"photo_revision":revision,"prompt":job.prompt,"current_state":settings,"preview":snapshot.preview,"history":job.history,"capabilities":capabilities(&settings)});
            let message =
                self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?.messages.get_mut(job.user_message).ok_or("AI 用户请求缺失。")?;
            *message = json!({"local_round_id":identity.local_round_id,"role":"user","text":format!("{}\nActual photo state: {}",job.prompt,settings.to_json()),"images":[{"mime_type":"image/jpeg","data":snapshot.preview["data_base64"]}]});
            self.send(format!("/v1/rounds/{round}/input"), input, 0, round)?;
            self.save(app, photo)?;
        }
        Ok(())
    }
    fn poll_inner(&mut self, app: &mut LightcraftApp, ctx: &egui::Context) {
        let generation = app.session.library_generation();
        if self.generation != generation {
            let photos: Vec<_> = self.jobs.keys().copied().collect();
            for photo in photos {
                let _ = self.stop(photo);
                self.finish(photo, "图库已关闭，保留最近一次完整结果。");
            }
            self.connection = None;
            self.conversations.clear();
            self.generation = generation;
        }
        self.workers.retain(|_, alive| alive.load(Ordering::Relaxed));
        if self.open
            && let Some(photo) = app.session.active()
            && let Err(error) = self.load(app, photo)
        {
            self.text = error;
        }
        loop {
            let event = self.connection.as_ref().and_then(|c| c.events.try_recv().ok());
            let Some(event) = event else {
                break;
            };
            let result = match event {
                network::Event::Connected { client, connection } => {
                    let _ = (client, connection);
                    Ok(())
                }
                network::Event::Server { kind, data } => {
                    let target = self.jobs.iter().find_map(|(p, j)| {
                        (data.get("round_id").and_then(Value::as_str) == Some(j.round.as_str())
                            || data.pointer("/payload/submission_seq").and_then(Value::as_u64) == Some(j.identity.submission_seq))
                        .then_some(*p)
                    });
                    if let Err(error) = self.server(app, ctx, &kind, &data) {
                        if let Some(photo) = target {
                            self.fail_one(app, photo, &error);
                        } else {
                            self.text = error;
                        }
                    }
                    Ok(())
                }
                network::Event::Response { submission, round, path, identity, result } => {
                    if identity.scope.library_generation != app.session.library_generation()
                        || identity.connection_generation != self.connection_generation
                    {
                        continue;
                    }
                    if self.jobs.get(&identity.scope.photo).is_none_or(|j| j.identity != identity) {
                        // Stop removes the local job before its POST completes. A
                        // failed delivery still needs the original disconnect
                        // fallback to cancel the task on this same connection.
                        if path.ends_with("/stop") && result.is_err() {
                            self.connection = None;
                        }
                        continue;
                    }
                    if let Err(error) = result {
                        if path.ends_with("/stop") || (submission != 0 && !error.rejected) {
                            // A failed creation POST may have been accepted. Disconnect
                            // rather than abandon an unidentifiable server task.
                            self.connection = None;
                        }
                        let photo = identity.scope.photo;
                        if !round.is_empty() {
                            let _ = self.stop(photo);
                        }
                        self.finish(photo, &format!("处理未完成：{error}。保留最近完整结果。"));
                        if let Some(conversation) = self.conversations.get_mut(&photo) {
                            conversation.notice = format!("处理未完成：{error}。保留最近完整结果。");
                        }
                        let _ = self.save(app, photo);
                    } else if path == "/v1/rounds" {
                        // SSE owns admission and uploads; a receipt alone cannot trigger work.
                    }
                    Ok(())
                }
                network::Event::Closed(error) => {
                    let photos: Vec<_> = self.jobs.keys().copied().collect();
                    for photo in photos {
                        self.finish(photo, &error);
                        let _ = self.save(app, photo);
                    }
                    self.text = error;
                    Ok(())
                }
            };
            if let Err(error) = result {
                self.text = error;
            }
        }
        while let Ok(message) = self.rx.try_recv() {
            let WorkerMessage::Prepared { identity, .. } = &message;
            let photo = identity.scope.photo;
            if let Err(error) = self.prepared(app, ctx, message) {
                self.fail_one(app, photo, &error);
            }
        }
        if self.connection.as_ref().is_none_or(|c| !c.alive.load(Ordering::Relaxed)) && !self.jobs.is_empty() {
            let photos: Vec<_> = self.jobs.keys().copied().collect();
            for photo in photos {
                self.finish(photo, "AI 事件连接已断开，保留最近完整结果。");
                let _ = self.save(app, photo);
            }
        }
        let stale: Vec<_> = self
            .conversations
            .iter()
            .filter_map(|(p, c)| c.checkpoint.as_ref().filter(|c| c.eligible && !current(app, c) && !self.jobs.contains_key(p)).map(|_| *p))
            .collect();
        for photo in stale {
            if let Some(c) = self.conversations.get_mut(&photo).and_then(|c| c.checkpoint.as_mut()) {
                c.eligible = false;
            }
            let _ = self.save(app, photo);
        }
        if !self.jobs.is_empty() || !self.workers.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    fn fail_one(&mut self, app: &mut LightcraftApp, photo: PhotoId, error: &str) {
        let _ = self.stop(photo);
        if let Some(c) = self.conversations.get_mut(&photo) {
            c.notice = format!("处理未完成：{error}。保留最近完整结果。");
        }
        let _ = self.save(app, photo);
    }
}

fn current(app: &LightcraftApp, c: &Checkpoint) -> bool {
    app.session.library_generation() == c.generation
        && app.session.photo_revision(c.photo) == c.revision
        && app.session.catalog.photo(c.photo).is_some_and(|p| !p.deleted && lightcraft_engine::media::content_key(p) == c.content)
        && app.session.develop_of(c.photo).is_some_and(|d| *d == *c.expected)
}
fn render_snapshot(mut job: RenderJob, cancel: &AtomicBool) -> Result<Snapshot, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("已停止生成。".into());
    }
    job.cache = None;
    job.view_cache = None;
    job.stages = None;
    let source = job.source.load_source()?;
    let info = source.info_or(job.info.clone());
    let context = lightcraft_engine::companion::CandidateContext::new(
        source.image_for(job.settings.denoise_amount()),
        info,
        &job.settings,
        &format!("frame-{}-{}-{}", job.photo.0, job.request_id, job.key),
    );
    job.source = lightcraft_engine::media::SourceRef::Loaded(Box::new(source.clone()));
    let mut result = job.run();
    result.loaded = Some(source);
    let rendered = result.rendered.as_ref().map_err(Clone::clone)?;
    let options = lightcraft_engine::export::ExportOptions {
        format: lightcraft_engine::export::ExportFormat::Jpeg,
        quality: 75,
        metadata: lightcraft_engine::export::MetadataPolicy::None,
        ..Default::default()
    };
    let data = lightcraft_engine::export::encode_image(&rendered.image, &options)?;
    if data.len() > 128 * 1024 {
        return Err("压缩预览超过服务限制。".into());
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("已停止生成。".into());
    }
    let preview = json!({"mime_type":"image/jpeg","width":rendered.image.width,"height":rendered.image.height,"frame_id":context.frame_id,"data_base64":Base64::encode_string(&data)});
    Ok(Snapshot { result, preview, context })
}
fn capabilities(settings: &DevelopSettings) -> Value {
    let tools: [(&str, &str); 5] = [
        ("get_photo_state", "读取目标照片的实际编辑状态"),
        ("list_edit_controls", "读取本轮允许的参数范围和当前值"),
        ("get_current_preview", "读取目标照片当前实际 JPEG 预览"),
        ("get_photo_histogram", "读取实际预览的 sRGB 显示统计"),
        ("list_manual_masks", "读取目标照片的蒙版，未开放的类型不能编辑"),
    ];
    let mut tools:Vec<_>=tools.iter().map(|(name,description)|json!({"name":name,"description":description,"parameters":{"type":"object","properties":{},"additionalProperties":false}})).collect();
    tools.push(json!({"name":"sample_photo_color","description":"在本阶段冻结预览上从源图纯取样；返回 Point Color OkLCh、ColorRange OkLab 或白平衡数值，不写入照片。","parameters":{"type":"object","additionalProperties":false,"required":["purpose","point","frame_id","coordinate_space"],"properties":{"purpose":{"type":"string","enum":["point_color","color_range","white_balance"]},"point":{"type":"object","additionalProperties":false,"required":["x","y"],"properties":{"x":{"type":"number","minimum":0,"maximum":1},"y":{"type":"number","minimum":0,"maximum":1}}},"frame_id":{"type":"string","minLength":1,"maxLength":128},"coordinate_space":{"type":"string","enum":["native_normalized","preview_normalized"]}}}}));
    json!({"capability_version":"lightcraft-v2","controls":lightcraft_engine::companion::control_ranges(settings),"edit_schemas":lightcraft_engine::companion::operation_schemas(settings),"tools":tools})
}

fn close_pending_tools(messages: &mut Vec<Value>) {
    let mut pending = BTreeSet::new();
    for message in messages.iter() {
        if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    pending.insert(id.to_owned());
                }
            }
        }
        if let Some(id) = message.get("tool_call_id").and_then(Value::as_str) {
            pending.remove(id);
        }
    }
    for id in pending {
        messages.push(json!({"role":"tool","tool_call_id":id,"text":"cancelled or failed before confirmed tool completion"}));
    }
}

pub fn poll(app: &mut LightcraftApp, ctx: &egui::Context) {
    let mut state = std::mem::take(&mut app.ai);
    state.poll_inner(app, ctx);
    if state.open
        && let Some(photo) = app.session.active()
        && let Err(e) = state.load(app, photo)
    {
        state.text = e;
    }
    app.ai = state;
}
pub fn before_library_change(app: &mut LightcraftApp) -> Result<(), String> {
    let mut state = std::mem::take(&mut app.ai);
    let result = (|| {
        let photos: Vec<_> = state.jobs.keys().copied().collect();
        for photo in photos {
            // Close unfinished tool messages while the old library still owns them.
            let _ = state.stop(photo);
            state.finish(photo, "图库已关闭，保留最近一次完整结果。");
            state.save(app, photo)?;
        }
        state.connection = None;
        Ok(())
    })();
    app.ai = state;
    result
}
pub fn command(app: &mut LightcraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !id.starts_with("ai.") {
        return None;
    }
    let mut state = std::mem::take(&mut app.ai);
    // Commands may arrive before the next UI frame after a library change.
    if state.generation != app.session.library_generation() {
        let photos: Vec<_> = state.jobs.keys().copied().collect();
        for photo in photos {
            let _ = state.stop(photo);
            state.finish(photo, "图库已关闭，保留最近一次完整结果。");
        }
        state.connection = None;
        state.conversations.clear();
        state.generation = app.session.library_generation();
    }
    let photo = params.get("photo").and_then(Value::as_u64).map(PhotoId).or_else(|| app.session.active());
    let result = match id {
        "ai.open" => {
            state.open = true;
            Ok(Value::Null)
        }
        "ai.start" => {
            if let Some(prompt) = params.get("prompt").and_then(Value::as_str) {
                state.prompt = prompt.into();
            }
            let explicit = params
                .get("photo")
                .map(|v| v.as_u64().map(PhotoId).ok_or_else(|| tr("The requested photo must match the single selected photo.").to_owned()))
                .transpose();
            explicit.and_then(|p| state.start_for(app, p))
        }
        "ai.stop" => photo.ok_or_else(|| "请先选择照片。".into()).and_then(|p| {
            state.stop(p)?;
            state.save(app, p)?;
            Ok(Value::Null)
        }),
        "ai.status" => {
            if let Some(photo) = photo
                && let Err(error) = state.load(app, photo)
            {
                app.ai = state;
                return Some(Err(error));
            }
            let conversation = photo.and_then(|p| state.conversations.get(&p));
            Ok(json!({
                "busy": photo.is_some_and(|p| state.jobs.contains_key(&p)),
                "workerBusy": photo.is_some_and(|p| state.worker_busy(p)),
                "photo": photo.map(|p| p.0),
                "localRoundId": photo.and_then(|p| state.jobs.get(&p)).map(|j| j.identity.local_round_id.clone()),
                "activeRounds": state.jobs.values().map(|j| json!({
                    "photo": j.identity.scope.photo.0,
                    "libraryGeneration": j.identity.scope.library_generation,
                    "localRoundId": j.identity.local_round_id,
                    "submissionSeq": j.identity.submission_seq,
                    "connectionGeneration": j.identity.connection_generation,
                    "backendRoundId": (!j.round.is_empty()).then_some(j.round.as_str()),
                })).collect::<Vec<_>>(),
                "round": photo.and_then(|p| state.jobs.get(&p)).map(|j| j.round.clone()),
                "text": conversation.map(display_text).unwrap_or_else(|| state.text.clone()),
                "activePhotos": state.jobs.keys().map(|p| p.0).collect::<Vec<_>>(),
            }))
        }
        "ai.undo" => photo.ok_or_else(|| "请先选择照片。".into()).and_then(|p| undo(&mut state, app, p)),
        "ai.clear" => photo.ok_or_else(|| "请先选择照片。".into()).and_then(|p| clear(&mut state, app, p)),
        _ => {
            app.ai = state;
            return None;
        }
    };
    app.ai = state;
    Some(result)
}
fn undo(state: &mut State, app: &mut LightcraftApp, photo: PhotoId) -> Result<Value, String> {
    if state.jobs.contains_key(&photo) || state.worker_busy(photo) {
        return Err("请先停止并等待此照片处理结束。".into());
    }
    state.load(app, photo)?;
    let checkpoint = state.conversations.get(&photo).and_then(|c| c.checkpoint.clone()).ok_or("此照片没有可撤回的 AI 轮次。")?;
    if !checkpoint.eligible || !current(app, &checkpoint) {
        return Err("照片已有后续编辑，请使用历史恢复，未覆盖当前照片。".into());
    }
    let op = app.session.develop_op(photo, (*checkpoint.original).clone(), "Undo AI round").ok_or("照片不存在。")?;
    app.session.commit("Undo AI round", op).map_err(|e| e.to_string())?;
    let conversation = state.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
    if let Some(c) = conversation.checkpoint.as_mut() {
        c.eligible = false;
    }
    conversation.messages.push(json!({"role":"user","text":"用户撤回了本轮全部 AI 修改，照片恢复到本轮起点。"}));
    conversation.notice = "已撤回本轮，保留对话。".into();
    state.save(app, photo)?;
    Ok(json!({"undone":true}))
}
fn clear(state: &mut State, app: &mut LightcraftApp, photo: PhotoId) -> Result<Value, String> {
    if state.jobs.contains_key(&photo) || state.worker_busy(photo) {
        return Err("处理期间不能删除对话，请先停止并等待结束。".into());
    }
    state.load(app, photo)?;
    let names: BTreeSet<_> = state
        .conversations
        .get(&photo)
        .into_iter()
        .flat_map(|c| c.messages.iter())
        .flat_map(|m| m.get("images").and_then(Value::as_array).into_iter().flatten())
        .filter_map(|p| p.get("attachment").and_then(Value::as_str).map(str::to_owned))
        .collect();
    app.session.set_photo_companion(photo, None).map_err(|e| e.to_string())?;
    app.session.persist().map_err(|e| e.to_string())?;
    state.conversations.insert(
        photo,
        Conversation { scope: Some(PhotoScope { library_generation: app.session.library_generation(), photo }), ..Default::default() },
    );
    if let Some(library) = app.session.library.as_mut() {
        for name in names {
            library.remove_companion_attachment(&name).map_err(|e| e.to_string())?;
        }
    }
    Ok(json!({"cleared":true}))
}
fn display_text(conversation: &Conversation) -> String {
    let mut text = String::new();
    for message in &conversation.messages {
        if matches!(message.get("role").and_then(Value::as_str), Some("user" | "assistant")) {
            let value = message.get("text").and_then(Value::as_str).unwrap_or_default();
            if !value.starts_with("Actual execution feedback:") {
                let visible = value.split("\nActual photo state:").next().unwrap_or(value);
                if !visible.is_empty() {
                    text.push_str(visible);
                    text.push_str("\n\n");
                }
            }
        }
    }
    for value in conversation.pending.values() {
        text.push_str(value);
    }
    text.push_str(&conversation.notice);
    text
}
pub fn show(app: &mut LightcraftApp, ctx: &egui::Context) {
    if !app.ai.open {
        return;
    }
    let mut open = true;
    egui::Window::new("AI Photo Companion").id(egui::Id::new("photo-companion-window")).open(&mut open).default_width(420.0).show(ctx, |ui| {
        if let Some(photo) = app.session.active() {
            panel(app, ui, photo);
        } else {
            ui.label(tr("Select exactly one photo to start an AI task."));
        }
    });
    app.ai.open = open;
}
fn panel(app: &mut LightcraftApp, ui: &mut egui::Ui, photo: PhotoId) {
    egui::Frame::NONE.inner_margin(egui::Margin::same(16)).show(ui, |ui| {
        if let Some(p) = app.session.catalog.photo(photo) {
            ui.label(p.file_name.as_str());
        }
        ui.label(tr("Understand and naturally optimize your photo. Originals stay on this computer."));
        let busy = app.ai.jobs.contains_key(&photo);
        if app.session.companion_task_photo() == Some(photo) {
            ui.label(tr("Manual editing is paused while AI processes this photo; stop it or wait for completion."));
        }
        let worker = app.ai.worker_busy(photo);
        let input = ui.add_enabled(
            !busy,
            egui::TextEdit::multiline(&mut app.ai.prompt).desired_rows(2).hint_text(tr("For example: review this photo, then naturally optimize it")),
        );
        crate::widgets::register(ui.ctx(), "field:aiPrompt", input.rect);
        if !busy {
            let consent = ui.checkbox(&mut app.ai.consent, tr("Allow compressed previews to be sent to the cloud model"));
            crate::widgets::register(ui.ctx(), "checkbox:aiConsent", consent.rect);
            let admission = app.ai.admission(app, None);
            if let Err(message) = &admission {
                ui.label(message);
            }
            let start = ui.add_enabled(admission.is_ok(), egui::Button::new(tr("Analyze and Optimize")));
            crate::widgets::register(ui.ctx(), "button:aiStart", start.rect);
            if start.clicked()
                && let Err(e) = app.run("ai.start", json!({}))
            {
                app.ai.text = e.clone();
                app.ai.conversations.entry(photo).or_default().notice = e;
            }
            if worker {
                ui.label(tr("Finishing processing for this photo…"));
            }
            for (label, widget, command) in
                [(tr("Undo AI Round"), "button:aiUndo", "ai.undo"), (tr("Delete Conversation"), "button:aiClear", "ai.clear")]
            {
                let button = ui.add_enabled(!worker, egui::Button::new(label));
                crate::widgets::register(ui.ctx(), widget, button.rect);
                if button.clicked()
                    && let Err(e) = app.run(command, json!({}))
                {
                    app.ai.conversations.entry(photo).or_default().notice = e;
                }
            }
        } else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr("Processing this photo…"));
                let stop = ui.button(tr("Stop Generating"));
                crate::widgets::register(ui.ctx(), "button:aiStop", stop.rect);
                if stop.clicked() {
                    let _ = app.run("ai.stop", json!({}));
                }
            });
        }
        if let Some(conversation) = app.ai.conversations.get(&photo) {
            egui::ScrollArea::vertical().id_salt(("ai-messages", photo.0)).max_height(240.0).stick_to_bottom(true).show(ui, |ui| {
                ui.label(display_text(conversation));
            });
        } else if !app.ai.text.is_empty() {
            ui.label(&app.ai.text);
        }
    });
}
