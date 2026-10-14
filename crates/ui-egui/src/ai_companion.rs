//! Photo-bound conversations and the native Photo Companion round protocol.
#[cfg(test)]
#[path = "ai_companion/admission_tests.rs"]
mod admission_tests;
mod archive;
mod attachments;
#[cfg(test)]
#[path = "ai_companion/cancellation_tests.rs"]
mod cancellation_tests;
mod card;
#[cfg(test)]
#[path = "ai_companion/card_tests.rs"]
mod card_tests;
#[cfg(test)]
mod clear_tests;
mod comparison;
#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod damaged_tests;
#[cfg(test)]
#[path = "ai_companion/disconnect_tests.rs"]
mod disconnect_tests;
mod drafts;
mod errors;
mod health;
#[cfg(test)]
mod health_task_tests;
mod historical;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
#[path = "ai_companion/identity_tests.rs"]
mod identity_tests;
mod interrupted;
mod legacy;
#[cfg(test)]
#[path = "ai_companion/library_tests.rs"]
mod library_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod native_history_tests;
mod network;
mod paging;
#[cfg(test)]
mod paging_tests;
#[cfg(test)]
#[path = "ai_companion/panel_tests.rs"]
mod panel_tests;
#[cfg(test)]
mod persistence_tests;
mod projection;
mod reading;
#[cfg(test)]
mod reading_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
#[path = "ai_companion/revert_tests.rs"]
mod revert_tests;
mod task_entry;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

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
    health: health::Health,
    telemetry: network::Telemetry,
    task: Option<task_entry::Task>,
    readonly_task: Option<PhotoScope>,
    readonly_selection: Option<lightcraft_engine::Selection>,
    consent: bool,
    exiting: bool,
    prompt: String,
    editor_photo: Option<PhotoId>,
    editors: BTreeMap<PhotoId, drafts::Editor>,
    dirty_editors: BTreeSet<PhotoId>,
    pub text: String,
    generation: u64,
    serial: u64,
    connection_generation: u64,
    connection: Option<network::Connection>,
    retiring_connection: Option<Arc<AtomicBool>>,
    retiring_migrations: BTreeMap<u64, lightcraft_engine::companion::MigrationRetirement>,
    comparison: Option<comparison::Comparison>,
    comparison_workers: Vec<Arc<AtomicBool>>,
    #[cfg(test)]
    next_connection: Option<network::Connection>,
    pending_confirmations: Vec<(RoundIdentity, Option<String>)>,
    confirmation_retry_at: Option<Instant>,
    paging: paging::Paging,
    bottom_reading: BTreeSet<PhotoId>,
    attachments: attachments::Attachments,
    conversations: BTreeMap<PhotoId, Conversation>,
    pending_local_saves: BTreeSet<PhotoId>,
    save_retry_feedback: BTreeMap<PhotoId, &'static str>,
    local_save_retry_at: Option<Instant>,
    local_save_after_frame: Option<u64>,
    feedback_pending: bool,
    jobs: BTreeMap<PhotoId, Job>,
    workers: BTreeMap<PhotoId, Arc<AtomicBool>>,
    tx: mpsc::SyncSender<WorkerMessage>,
    rx: mpsc::Receiver<WorkerMessage>,
    completed: VecDeque<WorkerMessage>,
}
impl Default for State {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(8);
        Self {
            open: false,
            health: health::Health::default(),
            telemetry: network::Telemetry::default(),
            task: None,
            readonly_task: None,
            readonly_selection: None,
            consent: false,
            exiting: false,
            prompt: String::new(),
            editor_photo: None,
            editors: BTreeMap::new(),
            dirty_editors: BTreeSet::new(),
            text: String::new(),
            generation: 0,
            serial: 0,
            connection_generation: 0,
            connection: None,
            retiring_connection: None,
            retiring_migrations: BTreeMap::new(),
            comparison: None,
            comparison_workers: Vec::new(),
            #[cfg(test)]
            next_connection: None,
            pending_confirmations: Vec::new(),
            confirmation_retry_at: None,
            paging: paging::Paging::default(),
            bottom_reading: BTreeSet::new(),
            attachments: attachments::Attachments::default(),
            conversations: BTreeMap::new(),
            pending_local_saves: BTreeSet::new(),
            save_retry_feedback: BTreeMap::new(),
            local_save_retry_at: None,
            local_save_after_frame: None,
            feedback_pending: false,
            jobs: BTreeMap::new(),
            workers: BTreeMap::new(),
            tx,
            rx,
            completed: VecDeque::new(),
        }
    }
}
struct HistoryView {
    captured_header: lightcraft_engine::companion::storage::PhotoHeader,
    _retention: lightcraft_engine::companion::storage::HistoryRetention,
    items: Vec<lightcraft_engine::companion::storage::HistoryItem>,
    cursor: lightcraft_engine::companion::storage::HistoryCursor,
    next: Option<lightcraft_engine::companion::storage::HistoryCursor>,
    messages: Vec<Value>,
    fragments: Vec<String>,
    fragment_messages: BTreeMap<u64, Value>,
    bytes: usize,
    cards: Vec<card::Card>,
    diagnostic: Option<String>,
    legacy_plan: Option<Value>,
    legacy_scan: Option<lightcraft_engine::companion::storage::DisplayScan>,
    preview: Option<historical::Preview>,
    cards_bytes: usize,
    card_cursor: Option<u64>,
    card_pending: BTreeSet<String>,
    card_owners: VecDeque<u64>,
    card_scan: Option<lightcraft_engine::companion::storage::DisplayScan>,
}
const ROOT_RECOVERY_GUIDANCE: &str =
    "AI history is protected. Original files are kept. Back up the library, restore a verified companion backup, then reopen it.";
enum HistoryError {
    Data(String),
    Ownership(String),
}
impl HistoryError {
    fn reason(&self) -> &str {
        match self {
            Self::Data(reason) | Self::Ownership(reason) => reason,
        }
    }
    fn ownership_unknown(&self) -> bool {
        matches!(self, Self::Ownership(_))
    }
    fn guidance(&self) -> &'static str {
        match self {
            Self::Data(_) => {
                "Some AI history could not be verified. Readable records are kept. Repair the damaged data or delete this conversation before submitting again."
            }
            Self::Ownership(_) => {
                "This photo's AI history cannot be verified. Original files are kept. Repair it from this library's verified backup, then reopen the library."
            }
        }
    }
}
struct Conversation {
    scope: Option<PhotoScope>,
    schema_version: u64,
    legacy_cursor: u64,
    preview_cursor: u64,
    legacy_readonly: bool,
    history_error: Option<HistoryError>,
    recovery_checked: bool,
    recovery_scan: Option<interrupted::Scan>,
    prefix: Option<lightcraft_engine::companion::storage::PhotoHeader>,
    view: Option<HistoryView>,
    dirty: bool,
    dirty_queued: bool,
    storage_notice: bool,
    records: Vec<Value>,
    messages: Vec<Value>,
    rounds: Vec<Value>,
    pending: BTreeMap<String, String>,
    notice: String,
    checkpoint: Option<Checkpoint>,
}
impl Default for Conversation {
    fn default() -> Self {
        Self {
            scope: None,
            schema_version: 2,
            legacy_cursor: 0,
            preview_cursor: 0,
            legacy_readonly: false,
            history_error: None,
            recovery_checked: false,
            recovery_scan: None,
            prefix: None,
            view: None,
            dirty: false,
            dirty_queued: false,
            storage_notice: false,
            records: Vec::new(),
            messages: Vec::new(),
            rounds: Vec::new(),
            pending: BTreeMap::new(),
            notice: String::new(),
            checkpoint: None,
        }
    }
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
    local_round_id: Option<String>,
}
struct Job {
    lease: LeaseToken,
    identity: RoundIdentity,
    round: String,
    state: String,
    prompt: String,
    draft_revision: u64,
    user_message: usize,
    history: Vec<Value>,
    history_preflight: bool,
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
    HistoryPrepared { identity: RoundIdentity, result: Result<Vec<Value>, String> },
    Prepared { identity: RoundIdentity, plan: Option<Value>, settings: Arc<DevelopSettings>, result: Result<Box<Snapshot>, String> },
}
struct WorkerAlive(Arc<AtomicBool>);
impl Drop for WorkerAlive {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

impl State {
    fn leave_task_view(&mut self) {
        self.comparison = None;
        if self.readonly_task.take().is_some() {
            self.paging.cancel();
        }
        self.readonly_selection = None;
    }
    fn sync_task_selection(&mut self, app: &LightcraftApp) {
        if self.readonly_task.is_some() && self.readonly_selection.as_ref().is_some_and(|selection| selection != &app.session.selection) {
            self.leave_task_view();
        }
    }
    fn displayed_photo(&self, app: &LightcraftApp) -> Option<PhotoId> {
        self.readonly_task
            .filter(|scope| scope.library_generation == app.session.library_generation())
            .map(|scope| scope.photo)
            .or_else(|| app.session.active())
    }
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
        let cancel = self.jobs.get(&identity.scope.photo).ok_or("AI 轮次已结束。")?.cancel.clone();
        self.enqueue(path, body, submission, round, identity, cancel)
    }
    fn enqueue(
        &self,
        path: String,
        body: Value,
        submission: u64,
        round: String,
        identity: RoundIdentity,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let control = path.ends_with("/stop");
        if !control && (!self.consent || cancel.load(Ordering::Relaxed)) {
            return Err("已取消未完成的 AI 请求。".into());
        }
        let connection = self.connection.as_ref().ok_or("AI 事件连接尚未建立。")?;
        let queue = if control { &connection.controls } else { &connection.requests };
        queue
            .try_send(network::Request { path, body, submission, round, identity, cancel })
            .map_err(|_| "AI 请求交付积压，请稍候；未自动重试。".into())
    }
    fn connect(&mut self, app: &LightcraftApp) -> Result<network::Connection, String> {
        #[cfg(test)]
        if let Some(connection) = self.next_connection.take() {
            return Ok(connection);
        }
        network::connect(&network::service_address(app.services.companion_address.as_deref()))
    }
    fn retire_disconnected_jobs(&mut self, app: &mut LightcraftApp) -> Result<(), String> {
        let photos: Vec<_> = self
            .jobs
            .iter()
            .filter_map(|(photo, job)| (job.identity.connection_generation == self.connection_generation).then_some(*photo))
            .collect();
        // The receiver may still contain Closed. Drop that connection only after
        // retaining its ownership list; no old route can outlive replacement.
        self.retire_connection();
        for photo in &photos {
            self.disconnected(*photo, "AI 事件连接已断开。");
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
        if self.exiting || app.quit_confirmed {
            return Err(tr("The app is closing. AI tasks cannot start.").into());
        }
        if self.readonly_task.is_some() || self.comparison.is_some() {
            return Err(tr("Return to the selected photo before starting a new AI task.").into());
        }
        if app.session.companion_migration_pending() {
            return Err(tr("Legacy conversation is being upgraded. New AI tasks are paused.").into());
        }
        if app.session.companion_archive_protected_reason().is_some() {
            return Err(tr("This conversation format is protected. Original history is retained.").into());
        }
        if self.pending_confirmations.len() >= 96 {
            return Err(tr("AI history is waiting to be saved. Retry after saving completes.").into());
        }
        if cfg!(target_arch = "wasm32") {
            return Err(tr("AI Companion is currently available on desktop only.").into());
        }
        if !self.consent {
            return Err(tr("Please consent to sending compressed previews first.").into());
        }
        if self.retiring_connection.as_ref().is_some_and(|alive| alive.load(Ordering::Relaxed))
            || self.connection.as_ref().is_some_and(|c| c.is_closing() && c.alive.load(Ordering::Relaxed))
            || !self.jobs.is_empty()
            || self.workers.values().any(|a| a.load(Ordering::Relaxed))
            || app.session.companion_task_photo().is_some()
        {
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
        if self.conversations.get(&photo).is_some_and(|conversation| conversation.history_error.is_some()) {
            return Err(projection::invalid());
        }
        Ok(photo)
    }
    #[cfg(test)]
    fn start(&mut self, app: &mut LightcraftApp) -> Result<Value, String> {
        self.start_for(app, None)
    }
    #[cfg(test)]
    fn start_for(&mut self, app: &mut LightcraftApp, explicit: Option<PhotoId>) -> Result<Value, String> {
        self.start_with_prompt(app, explicit, None)
    }
    fn start_with_prompt(&mut self, app: &mut LightcraftApp, explicit: Option<PhotoId>, supplied_prompt: Option<&str>) -> Result<Value, String> {
        if self.connection.as_ref().is_none_or(|c| !c.alive.load(Ordering::Relaxed)) {
            self.retire_disconnected_jobs(app)?;
        }
        let photo = self.admission(app, explicit)?;
        self.sync_editor(app)?;
        if let Some(prompt) = supplied_prompt {
            self.prompt = prompt.into();
        }
        app.session.end_interaction().map_err(|e| e.to_string())?;
        let original = app.session.develop_of(photo).ok_or("照片不存在。")?;
        let content = app.session.catalog.photo(photo).map(|p| lightcraft_engine::media::content_key(p)).ok_or("照片不存在。")?;
        let prompt =
            if self.prompt.trim().is_empty() { tr("Please review this photo and optimize it naturally.").into() } else { self.prompt.clone() };
        if prompt.len() > 4096 {
            return Err("请求过长。".into());
        }
        self.load(app, photo)?;
        if self.conversations.get(&photo).is_some_and(|conversation| conversation.history_error.is_some()) {
            return Err(projection::invalid());
        }
        if self.connection.is_none() {
            self.connection_generation = self.connection_generation.checked_add(1).ok_or("AI 连接编号耗尽，请重新启动客户端。")?;
            self.connection = Some(self.connect(app)?);
            self.telemetry = network::Telemetry::default();
        }
        self.telemetry.new_round();
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
        let cancel = Arc::new(AtomicBool::new(false));
        let reader = self
            .conversations
            .get(&photo)
            .and_then(|c| c.prefix.as_ref())
            .map(|h| app.session.companion_owned_history_reader(h))
            .transpose()
            .map_err(|e| e.to_string())?
            .flatten();
        if reader.is_none() && self.conversations.get(&photo).is_some_and(|c| c.prefix.is_some()) {
            return Err(tr("AI history preparation failed. Try again.").into());
        }
        let history = if reader.is_some() { Vec::new() } else { self.history(app, photo)? };
        if reader.is_none() {
            projection::reserve_input(&history, &prompt, &original)?;
        }
        let history_tail = self.conversations.get(&photo).map(|c| c.messages.clone()).unwrap_or_default();
        if app.session.library.is_some()
            && self
                .conversations
                .get(&photo)
                .is_some_and(|c| c.schema_version == 2 && (!c.messages.is_empty() || !c.records.is_empty() || !c.rounds.is_empty()))
        {
            self.save(app, photo)?;
            let prefix = app.session.companion_photo_header(photo).map_err(|e| e.to_string())?;
            app.session.pin_companion_prefix(photo, prefix.clone()).map_err(|e| e.to_string())?;
            if let Some(c) = self.conversations.get_mut(&photo) {
                c.prefix = prefix;
                c.messages.clear();
                c.records.clear();
                c.rounds.clear();
                c.view = None;
            }
        }
        if reader.is_none() {
            self.enqueue(
                "/v1/rounds".into(),
                json!({"submission_seq":self.serial,"photo_ref":{"library_generation":generation,"photo_id":photo.0},"photo_revision":revision}),
                self.serial,
                String::new(),
                identity.clone(),
                cancel.clone(),
            )?;
        }
        self.jobs.insert(
            photo,
            Job {
                lease,
                identity: identity.clone(),
                round: String::new(),
                state: "submitting".into(),
                prompt: prompt.clone(),
                draft_revision: 0,
                user_message: self.conversations.get(&photo).map(|c| c.messages.len()).unwrap_or(0),
                history,
                history_preflight: reader.is_some(),
                checkpoint: Checkpoint {
                    photo,
                    generation,
                    original: original.clone(),
                    expected: original,
                    revision,
                    content,
                    eligible: true,
                    local_round_id: Some(identity.local_round_id.as_str().to_owned()),
                },
                cancel,
                snapshot: None,
                plans: BTreeMap::new(),
                stopped: false,
                inflight_plan: None,
            },
        );
        self.task = Some(task_entry::Task::new(identity.clone(), app.session.catalog.photo(photo).map(|p| p.file_name.clone()).unwrap_or_default()));
        let draft_revision = self.clear_submitted_draft(app, photo)?;
        if let Some(job) = self.jobs.get_mut(&photo) {
            job.draft_revision = draft_revision;
        }
        let conversation = self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
        conversation.records.push(json!({"local_round_id":identity.local_round_id,"photo_id":photo.0,"request_text":prompt,"submission_seq":identity.submission_seq,"connection_generation":identity.connection_generation,"runtime_library_generation":identity.scope.library_generation,"original":self.jobs.get(&photo).map(|j| &j.checkpoint.original),"stages":[],"phase":"submitting"}));
        conversation.messages.push(json!({"role":"user","text":prompt,"local_round_id":identity.local_round_id,"local_message_id":format!("{}-user",identity.local_round_id.as_str()),"complete_state":"complete","display_kind":"request"}));
        conversation.storage_notice = false;
        conversation.notice = tr("Submitting…").into();
        if let Err(error) = self.save(app, photo) {
            if reader.is_some() {
                self.mark_failure(photo, &error);
                let _ = self.stop(photo);
            }
            return Err(error);
        }
        if let Some(reader) = reader {
            let job = self.jobs.get(&photo).ok_or("AI 轮次不存在。")?;
            // This owned envelope belongs to the leased submission, not later drafts.
            let (prompt, original, cancel) = (job.prompt.clone(), job.checkpoint.original.clone(), job.cancel.clone());
            if let Err(error) = self.begin_history(reader, identity.clone(), history_tail, prompt, original, cancel) {
                self.fail_one(app, photo, &error);
                return Err(error);
            }
        }
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
                let result = render_snapshot(render, &cancel).map(Box::new);
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
    fn set_consent(&mut self, consent: bool) -> Result<(), String> {
        self.consent = consent;
        if !consent {
            let photos: Vec<_> = self.jobs.keys().copied().collect();
            self.pending_local_saves.extend(photos.iter().copied());
            for photo in photos {
                self.stop(photo)?;
            }
        }
        Ok(())
    }
    fn request_feedback(&mut self) {
        // Commands can cancel outside a UI pass; bind their first feedback frame
        // when poll next sees the actual app context.
        self.feedback_pending = true;
        self.local_save_after_frame = None;
    }
    fn defer_local_saves_for_feedback(&mut self, ctx: &egui::Context) {
        self.feedback_pending = false;
        self.local_save_after_frame = Some(ctx.cumulative_frame_nr());
    }
    pub(crate) fn feedback_needs_paint(&self, ctx: &egui::Context) -> bool {
        self.feedback_pending || self.local_save_after_frame.is_some_and(|frame| ctx.cumulative_frame_nr() <= frame)
    }
    fn flush_local_saves(&mut self, app: &mut LightcraftApp) -> Result<(), String> {
        let mut failure = None;
        for photo in self.pending_local_saves.iter().copied().collect::<Vec<_>>() {
            if let Err(error) = self.save(app, photo) {
                failure.get_or_insert(error);
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn close_connection(&self) {
        if let Some(connection) = &self.connection {
            connection.close();
        }
    }
    fn retire_connection(&mut self) {
        if let Some(connection) = self.connection.take() {
            connection.close();
            if connection.alive.load(Ordering::Relaxed) {
                // Keep the real transport's join/retirement guard across library
                // generations even though its old event outlet is discarded.
                self.retiring_connection = Some(connection.alive.clone());
            }
        }
    }
    fn stop(&mut self, photo: PhotoId) -> Result<Value, String> {
        if self.jobs.contains_key(&photo) {
            self.request_feedback();
        }
        let recovery = self.jobs.get(&photo).map(|job| (job.identity.scope, job.draft_revision, job.prompt.clone()));
        if self.jobs.get(&photo).is_some_and(|j| j.history_preflight) {
            if let Some(job) = self.jobs.get_mut(&photo) {
                job.cancel.store(true, Ordering::Relaxed);
                job.stopped = true;
            }
            self.finish(photo, "已停止，未发送模型请求。");
            return Ok(json!({"stopped":true}));
        }
        let round = if let Some(job) = self.jobs.get_mut(&photo) {
            job.stopped = true;
            job.cancel.store(true, Ordering::Relaxed);
            job.round.clone()
        } else {
            return Ok(Value::Null);
        };
        if let Some((scope, revision, prompt)) = recovery {
            self.restore_text(photo, scope, revision, &prompt);
        }
        if !round.is_empty() {
            if self.send(format!("/v1/rounds/{round}/stop"), json!({"reason":"user_stop"}), 0, round).is_err() {
                self.close_connection();
            }
        } else {
            // Keep the submission identity until its admission event supplies
            // the round id. A new request cannot overtake its cancellation.
            if let Some(conversation) = self.conversations.get_mut(&photo) {
                conversation.pending.clear();
                close_pending_tools(&mut conversation.messages, "cancelled");
                conversation.notice = tr("Stopped generating. Confirming cancellation of the submission…").into();
            }
            return Ok(Value::Null);
        }
        self.finish(photo, tr("Stopped generating. Keeping the latest complete result."));
        Ok(Value::Null)
    }
    fn disconnected(&mut self, photo: PhotoId, diagnostic: &str) {
        let Some(job) = self.jobs.get(&photo) else {
            return;
        };
        let stopped = job.stopped;
        let record = self
            .conversations
            .get_mut(&photo)
            .and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == job.identity.local_round_id.as_str()));
        let published = record.as_deref().is_some_and(errors::published);
        if let Some(record) = record {
            if record.get("local_terminal").is_none() {
                record["local_terminal"] = json!(if stopped { "cancelled" } else { "disconnected" });
            }
            record["transport_diagnostic"] = json!(diagnostic.chars().take(4096).collect::<String>());
        }
        self.finish(photo, &errors::notice(if stopped { "user_stop" } else { "network_failure" }, published));
    }
    fn finish(&mut self, photo: PhotoId, notice: &str) {
        let Some(job) = self.jobs.remove(&photo) else { return };
        let completed = self.conversations.get(&photo).is_some_and(|c| {
            c.records.iter().any(|r| r["local_round_id"] == job.identity.local_round_id.as_str() && r["local_terminal"] == "completed")
        });
        if let Some(conversation) = self.conversations.get_mut(&photo) {
            if !job.plans.is_empty() {
                conversation.checkpoint = Some(job.checkpoint.clone());
            }
            if let Some(record) = conversation.records.iter_mut().find(|r| r["local_round_id"] == job.identity.local_round_id.as_str()) {
                if record.get("local_terminal").is_none() {
                    record["local_terminal"] = json!(if job.stopped { "cancelled" } else { "failed" });
                }
                let cancelled = record["local_terminal"] == "cancelled";
                if let Some(stages) = record.get_mut("stages").and_then(Value::as_array_mut) {
                    for step in stages {
                        if step["execution_outcome"] == "not_executed" {
                            step["execution_outcome"] = json!(if cancelled { "cancelled" } else { "failed" });
                            conversation.messages.push(json!({"role":"user","display_kind":"execution_feedback","complete_state":"complete","local_round_id":job.identity.local_round_id,"local_message_id":format!("{}-{}-not-executed",job.identity.local_round_id.as_str(),step["plan_id"]),"text":format!("Actual execution feedback: {}",json!({"plan_id":step.get("plan_id"),"outcome":if cancelled {"cancelled"} else {"failed"},"executed":false,"reason_code":"ended_before_plan_publication","photo_revision":job.checkpoint.revision,"actual_state":job.checkpoint.expected}))}));
                        }
                    }
                }
                if let Some(requests) = record.get_mut("tool_requests").and_then(Value::as_array_mut) {
                    for request in requests {
                        if request["outcome"] == "not_executed" {
                            request["outcome"] = json!(if cancelled { "cancelled" } else { "failed" });
                        }
                    }
                }
                let terminal_notice =
                    record.pointer("/local_failure/message").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| notice.into());
                record["terminal_notice"] = json!(terminal_notice);
                record["backend_round_id"] = if job.round.is_empty() { Value::Null } else { json!(job.round) };
            }
            conversation.pending.clear();
            let terminal = conversation
                .records
                .iter()
                .find(|r| r["local_round_id"] == job.identity.local_round_id.as_str())
                .and_then(|r| r["local_terminal"].as_str())
                .unwrap_or("failed");
            close_pending_tools(&mut conversation.messages, terminal);
            conversation.notice = notice.into();
        }
        if let Some(task) = self.task.as_mut().filter(|t| t.identity == job.identity) {
            let terminal = self
                .conversations
                .get(&photo)
                .and_then(|c| c.records.iter().find(|r| r["local_round_id"] == job.identity.local_round_id.as_str()))
                .and_then(|r| r["local_terminal"].as_str())
                .unwrap_or(if job.stopped { "cancelled" } else { "failed" });
            task.terminal = Some(terminal.to_owned());
            task.unread = true;
        }
        if !completed {
            self.restore_request(photo, &job);
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
                    self.close_connection();
                }
                self.finish(photo, tr("Stopped generating. Keeping the latest complete result."));
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
                if let Some(task) = self.task.as_mut().filter(|t| t.identity == job.identity) {
                    task.phase = state.clone();
                }
                if let Some(conversation) = self.conversations.get_mut(&photo) {
                    conversation.notice = tr(phase_text(&state)).into();
                }

                if let Some(record) = self
                    .conversations
                    .get_mut(&photo)
                    .and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == job.identity.local_round_id.as_str()))
                {
                    record["phase"] = json!(state);
                    record["backend_round_id"] = json!(round);
                }
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
                if !delta.is_empty() {
                    self.telemetry.text();
                }
            }
            "message.completed" => {
                let id = payload.get("message_id").and_then(Value::as_str).ok_or("AI 消息编号缺失。")?;
                let local_round_id = self.jobs.get(&photo).ok_or("AI 轮次已结束。")?.identity.local_round_id.clone();
                let stage = data.get("stage").or_else(|| payload.get("stage"));
                let conversation = self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
                let text = conversation.pending.remove(id).unwrap_or_default();
                let calls = payload.get("tool_calls").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| json!([]));
                if !calls.is_array() {
                    return Err("AI 工具消息无效。".into());
                }
                conversation.messages.push(json!({"role":"assistant","text":text,"tool_calls":calls,"local_round_id":local_round_id,"local_message_id":format!("{}-{}-{id}",local_round_id.as_str(),stage.and_then(Value::as_str).unwrap_or("unknown")),"model_call_id":payload.get("model_call_id"),"stage":stage,"complete_state":"complete","display_kind":"assistant"}));
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
                let candidate = lightcraft_engine::companion::candidate(&job.checkpoint.expected, payload, &snapshot.context);
                let local_round_id = job.identity.local_round_id.clone();
                let stage = if job.plans.is_empty() { "initial" } else { "correction" };
                let before = job.checkpoint.expected.clone();
                let (display_before, display_target) =
                    card::capture_pair(&before, candidate.as_ref().unwrap_or(before.as_ref()), &snapshot.context.info);
                let model_call = self
                    .conversations
                    .get(&photo)
                    .and_then(|c| {
                        c.messages.iter().rev().find(|m| {
                            m.get("tool_calls")
                                .and_then(Value::as_array)
                                .is_some_and(|calls| calls.iter().any(|call| call.get("id") == payload.get("tool_call_id")))
                        })
                    })
                    .and_then(|m| m.get("model_call_id"))
                    .cloned();
                if let Some(record) =
                    self.conversations.get_mut(&photo).and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == local_round_id.as_str()))
                    && let Some(stages) = record.get_mut("stages").and_then(Value::as_array_mut)
                {
                    stages.push(json!({"stage":stage,"backend_stage":data.get("stage"),"model_call_id":model_call,"plan_id":plan_id,"tool_call_id":payload.get("tool_call_id"),"reason":payload.get("reason"),"operations":payload.get("operations"),"before_state":before,"display_before":display_before,"display_target":display_target,"proposed_state":candidate.as_ref().ok(),"execution_outcome":if candidate.is_ok() {"not_executed"} else {"validation_failed"}}));
                }
                if let Some(id) = payload.get("tool_call_id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    self.conversations
                        .get_mut(&photo)
                        .ok_or("AI 会话不存在。")?
                        .messages
                        .push(json!({"role":"tool","text":"plan_proposed; await actual client execution feedback","tool_call_id":id,"local_round_id":local_round_id,"tool_request_id":payload.get("tool_request_id"),"stage":payload.get("stage"),"model_call_id":payload.get("model_call_id"),"complete_state":"complete"}));
                }
                let candidate = match candidate {
                    Ok(candidate) => candidate,
                    Err(error) => {
                        self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?.messages.push(json!({"role":"user","display_kind":"execution_feedback","complete_state":"complete","local_round_id":local_round_id,"local_message_id":format!("{}-{plan_id}-validation-failure",local_round_id.as_str()),"text":format!("Actual execution feedback: {}",json!({"plan_id":plan_id,"outcome":"failed","reason_code":"validation_failed","executed":false,"photo_revision":payload.get("expected_revision"),"actual_state":before}))}));
                        return Err(error);
                    }
                };
                self.jobs.get_mut(&photo).ok_or("AI 轮次已结束。")?.inflight_plan = Some(plan_id.into());
                self.spawn(app, photo, Some(payload.clone()), Arc::new(candidate))?;
            }
            "round.ended" => {
                let state = payload.get("terminal_state").and_then(Value::as_str).unwrap_or("failed");
                let reason = payload.get("reason_code").and_then(Value::as_str).unwrap_or("unknown");
                let published = self
                    .conversations
                    .get(&photo)
                    .and_then(|c| c.records.iter().find(|r| r["backend_round_id"] == round))
                    .is_some_and(errors::published);
                let notice = if state == "completed" { tr("This round is complete.").to_owned() } else { errors::notice(reason, published) };
                if let Some(record) = self.conversations.get_mut(&photo).and_then(|c| c.records.iter_mut().find(|r| r["backend_round_id"] == round)) {
                    record["local_terminal"] = json!(state);
                    record["backend_terminal"] = json!(payload);
                    record["trace_id"] = data.get("trace_id").cloned().unwrap_or(Value::Null);
                    if let Some(confirmed) = payload.get("last_confirmed_plan_id").and_then(Value::as_str) {
                        record["backend_reported_last_confirmed_plan_id"] = json!(confirmed);
                    }
                }
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
        if let Some(record) = self
            .conversations
            .get_mut(&photo)
            .and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == job.identity.local_round_id.as_str()))
        {
            if record.get("tool_requests").is_none() {
                record["tool_requests"] = json!([]);
            }
            if let Some(requests) = record.get_mut("tool_requests").and_then(Value::as_array_mut) {
                requests.push(json!({"tool_request_id":request,"tool_call_id":payload.get("tool_call_id"),"tool_name":payload.get("tool_name"),"args":payload.get("args"),"expected_revision":payload.get("expected_revision"),"outcome":"not_executed"}));
            }
        }
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
        let association = self
            .conversations
            .get(&photo)
            .and_then(|c| {
                c.messages.iter().rev().find(|m| {
                    m.get("tool_calls")
                        .and_then(Value::as_array)
                        .is_some_and(|calls| calls.iter().any(|call| call.get("id") == payload.get("tool_call_id")))
                })
            })
            .cloned();
        let stage = association.as_ref().and_then(|m| m.get("stage")).cloned();
        let model_call = association.as_ref().and_then(|m| m.get("model_call_id")).cloned();
        let id = payload.get("tool_call_id").and_then(Value::as_str).ok_or("AI 工具关联缺失。")?;
        self.conversations
            .get_mut(&photo)
            .ok_or("AI 会话不存在。")?
            .messages
            .push(json!({"role":"tool","text":result["text"],"images":result["images"],"tool_call_id":id,"local_round_id":local_round_id,"tool_request_id":request,"stage":stage,"model_call_id":model_call,"complete_state":"complete","local_message_id":format!("{}-tool-{request}",local_round_id.as_str())}));
        if let Some(record) =
            self.conversations.get_mut(&photo).and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == local_round_id.as_str()))
            && let Some(fact) = record
                .get_mut("tool_requests")
                .and_then(Value::as_array_mut)
                .and_then(|requests| requests.iter_mut().find(|r| r["tool_request_id"] == request))
        {
            fact["outcome"] = json!("succeeded");
            fact["result_message_id"] = json!(format!("{}-tool-{request}", local_round_id.as_str()));
        }
        self.save(app, photo)?;
        self.send(
            format!("/v1/rounds/{round}/tool-results"),
            json!({"tool_request_id":request,"photo_revision":revision,"outcome":"succeeded","result":result}),
            0,
            round,
        )
    }
    fn prepared(&mut self, app: &mut LightcraftApp, ctx: &egui::Context, message: WorkerMessage) -> Result<(), String> {
        let message = match message {
            WorkerMessage::HistoryPrepared { identity, result } => {
                let photo = identity.scope.photo;
                let Some(job) = self.jobs.get_mut(&photo) else { return Ok(()) };
                if job.identity != identity
                    || identity.scope.library_generation != app.session.library_generation()
                    || identity.connection_generation != self.connection_generation
                    || job.cancel.load(Ordering::Relaxed)
                    || !self.consent
                {
                    return Ok(());
                }
                if !current(app, &job.checkpoint) {
                    return Err(tr("AI result is stale; the photo was not changed.").into());
                }
                job.history = result?;
                let revision = job.checkpoint.revision;
                let cancel = job.cancel.clone();
                self.enqueue("/v1/rounds".into(), json!({"submission_seq":identity.submission_seq,"photo_ref":{"library_generation":identity.scope.library_generation,"photo_id":photo.0},"photo_revision":revision}), identity.submission_seq, String::new(), identity,cancel)?;
                if let Some(job) = self.jobs.get_mut(&photo) {
                    job.history_preflight = false;
                }
                return Ok(());
            }
            message => message,
        };
        let WorkerMessage::Prepared { identity, plan, settings, result } = message else { return Ok(()) };
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
                Arc::<Snapshot>::from(s)
            }
            Err(error) => {
                if let Some(plan) = &plan {
                    let round = job.round.clone();
                    let revision = job.checkpoint.revision;
                    let actual = job.checkpoint.expected.clone();
                    let conversation = self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
                    if let Some(step) = conversation
                        .records
                        .iter_mut()
                        .find(|r| r["local_round_id"] == identity.local_round_id.as_str())
                        .and_then(|r| r.get_mut("stages"))
                        .and_then(Value::as_array_mut)
                        .and_then(|s| s.iter_mut().find(|s| s["plan_id"] == plan["plan_id"]))
                    {
                        step["execution_outcome"] = json!("failed");
                        step["render_outcome"] = json!("failed");
                        step["error"] = json!(error);
                        step["delivery_state"] = json!("not_sent");
                    }
                    conversation.messages.push(json!({"local_round_id":identity.local_round_id,"local_message_id":format!("{}-{}-failure",identity.local_round_id.as_str(),plan["plan_id"]),"complete_state":"complete","display_kind":"execution_feedback","role":"user","text":format!("Actual execution feedback: {}",json!({"outcome":"failed","reason_code":"render_failed","photo_revision":revision,"actual_state":actual}))}));
                    self.save(app, photo)?;
                    self.send(
                        format!("/v1/rounds/{round}/execution"),
                        json!({"plan_id":plan["plan_id"],"outcome":"failed","photo_revision":revision,"reason_code":"render_failed"}),
                        0,
                        round,
                    )?;
                    if let Some(step) = self
                        .conversations
                        .get_mut(&photo)
                        .and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == identity.local_round_id.as_str()))
                        .and_then(|r| r.get_mut("stages"))
                        .and_then(Value::as_array_mut)
                        .and_then(|s| s.iter_mut().find(|s| s["plan_id"] == plan["plan_id"]))
                    {
                        step["delivery_state"] = json!("queued");
                    }
                }
                let published = self
                    .conversations
                    .get(&photo)
                    .is_some_and(|c| c.records.iter().any(|r| r["local_round_id"] == identity.local_round_id.as_str() && errors::published(r)));
                self.finish(photo, &errors::notice("execution_failed", published));
                return self.save(app, photo);
            }
        };
        let round = job.round.clone();
        let revision = job.checkpoint.revision;
        if let Some(plan) = plan {
            let rendered = snapshot.result.rendered.as_ref().map_err(Clone::clone)?;
            let op = app.session.develop_op(photo, (*settings).clone(), "AI edit plan").ok_or("照片不存在。")?;
            app.session
                .commit_companion(&job.lease, &identity.local_round_id, job.checkpoint.revision, op)
                .map_err(|e| tr(&e.to_string()).to_owned())?;
            app.session.accept(&snapshot.result);
            app.renderer.publish_companion(ctx, photo, snapshot.result.key, rendered, app.session.active() == Some(photo));
            let new_revision = app.session.photo_revision(photo);
            let feedback = json!({"plan_id":plan["plan_id"],"outcome":"succeeded","photo_revision":new_revision,"actual_state":settings,"preview":snapshot.preview});
            let job = self.jobs.get_mut(&photo).ok_or("AI 轮次已结束。")?;
            let before = job.checkpoint.expected.clone();
            let stage = if job.plans.is_empty() { "initial" } else { "correction" };
            job.checkpoint.expected = settings.clone();
            job.checkpoint.revision = new_revision;
            let plan_id = plan.get("plan_id").and_then(Value::as_str).ok_or("AI 方案编号缺失。")?.to_owned();
            job.plans.insert(plan_id.clone(), feedback.clone());
            job.snapshot = Some(snapshot.clone());
            job.inflight_plan = None;
            let checkpoint = job.checkpoint.clone();
            let conversation = self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
            conversation.checkpoint = Some(checkpoint.clone());
            if let Some(record) = conversation.records.iter_mut().find(|r| r["local_round_id"] == identity.local_round_id.as_str()) {
                record["recent_complete_result"] = json!(settings);
                record["recent_complete_hash"] = json!(settings.hash64());
                if let Some(stages) = record.get_mut("stages").and_then(Value::as_array_mut)
                    && let Some(step) = stages.iter_mut().find(|s| s["plan_id"] == plan_id)
                {
                    step["before_state"] = json!(before);
                    step["applied_state"] = json!(settings);
                    let (display_before, display_actual) = card::capture_pair(&before, &settings, &snapshot.context.info);
                    if step.get("display_before").is_none() {
                        step["display_before"] = display_before;
                    }
                    step["display_actual"] = display_actual;
                    step["effective_control_values"] = lightcraft_engine::companion::control_list(&settings, &snapshot.context.info);
                    step["execution_outcome"] = json!("succeeded");
                    step["delivery_state"] = json!("not_sent");
                    step["render_outcome"] = json!("succeeded");
                    step["locally_published"] = json!(true);
                    step["preview_message_id"] = json!(format!("{}-{stage}", identity.local_round_id.as_str()));
                }
            }
            conversation.messages.push(json!({"local_round_id":identity.local_round_id,"local_message_id":format!("{}-{stage}",identity.local_round_id.as_str()),"stage":stage,"complete_state":"complete","display_kind":"execution_feedback","role":"user","text":format!("Actual execution feedback: {}",json!({"outcome":"succeeded","actual_state":settings,"photo_revision":new_revision})),"images":[{"mime_type":"image/jpeg","data":snapshot.preview["data_base64"]}]}));
            conversation.rounds.push(
                json!({"local_round_id":identity.local_round_id,"round_id":round,"plan_id":plan_id,"original":checkpoint.original,"actual_state":settings,"operations":plan["operations"]}),
            );
            // A failed POST retains the actual visible result; never roll back on network failure.
            self.save(app, photo)?;
            self.send(format!("/v1/rounds/{round}/execution"), feedback, 0, round)?;
            if let Some(step) = self
                .conversations
                .get_mut(&photo)
                .and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == identity.local_round_id.as_str()))
                .and_then(|r| r.get_mut("stages"))
                .and_then(Value::as_array_mut)
                .and_then(|s| s.iter_mut().find(|s| s["plan_id"] == plan_id))
            {
                step["delivery_state"] = json!("queued");
            }
            self.save(app, photo)?;
        } else {
            let job = self.jobs.get_mut(&photo).ok_or("AI 轮次已结束。")?;
            job.snapshot = Some(snapshot.clone());
            let input = json!({"input_id":format!("input-{seq}"),"photo_revision":revision,"prompt":job.prompt,"current_state":settings,"preview":snapshot.preview,"history":job.history,"capabilities":capabilities(&settings)});
            let message =
                self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?.messages.get_mut(job.user_message).ok_or("AI 用户请求缺失。")?;
            *message = json!({"local_round_id":identity.local_round_id,"local_message_id":format!("{}-user",identity.local_round_id.as_str()),"complete_state":"complete","display_kind":"request","role":"user","authored_text_bytes":job.prompt.len(),"text":format!("{}\nActual photo state: {}",job.prompt,settings.to_json()),"images":[{"mime_type":"image/jpeg","data":snapshot.preview["data_base64"]}]});
            if let Some(record) = self
                .conversations
                .get_mut(&photo)
                .and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == identity.local_round_id.as_str()))
            {
                record["original"] = json!(settings);
                record["before_preview_message_id"] = json!(format!("{}-user", identity.local_round_id.as_str()));
                record["input_delivery_state"] = json!("not_sent");
                record["input_id"] = json!(format!("input-{seq}"));
            }
            self.save(app, photo)?;
            self.send(format!("/v1/rounds/{round}/input"), input, 0, round)?;
            if let Some(record) = self
                .conversations
                .get_mut(&photo)
                .and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == identity.local_round_id.as_str()))
            {
                record["input_delivery_state"] = json!("queued");
            }
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
            self.retire_connection();
            self.paging.cancel();
            self.pending_confirmations.clear();
            self.confirmation_retry_at = None;
            self.conversations.clear();
            self.editors.clear();
            self.dirty_editors.clear();
            self.editor_photo = None;
            self.prompt.clear();
            self.completed.clear();
            self.pending_local_saves.clear();
            self.save_retry_feedback.clear();
            self.local_save_retry_at = None;
            self.local_save_after_frame = None;
            self.feedback_pending = false;
            self.readonly_selection = None;
            self.readonly_task = None;
            self.task = None;
            self.telemetry = network::Telemetry::default();
            self.health = health::Health::default();
            self.generation = generation;
        }
        self.sync_task_selection(app);
        self.health.poll();
        if self.health.checking() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if self.feedback_pending {
            self.defer_local_saves_for_feedback(ctx);
        }
        // Egui may rerun a discarded pass; receipts and record retries must wait
        // until the first feedback frame has completed.
        if self.feedback_needs_paint(ctx) {
            ctx.request_repaint();
            return;
        }
        if let Err(error) = self.sync_editor(app).and_then(|()| self.stage_editors(app)) {
            self.text = error;
        }
        self.workers.retain(|_, alive| alive.load(Ordering::Relaxed));
        self.retiring_migrations.retain(|_, receipt| !receipt.retired());
        self.poll_pages(app);
        self.poll_interruption(app, ctx);
        comparison::poll(self, app, ctx);
        for (photo, c) in &mut self.conversations {
            if c.storage_notice && !app.session.companion_history_unsaved(*photo) && !self.pending_local_saves.contains(photo) {
                c.notice = tr("AI history saved. Photo changes are kept.").into();
                c.storage_notice = false;
            }
            if c.schema_version == 2 && c.dirty && c.dirty_queued && !app.session.companion_tail_unsaved(*photo) {
                c.dirty = false;
                c.dirty_queued = false;
            }
        }
        if self.paging.busy() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if self.open
            && let Some(photo) = self.displayed_photo(app)
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
                    if identity.scope.library_generation != app.session.library_generation() {
                        continue;
                    }
                    if path.ends_with("/execution")
                        && let Ok(response) = &result
                        && response.get("accepted").and_then(Value::as_bool) == Some(true)
                        && let Some(plan_id) = response.get("_client_execution_plan_id").and_then(Value::as_str)
                    {
                        if let Some(record) = self.conversations.get_mut(&identity.scope.photo).and_then(|c| {
                            c.records.iter_mut().find(|r| {
                                r["local_round_id"] == identity.local_round_id.as_str()
                                    && r["submission_seq"] == identity.submission_seq
                                    && r["connection_generation"] == identity.connection_generation
                                    && r["runtime_library_generation"] == identity.scope.library_generation
                            })
                        }) && let Some(step) =
                            record.get_mut("stages").and_then(Value::as_array_mut).and_then(|s| s.iter_mut().find(|s| s["plan_id"] == plan_id))
                        {
                            step["delivery_state"] = json!("confirmed");
                            if step["execution_outcome"] == "succeeded" {
                                record["backend_confirmed_plan"] = json!(plan_id);
                            }
                        } else if !self.pending_confirmations.iter().any(|(owner, plan)| owner == &identity && plan.as_deref() == Some(plan_id)) {
                            if self.pending_confirmations.len() < 128 && plan_id.len() <= 512 {
                                self.pending_confirmations.push((identity.clone(), Some(plan_id.to_owned())));
                            } else {
                                self.text = tr("AI history is waiting to be saved. Retry after saving completes.").into();
                                self.retire_connection();
                            }
                        }
                        let _ = self.save(app, identity.scope.photo);
                    }
                    if path.ends_with("/input")
                        && let Ok(response) = &result
                        && response.get("accepted").and_then(Value::as_bool) == Some(true)
                    {
                        if let Some(record) = self.conversations.get_mut(&identity.scope.photo).and_then(|c| {
                            c.records.iter_mut().find(|r| {
                                r["local_round_id"] == identity.local_round_id.as_str()
                                    && r["submission_seq"] == identity.submission_seq
                                    && r["connection_generation"] == identity.connection_generation
                                    && r["runtime_library_generation"] == identity.scope.library_generation
                            })
                        }) {
                            record["input_delivery_state"] = json!("confirmed");
                        } else if !self.pending_confirmations.iter().any(|(owner, plan)| owner == &identity && plan.is_none())
                            && self.pending_confirmations.len() < 128
                        {
                            self.pending_confirmations.push((identity.clone(), None));
                        }
                        let _ = self.save(app, identity.scope.photo);
                    }
                    if identity.connection_generation != self.connection_generation {
                        continue;
                    }
                    if self.jobs.get(&identity.scope.photo).is_none_or(|j| j.identity != identity) {
                        // Stop removes the local job before its POST completes. A
                        // failed delivery still needs the original disconnect
                        // fallback to cancel the task on this same connection.
                        if path.ends_with("/stop") && result.is_err() {
                            self.close_connection();
                        }
                        continue;
                    }
                    if let Err(error) = result {
                        if submission != 0 && error.cancelled_before_send {
                            self.finish(identity.scope.photo, tr("Stopped generating. Keeping the latest complete result."));
                            let _ = self.save(app, identity.scope.photo);
                            continue;
                        }
                        let photo = identity.scope.photo;
                        let record = self
                            .conversations
                            .get(&photo)
                            .and_then(|c| c.records.iter().find(|r| r["local_round_id"] == identity.local_round_id.as_str()));
                        let notice = errors::notice(error.code.as_deref().unwrap_or("network_failure"), record.is_some_and(errors::published));
                        self.mark_failure(photo, &notice);
                        if let Some(record) = self
                            .conversations
                            .get_mut(&photo)
                            .and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == identity.local_round_id.as_str()))
                        {
                            record["diagnostic_reason_code"] = json!(error.code);
                            record["trace_id"] = json!(error.trace);
                        }

                        if path.ends_with("/stop") || (submission != 0 && !error.rejected) {
                            // A failed creation POST may have been accepted. Disconnect
                            // rather than abandon an unidentifiable server task.
                            self.close_connection();
                            if submission != 0 && !error.rejected {
                                if let Some(job) = self.jobs.get_mut(&identity.scope.photo) {
                                    job.stopped = true;
                                    job.cancel.store(true, Ordering::Relaxed);
                                }
                                if let Some(c) = self.conversations.get_mut(&identity.scope.photo) {
                                    c.pending.clear();
                                    c.notice = tr("Cancelling… Waiting for the AI connection to close safely.").into();
                                }
                                let _ = self.save(app, identity.scope.photo);
                                continue;
                            }
                        }
                        let photo = identity.scope.photo;
                        if !round.is_empty() {
                            let _ = self.stop(photo);
                        }
                        self.finish(photo, &notice);
                        if let Some(conversation) = self.conversations.get_mut(&photo) {
                            conversation.notice = notice;
                        }
                        let _ = self.save(app, photo);
                    } else if path == "/v1/rounds" {
                        // SSE owns admission and uploads; a receipt alone cannot trigger work.
                    }
                    Ok(())
                }
                network::Event::Heartbeat { sequence } => {
                    self.telemetry.heartbeat(sequence);
                    Ok(())
                }
                network::Event::Closed(error) => {
                    let photos: Vec<_> = self.jobs.keys().copied().collect();
                    for photo in photos {
                        self.disconnected(photo, &error);
                        let _ = self.save(app, photo);
                    }
                    self.text = tr("The AI connection was interrupted. Check the local service, then start a new request.").into();
                    Ok(())
                }
            };
            if let Err(error) = result {
                self.text = error;
            }
        }
        while self.completed.len() < 8
            && let Ok(message) = self.rx.try_recv()
        {
            let identity = match &message {
                WorkerMessage::Prepared { identity, .. } | WorkerMessage::HistoryPrepared { identity, .. } => identity,
            };
            if self.jobs.get(&identity.scope.photo).is_some_and(|j| j.identity == *identity) {
                self.completed.push_back(message);
            }
        }
        for _ in 0..self.completed.len() {
            let Some(message) = self.completed.pop_front() else { break };
            let identity = match &message {
                WorkerMessage::Prepared { identity, .. } | WorkerMessage::HistoryPrepared { identity, .. } => identity,
            };
            let photo = identity.scope.photo;
            if self.jobs.get(&photo).is_none_or(|j| j.identity != *identity) {
                continue;
            }
            if self.worker_busy(photo) {
                // Completion transfer is not producer retirement. Publish neither
                // parameters nor HTTP feedback until the exact producer retires.
                self.completed.push_back(message);
                ctx.request_repaint_after(Duration::from_millis(5));
                continue;
            }
            if let Err(error) = self.prepared(app, ctx, message) {
                self.fail_one(app, photo, &error);
            }
        }
        // A history worker can fail to publish into a full/disconnected channel.
        // Its irreversible guard then marks a known local-only failure: no create
        // has been enqueued, so retiring it must release the photo lease.
        let lost_preflights: Vec<_> = self
            .jobs
            .iter()
            .filter_map(|(photo, job)| (job.history_preflight && job.cancel.load(Ordering::Relaxed) && !self.worker_busy(*photo)).then_some(*photo))
            .collect();
        for photo in lost_preflights {
            self.fail_one(app, photo, tr("AI history preparation failed. Try again."));
        }
        if self.connection.as_ref().is_none_or(|c| !c.alive.load(Ordering::Relaxed)) && !self.jobs.is_empty() {
            let photos: Vec<_> = self.jobs.keys().copied().collect();
            for photo in photos {
                self.disconnected(photo, "AI 事件连接已断开。");
                let _ = self.save(app, photo);
            }
        }
        if self.confirmation_retry_at.is_none_or(|at| at <= Instant::now()) {
            let pending = std::mem::take(&mut self.pending_confirmations);
            for (identity, plan) in pending {
                if identity.scope.library_generation != app.session.library_generation() {
                    continue;
                }
                if let Err(error) = match plan.as_deref() {
                    Some(plan) => app.session.confirm_companion_execution(&identity, plan),
                    None => app.session.confirm_companion_input(&identity),
                } {
                    self.pending_confirmations.push((identity.clone(), plan));
                    if let Some(c) = self.conversations.get_mut(&identity.scope.photo) {
                        c.notice = error.to_string();
                    }
                }
            }
            self.confirmation_retry_at =
                if self.pending_confirmations.is_empty() { None } else { Instant::now().checked_add(Duration::from_secs(1)) };
        }
        if !self.pending_confirmations.is_empty() {
            ctx.request_repaint_after(Duration::from_secs(1));
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
        if !self.pending_local_saves.is_empty()
            && self.local_save_after_frame.is_none_or(|frame| ctx.cumulative_frame_nr() > frame)
            && self.local_save_retry_at.is_none_or(|at| at <= Instant::now())
        {
            match self.flush_local_saves(app) {
                Ok(()) => {
                    self.local_save_retry_at = None;
                    self.local_save_after_frame = None;
                    self.feedback_pending = false;
                }
                Err(error) => {
                    self.text = error;
                    self.local_save_retry_at = Instant::now().checked_add(Duration::from_secs(1));
                }
            }
        }
        if !self.pending_local_saves.is_empty() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if app.session.companion_migration_pending() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if app.session.companion_cleanup_pending() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if !self.jobs.is_empty() || !self.workers.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    /// Cancelling transport/work is a cleanup action, not the cause of failure.
    fn mark_failure(&mut self, photo: PhotoId, error: &str) {
        let Some(job) = self.jobs.get(&photo).filter(|j| !j.stopped) else { return };
        if let Some(record) = self
            .conversations
            .get_mut(&photo)
            .and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == job.identity.local_round_id.as_str()))
        {
            if record.get("local_terminal").is_none() {
                record["local_terminal"] = json!("failed");
            }
            record["local_failure"] = json!({"message":error,"reason":"client_failure"});
        }
    }
    fn fail_one(&mut self, app: &mut LightcraftApp, photo: PhotoId, error: &str) {
        let error = tr(error);
        let published = self.jobs.get(&photo).is_some_and(|job| {
            self.conversations
                .get(&photo)
                .is_some_and(|c| c.records.iter().any(|r| r["local_round_id"] == job.identity.local_round_id.as_str() && errors::published(r)))
        });
        let notice = errors::local_notice(error, published);
        self.mark_failure(photo, &notice);
        if let Some(job) = self.jobs.get(&photo)
            && let Some(record) = self
                .conversations
                .get_mut(&photo)
                .and_then(|c| c.records.iter_mut().find(|r| r["local_round_id"] == job.identity.local_round_id.as_str()))
        {
            record["local_failure"]["diagnostic"] = json!(error.chars().take(4096).collect::<String>());
        }
        let _ = self.stop(photo);
        if let Some(c) = self.conversations.get_mut(&photo) {
            c.storage_notice = app.session.companion_history_unsaved(photo);
            c.notice = if c.storage_notice { tr("Photo changes are kept. AI history is not saved; saving will retry.").into() } else { notice };
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
    let (data, width, height) = encode_preview(&rendered.image)?;
    if cancel.load(Ordering::Relaxed) {
        return Err("已停止生成。".into());
    }
    let preview =
        json!({"mime_type":"image/jpeg","width":width,"height":height,"frame_id":context.frame_id,"data_base64":Base64::encode_string(&data)});
    Ok(Snapshot { result, preview, context })
}
fn encode_preview(image: &lightcraft_raster::Rgba8) -> Result<(Vec<u8>, usize, usize), String> {
    let options = lightcraft_engine::export::ExportOptions {
        format: lightcraft_engine::export::ExportFormat::Jpeg,
        quality: 75,
        color_space: lightcraft_engine::export::OutputSpace::Srgb,
        metadata: lightcraft_engine::export::MetadataPolicy::None,
        ..Default::default()
    };
    let linear = image.to_linear();
    let mut preview = lightcraft_raster::resample::fit(&linear, 640, 640, lightcraft_raster::resample::Filter::Bilinear).to_srgb8();
    loop {
        let bytes = lightcraft_engine::export::encode_image(&preview, &options)?;
        if bytes.len() <= 128 * 1024 {
            return Ok((bytes, preview.width, preview.height));
        }
        let edge = preview.width.max(preview.height);
        if edge <= 1 {
            return Err("压缩预览超过服务限制。".into());
        }
        let smaller = edge.saturating_mul(3) / 4;
        preview = lightcraft_raster::resample::fit(&linear, smaller.max(1), smaller.max(1), lightcraft_raster::resample::Filter::Bilinear).to_srgb8();
    }
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

fn close_pending_tools(messages: &mut Vec<Value>, outcome: &str) {
    let mut pending: Vec<(String, Value)> = Vec::new();
    for message in messages.iter() {
        if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    // Preserve original call order; malformed legacy associations are
                    // never repaired by guessing, and fail the projection validator.
                    pending.push((id.to_owned(), message.clone()));
                }
            }
        }
        if let Some(id) = message.get("tool_call_id").and_then(Value::as_str) {
            pending.retain(|(pending_id, _)| pending_id != id);
        }
    }
    for (id, owner) in pending {
        messages.push(json!({"role":"tool","tool_call_id":id,"text":json!({"outcome":if outcome=="cancelled" {"cancelled"} else {"failed"},"executed":false,"reason_code":"ended_before_tool_execution"}).to_string(),"local_round_id":owner.get("local_round_id"),"stage":owner.get("stage"),"model_call_id":owner.get("model_call_id"),"complete_state":"complete","display_kind":"tool_result"}));
    }
}

pub(crate) fn selection_intent(app: &mut LightcraftApp) {
    app.ai.leave_task_view();
}
pub fn show_task_entry(app: &mut LightcraftApp, ui: &mut egui::Ui) {
    task_entry::show(app, ui);
}
pub fn show_comparison(app: &mut LightcraftApp, ui: &mut egui::Ui) -> bool {
    comparison::show(app, ui)
}
pub(crate) fn cpu_textures(state: &State, textures: &mut std::collections::HashMap<egui::TextureId, crate::softpaint::CpuTexture>) {
    comparison::cpu_textures(state, textures);
    state.attachments.cpu_textures(textures);
}

pub fn poll(app: &mut LightcraftApp, ctx: &egui::Context) {
    let mut state = std::mem::take(&mut app.ai);
    state.poll_inner(app, ctx);
    if state.open
        && !state.feedback_needs_paint(ctx)
        && let Some(photo) = state.displayed_photo(app)
        && let Err(e) = state.load(app, photo)
    {
        state.text = e;
    }
    if !state.feedback_needs_paint(ctx) {
        let mut attachments = std::mem::take(&mut state.attachments);
        // The captured request needs caller-owned photo headers, temporarily available here.
        app.ai = state;
        attachments.poll(app, ctx);
        state = std::mem::take(&mut app.ai);
        state.attachments = attachments;
    }
    app.ai = state;
}
/// Irreversibly cancel old publishers without storage I/O. Safe to repeat even
/// after a failed save or a confirmed unsaved exit.
pub fn cancel_for_library_change(app: &mut LightcraftApp) {
    let state = &mut app.ai;
    state.comparison = None;
    let photos: Vec<_> = state.jobs.keys().copied().collect();
    // Revoke every round before any stop control, draft staging or storage can fail.
    for job in state.jobs.values() {
        job.cancel.store(true, Ordering::Relaxed);
    }
    state.pending_local_saves.extend(photos.iter().copied());
    state.pending_local_saves.extend(state.conversations.iter().filter_map(|(photo, c)| c.dirty.then_some(*photo)));
    for photo in photos {
        let _ = state.stop(photo);
        state.finish(photo, "图库已关闭，保留最近一次完整结果。");
    }
    state.paging.cancel();
    // Closing the transport also cancels unsent content for already-completed
    // rounds whose actual effects must remain intact.
    state.close_connection();
}
/// A host or user has committed to exiting; consent/connection changes cannot
/// authorize content again. Cancellation itself performs no storage I/O.
pub fn before_exit(app: &mut LightcraftApp) {
    app.ai.exiting = true;
    cancel_for_library_change(app);
    if let Some(receipt) = app.session.cancel_companion_migration() {
        // One actual producer per live generation. Repeated host/user exit
        // callbacks replace the same receipt without accumulating duplicates.
        app.ai.retiring_migrations.insert(app.session.library_generation(), receipt);
    }
}
pub fn before_library_change(app: &mut LightcraftApp) -> Result<(), String> {
    cancel_for_library_change(app);
    let mut state = std::mem::take(&mut app.ai);
    let result = (|| {
        state.stage_editors(app)?;
        state.flush_local_saves(app)?;
        state.stage_editors(app)?;
        state.flush_local_saves(app)?;
        app.session.persist().map_err(|e| e.to_string())?;
        let pending = std::mem::take(&mut state.pending_confirmations);
        let mut receipt_failure = None;
        for (identity, plan) in pending {
            if let Err(error) = match plan.as_deref() {
                Some(plan) => app.session.confirm_companion_execution(&identity, plan),
                None => app.session.confirm_companion_input(&identity),
            } {
                state.pending_confirmations.push((identity, plan));
                receipt_failure.get_or_insert(error.to_string());
            }
        }
        if let Some(error) = receipt_failure {
            return Err(error);
        }
        state.paging.cancel();
        state.attachments.cancel();
        state.retire_connection();
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
        state.retire_connection();
        state.paging.cancel();
        state.attachments.cancel();
        state.bottom_reading.clear();
        state.pending_confirmations.clear();
        state.confirmation_retry_at = None;
        state.conversations.clear();
        state.editors.clear();
        state.dirty_editors.clear();
        state.editor_photo = None;
        state.prompt.clear();
        state.completed.clear();
        state.pending_local_saves.clear();
        state.save_retry_feedback.clear();
        state.local_save_retry_at = None;
        state.local_save_after_frame = None;
        state.feedback_pending = false;
        state.readonly_selection = None;
        state.readonly_task = None;
        state.task = None;
        state.telemetry = network::Telemetry::default();
        state.health = health::Health::default();
        state.generation = app.session.library_generation();
    }
    state.sync_task_selection(app);
    let photo = params.get("photo").and_then(Value::as_u64).map(PhotoId).or_else(|| app.session.active());
    let result = match id {
        "ai.undo" if state.readonly_task.is_some() => Err(tr("Return to the selected photo before changing its AI conversation or edits.").into()),
        "ai.open" => {
            state.open = true;
            state.leave_task_view();
            state.health.first_open(network::service_address(app.services.companion_address.as_deref()));
            app.session.end_interaction().map_err(|e| e.to_string()).and_then(|()| {
                app.ui.right = crate::state::RightPanel::Ai;
                app.ui.tool.clear();
                crate::docking::reveal_command(app, "ai.open")?;
                Ok(Value::Null)
            })
        }
        "ai.task" => task_entry::navigate(&mut state, app),
        "ai.compare" => comparison::enter(&mut state, app, params),
        "ai.returnCurrent" => {
            state.comparison = None;
            task_entry::return_current(&mut state, app)
        }
        "ai.health" => {
            state.health.request(network::service_address(app.services.companion_address.as_deref()));
            Ok(state.health.status())
        }
        "ai.start" => {
            if let Err(error) = state.sync_editor(app) {
                app.ai = state;
                return Some(Err(error));
            }
            let explicit = params
                .get("photo")
                .map(|v| v.as_u64().map(PhotoId).ok_or_else(|| tr("The requested photo must match the single selected photo.").to_owned()))
                .transpose();
            explicit.and_then(|p| state.start_with_prompt(app, p, params.get("prompt").and_then(Value::as_str)))
        }
        "ai.stop" => photo.ok_or_else(|| "请先选择照片。".into()).and_then(|p| {
            state.pending_local_saves.insert(p);
            state.request_feedback();
            app.session.defer_companion_retry(true);
            state.stop(p)?;
            Ok(Value::Null)
        }),
        "ai.retrySave" => (|| {
            if app.session.library.is_none()
                || params.get("library_generation").is_some_and(|value| value.as_u64() != Some(app.session.library_generation()))
            {
                return Err(tr("This save request belongs to another or closed library.").into());
            }
            if state.readonly_task.is_some_and(|scope| scope.library_generation != app.session.library_generation()) {
                return Err(tr("This AI task belongs to a closed library.").into());
            }
            let viewed_photo = state.displayed_photo(app);
            if params.get("photo").is_some_and(|value| value.as_u64().map(PhotoId) != viewed_photo) {
                return Err(tr("Choose the displayed photo to retry saving.").into());
            }
            // Storage-only recovery. Do not poll jobs, reconnect, confirm a
            // backend receipt, end an interaction, or replay a rejected edit.
            state.stage_editors(app)?;
            if let Some(photo) = viewed_photo
                && let Some(epoch) = app.session.pending_companion_clear_epoch(photo).map_err(|e| e.to_string())?
            {
                clear(
                    &mut state,
                    app,
                    photo,
                    &json!({"confirm":true,"photo":photo.0,"library_generation":app.session.library_generation(),"expected_epoch":epoch}),
                )?;
            }
            state.flush_local_saves(app)?;
            app.session.persist().map_err(|e| tr(&e.to_string()).to_owned())?;
            if viewed_photo.is_some_and(|photo| app.session.companion_photo_unsaved(photo) || state.pending_local_saves.contains(&photo)) {
                return Err(tr("Saving is still pending. Release the current gesture, then retry saving.").into());
            }
            state.local_save_retry_at = None;
            if let Some(photo) = viewed_photo {
                let conversation = state.conversations.entry(photo).or_default();
                conversation.notice = tr("Saved. Retry the edit explicitly when ready.").into();
                conversation.storage_notice = false;
                state.save_retry_feedback.remove(&photo);
            }
            Ok(json!({"saved":true}))
        })(),
        "ai.status" => {
            let viewed_photo = if params.get("photo").is_some() { photo } else { state.displayed_photo(app) };
            let root_protected = app.session.companion_archive_protected_reason().is_some();
            if !root_protected
                && let Some(photo) = viewed_photo
                && let Err(error) = state.load(app, photo)
            {
                let conversation = state.conversations.entry(photo).or_insert_with(|| Conversation {
                    scope: Some(PhotoScope { photo, library_generation: app.session.library_generation() }),
                    ..Default::default()
                });
                conversation.history_error.get_or_insert(HistoryError::Data(error));
            }
            let conversation = viewed_photo.and_then(|p| state.conversations.get(&p));
            let history_error = if root_protected {
                Some(tr(ROOT_RECOVERY_GUIDANCE))
            } else {
                conversation.and_then(|conversation| conversation.history_error.as_ref()).map(|error| tr(error.guidance()))
            };
            Ok(json!({
                "readonlyPhoto": state.readonly_task.map(|scope| scope.photo.0),
                "viewedPhoto": viewed_photo.map(|p| p.0),
                "task": state.task.as_ref().map(task_entry::Task::status),
                "comparison": state.comparison.as_ref().map(comparison::Comparison::status),
                "comparisonWorkerBusy": state.comparison_workers.iter().any(|alive| alive.load(Ordering::Relaxed)),
                "health": state.health.status(),
                "transport": state.telemetry.status(state.connection.as_ref().is_some_and(|c| c.alive.load(Ordering::Relaxed) && !c.is_closing())),
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
                "text": if root_protected { tr(ROOT_RECOVERY_GUIDANCE).to_owned() } else { conversation.map(display_text).unwrap_or_else(|| state.text.clone()) },
                "historyError": history_error,
                "resultCards": conversation.map(|c| c.view.iter().flat_map(|v| v.cards.iter()).cloned().chain(c.records.iter().filter_map(|r|card::from_record(r,None))).map(|card| present_card(app,&state,viewed_photo.unwrap_or(PhotoId(0)),card)).collect::<Vec<_>>()).unwrap_or_default(),
                "historyLoading": state.paging.busy() || viewed_photo.is_some_and(|photo| state.paging.awaiting_page(photo)) || conversation.and_then(|c| c.view.as_ref()).is_some_and(|v|v.card_cursor.is_some()),
                "migrationPending": app.session.companion_migration_pending(),
                "migrationBusy":app.session.companion_migration_busy(),
                "migrationError":app.session.companion_migration_error(),
                "historicalPreview":conversation.and_then(|c|c.view.as_ref()).and_then(|v|v.preview.as_ref()).map(historical::Preview::status),
                "legacyPlans": conversation.and_then(|c|c.view.as_ref()).and_then(|v|v.legacy_plan.as_ref()).into_iter().cloned().collect::<Vec<_>>(),
                "activePhotos": state.jobs.keys().map(|p| p.0).collect::<Vec<_>>(),
                "saveState": viewed_photo.map(|p| if app.session.companion_photo_unsaved(p) || state.pending_local_saves.contains(&p) || app.session.companion_migration_error().is_some() { "unsaved" } else if app.session.companion_migration_pending(){"migrating"}else { "saved" }),
                "cleanupPending": photo.is_some_and(|p| app.session.companion_photo_cleanup_pending(p)),
                "cleanupError": photo.and_then(|p| app.session.companion_cleanup_error(p)),
            }))
        }
        "ai.retryMigration" => {
            state.request_feedback();
            app.session.defer_companion_retry(true);
            app.session.retry_companion_migration().map(|done| json!({"complete":done})).map_err(|e| e.to_string())
        }
        "ai.historicalImage" => state.displayed_photo(app).ok_or_else(|| "请先选择照片。".into()).and_then(|photo| {
            state.load(app, photo)?;
            let conversation = state.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
            let total = conversation.view.as_ref().and_then(|v| v.preview.as_ref()).map(|p| p.total).unwrap_or(0);
            let index = params
                .get("index")
                .and_then(Value::as_u64)
                .filter(|index| *index < total)
                .ok_or_else(|| tr("Historical preview is unavailable.").to_owned())?;
            conversation.preview_cursor = index;
            let cursor = conversation.view.as_ref().map(|v| v.cursor).unwrap_or_default();
            state.attachments.cancel();
            state.request_page(app, photo, cursor)?;
            Ok(json!({"requested":true}))
        }),
        "ai.legacyPlan" => state.displayed_photo(app).ok_or_else(|| "请先选择照片。".into()).and_then(|photo| {
            state.load(app, photo)?;
            let conversation = state.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
            let header =
                conversation.prefix.as_ref().filter(|h| h.legacy_plan_count() > 0).ok_or_else(|| tr("Legacy plan is unavailable.").to_owned())?;
            let total = header.legacy_plan_count();
            let index = params
                .get("index")
                .and_then(Value::as_u64)
                .filter(|index| *index < total)
                .ok_or_else(|| tr("Legacy plan is unavailable.").to_owned())?;
            conversation.legacy_cursor = index;
            let cursor = conversation.view.as_ref().map(|v| v.cursor).unwrap_or_default();
            state.request_page(app, photo, cursor)?;
            Ok(json!({"requested":true}))
        }),
        "ai.bottom" => {
            let viewed = if params.get("photo").is_some() { photo } else { state.displayed_photo(app) };
            viewed.ok_or_else(|| tr("Select exactly one photo to start an AI task.").to_owned()).and_then(|photo| {
                state.load(app, photo)?;
                state.ensure_bottom(app, photo)?;
                Ok(json!({"following":true}))
            })
        }
        "ai.history" => {
            let viewed_photo = if params.get("photo").is_some() { photo } else { state.displayed_photo(app) };
            viewed_photo.ok_or_else(|| "请先选择照片。".into()).and_then(|p| {
                state.load(app, p)?;
                let record = params.get("record").and_then(Value::as_u64).unwrap_or(0);
                let offset = params.get("offset").and_then(Value::as_u64).unwrap_or(0);
                let cursor = lightcraft_engine::companion::storage::HistoryCursor { record, offset };
                state.navigate_page(app, p, cursor)?;
                state.request_page(app, p, cursor)?;
                Ok(json!({"requested":true}))
            })
        }
        "ai.undo" => params
            .get("local_round_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| tr("Choose the round to revert.").to_owned())
            .and_then(|round| {
                let target = params
                    .get("photo")
                    .map(|value| value.as_u64().map(PhotoId).ok_or_else(|| tr("Choose a valid photo to revert.").to_owned()))
                    .transpose()?
                    .or_else(|| app.session.active())
                    .ok_or_else(|| "请先选择照片。".to_owned())?;
                undo(&mut state, app, target, round)
            }),
        "ai.clear" => (|| {
            let target = params
                .get("photo")
                .map(|value| value.as_u64().map(PhotoId).ok_or_else(|| tr("Choose a valid photo to delete its conversation.").to_owned()))
                .transpose()?;
            if params.get("confirm").and_then(Value::as_bool) == Some(true) && target.is_none() {
                return Err(tr("Choose a valid photo to delete its conversation.").into());
            }
            let target =
                target.or_else(|| state.displayed_photo(app)).ok_or_else(|| tr("Select exactly one photo to start an AI task.").to_owned())?;
            if state.readonly_task.is_some_and(|scope| scope.photo != target) {
                return Err(tr("Choose the displayed photo to delete its conversation.").into());
            }
            clear(&mut state, app, target, params)
        })(),
        _ => {
            app.ai = state;
            return None;
        }
    };
    app.ai = state;
    Some(result)
}
fn present_card(app: &LightcraftApp, state: &State, photo: PhotoId, card: card::Card) -> card::Card {
    use lightcraft_engine::companion::RoundRevertState;
    let local: Option<LocalRoundId> = serde_json::from_value(json!(card.local_round_id)).ok();
    let outcome = local.as_ref().map(|round| app.session.companion_round_revert_state(photo, round)).unwrap_or(RoundRevertState::Unverified);
    let mut card = card.for_current(
        app.session.develop_of(photo).as_deref(),
        matches!(outcome, RoundRevertState::LaterEdits | RoundRevertState::RevertedWithLaterEdits),
    );
    card.reverted |= matches!(outcome, RoundRevertState::Reverted | RoundRevertState::RevertedWithLaterEdits);
    if outcome == RoundRevertState::Reverted || (card.reverted && outcome == RoundRevertState::Unverified) {
        card.current_photo_has_later_edits = false;
    }
    card.can_undo = state.readonly_task.is_none()
        && card.photo == Some(photo)
        && !state.jobs.contains_key(&photo)
        && !state.worker_busy(photo)
        && outcome == RoundRevertState::Eligible;
    card
}

fn undo(state: &mut State, app: &mut LightcraftApp, photo: PhotoId, round: &str) -> Result<Value, String> {
    if state.jobs.contains_key(&photo) || state.worker_busy(photo) {
        return Err("请先停止并等待此照片处理结束。".into());
    }
    state.load(app, photo)?;
    let local: LocalRoundId = serde_json::from_value(json!(round)).map_err(|_| "Choose the round to revert.".to_owned())?;
    let verified = app.session.verify_round_revert(photo, &local).map_err(|e| tr(&e.to_string()).to_owned())?;
    app.session.commit_verified_round_revert(verified).map_err(|e| tr(&e.to_string()).to_owned())?;
    let conversation = state.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
    if let Some(c) = conversation.checkpoint.as_mut() {
        c.eligible = false;
    }
    conversation.messages.push(json!({"role":"user","display_kind":"round_reverted","complete_state":"complete","local_round_id":round,"local_message_id":format!("{round}-reverted"),"text":"This round's edits were reverted. The photo was restored to its round starting state."}));
    conversation.notice = tr("This round was reverted. The conversation was kept.").into();
    if let Some(record) = conversation.records.iter_mut().find(|r| r["local_round_id"] == round) {
        record["revert_state"] = json!("reverted");
    }
    state.save(app, photo)?;
    Ok(json!({"undone":true}))
}
fn clear(state: &mut State, app: &mut LightcraftApp, photo: PhotoId, params: &Value) -> Result<Value, String> {
    if params.get("confirm").and_then(Value::as_bool) == Some(true)
        && params.get("library_generation").and_then(Value::as_u64) != Some(app.session.library_generation())
    {
        return Err(tr("This delete request belongs to another or closed library.").into());
    }
    if app.session.companion_archive_protected_reason().is_some() {
        return Err(tr("This conversation format is protected. Original history is retained.").into());
    }
    if state.conversations.get(&photo).and_then(|conversation| conversation.history_error.as_ref()).is_some_and(HistoryError::ownership_unknown) {
        return Err(tr("This photo's AI history cannot be verified. Original files are kept. Repair it from this library's verified backup, then reopen the library.").into());
    }
    if state.jobs.contains_key(&photo) || state.worker_busy(photo) {
        return Err(tr("Stop the AI task and wait for it to finish before deleting its conversation.").into());
    }
    if params.get("confirm").and_then(Value::as_bool) != Some(true) {
        let epoch = app.session.companion_conversation_epoch(photo).map_err(|e| e.to_string())?;
        app.ui.dialog = Some(crate::state::Dialog::ConfirmCompanionClear {
            photo: photo.0,
            library_generation: app.session.library_generation(),
            expected_epoch: epoch,
        });
        return Ok(json!({"confirmationRequired":true,"photo":photo.0,"libraryGeneration":app.session.library_generation(),"expectedEpoch":epoch}));
    }
    let expected_epoch =
        params.get("expected_epoch").and_then(Value::as_u64).ok_or_else(|| tr("Choose the conversation version to delete.").to_owned())?;
    state.stage_editors(app)?;
    let scope = PhotoScope { library_generation: app.session.library_generation(), photo };
    let receipt = match app.session.companion_clear(scope, expected_epoch) {
        Ok(receipt) => receipt,
        Err(_) => {
            let message = tr(if app.session.companion_history_unsaved(photo) {
                "Conversation deletion has not been saved. Check storage, then retry saving."
            } else {
                "This conversation could not be deleted. Original history is retained."
            })
            .to_owned();
            let conversation = state.conversations.entry(photo).or_default();
            conversation.notice = message.clone();
            conversation.storage_notice = app.session.companion_history_unsaved(photo);
            state.text = message.clone();
            return Err(message);
        }
    };
    state.paging.cancel();
    state.attachments.cancel();
    state.pending_local_saves.remove(&photo);
    state.pending_confirmations.retain(|(identity, _)| identity.scope != scope);
    state.save_retry_feedback.remove(&photo);
    state.bottom_reading.remove(&photo);
    state.conversations.insert(photo, Conversation { scope: Some(scope), ..Default::default() });
    if let Some(editor) = state.editors.get_mut(&photo) {
        editor.data.scroll_offset = 0.0;
        editor.data.follow_tail = true;
        editor.data.reading_cursor = None;
        editor.data.reading_epoch = Some(receipt.conversation_epoch);
        editor.data.reading_count = Some(0);
        editor.restore_scroll = true;
        // A removed photo's independently saved draft stays owned by the
        // archive. Its obsolete reading epoch cannot select the new context.
        if app.session.catalog.photo(photo).is_some() {
            app.session.set_companion_editor(photo, editor.data.clone()).map_err(|e| e.to_string())?;
        }
    }
    if state.task.as_ref().is_some_and(|task| task.identity.scope == scope) {
        state.task = None;
    }
    if state.comparison.as_ref().is_some_and(|view| view.belongs_to(scope)) {
        state.comparison = None;
    }
    if state.readonly_task == Some(scope) {
        state.leave_task_view();
    }
    state.conversations.entry(photo).or_default().notice =
        tr("The old conversation is cleared. Future requests start from the current photo.").into();
    if matches!(&app.ui.dialog, Some(crate::state::Dialog::ConfirmCompanionClear { photo: target, library_generation, expected_epoch: epoch }) if *target == photo.0 && *library_generation == scope.library_generation && *epoch == expected_epoch)
    {
        app.ui.dialog = None;
    }
    Ok(json!({"cleared":true}))
}
fn display_text(conversation: &Conversation) -> String {
    let mut text = String::new();
    if let Some(view) = &conversation.view {
        for item in &view.items {
            match *item {
                lightcraft_engine::companion::storage::HistoryItem::Unavailable { .. } => {}
                lightcraft_engine::companion::storage::HistoryItem::Record { index, .. } => {
                    if let Some(message) = view.messages.get(index) {
                        append_visible_message(&mut text, message);
                    }
                }
                lightcraft_engine::companion::storage::HistoryItem::Fragment { record, index, .. } => {
                    if let Some(fragment) = view.fragments.get(index)
                        && view.fragment_messages.get(&record).is_some_and(|message| {
                            matches!(message["role"].as_str(), Some("user" | "assistant" | "system")) && !reading::execution_feedback(message)
                        })
                    {
                        text.push_str(fragment);
                        text.push('\n');
                    }
                }
            }
        }
    }
    for message in &conversation.messages {
        append_visible_message(&mut text, message);
    }
    for value in conversation.pending.values() {
        text.push_str(value);
    }
    text.push_str(&conversation.notice);
    text
}
/// Shared Tools body; the host supplies its viewport, so only messages scroll.
fn append_visible_message(text: &mut String, message: &Value) {
    if matches!(message.get("role").and_then(Value::as_str), Some("user" | "assistant")) && !reading::execution_feedback(message) {
        let visible = reading::user_text(message);
        if !visible.is_empty() {
            text.push_str(if message["display_kind"] == "round_reverted" { tr(visible) } else { visible });
            text.push_str("\n\n");
        }
    }
}

pub fn show(app: &mut LightcraftApp, ui: &mut egui::Ui) {
    app.ai.open = true;
    if let Some(task) = app.ai.task.as_mut().filter(|t| {
        app.ai.comparison.is_none()
            && t.identity.scope.library_generation == app.session.library_generation()
            && Some(t.identity.scope.photo) == app.session.active()
    }) {
        task.unread = false;
    }
    app.ai.health.first_open(network::service_address(app.services.companion_address.as_deref()));
    let mut state = std::mem::take(&mut app.ai);
    state.sync_task_selection(app);
    let result = if state.feedback_needs_paint(ui.ctx()) { state.sync_editor_deferred(app) } else { state.sync_editor(app) };
    if let Err(error) = result {
        state.text = error;
    }
    app.ai = state;
    egui::Frame::NONE.inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let heading = ui.heading(tr("AI Assistant"));
            if let Some(photo) = app.ai.displayed_photo(app).and_then(|id| app.session.catalog.photo(id)) {
                heading.on_hover_text(photo.file_name.as_str());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = ui.button(tr("Collapse"));
                crate::widgets::register(ui.ctx(), "button:aiCollapse", close.rect);
                if close.clicked() {
                    let _ = app.run("panel.ai", json!({}));
                }
            });
        });
        ui.separator();
        if app.ai.comparison.is_some() {
            ui.label(tr("Historical preview"));
            ui.label(tr("Return to the selected photo before starting a new AI task."));
            // The conversation keeps its original viewport and reading state.
            // Resizing it to the smaller historical footer would clamp an
            // intentional upward-reading offset before the user returns.
            return;
        }
        if let Some(scope) = app.ai.readonly_task.filter(|scope| scope.library_generation == app.session.library_generation()) {
            if ui.available_height() < 200.0 {
                let height = ui.available_height().max(0.0);
                let scroll = egui::ScrollArea::vertical()
                    .id_salt(("ai-readonly-task", scope.library_generation, scope.photo.0))
                    .auto_shrink([false, false])
                    .min_scrolled_height(height)
                    .max_height(height)
                    .show(ui, |ui| readonly_task_body(app, ui, scope, true));
                composer_widget(ui, "ai:composer", scroll.inner_rect);
            } else {
                readonly_task_body(app, ui, scope, false);
            }
            return;
        }
        let selection = &app.session.selection;
        let Some(photo) = app.session.active().filter(|_| selection.ids.len() == 1) else {
            health::show(app, ui, ui.available_height() < 200.0);
            let notice = ui.label(tr("Select exactly one photo to start an AI task."));
            crate::widgets::register(ui.ctx(), "ai:selectionNotice", notice.rect);
            return;
        };
        if ui.available_height() >= 200.0
            && let Some(p) = app.session.catalog.photo(photo)
        {
            ui.label(p.file_name.as_str());
        }
        if ui.available_height() >= 500.0 {
            ui.label(tr("Understand and naturally optimize your photo. Originals stay on this computer."));
        }
        let body = ui.available_rect_before_wrap();
        // Measure with a disabled, invisible UI and independent widget IDs. This pass
        // cannot dispatch actions, touch a draft or register control targets.
        let mut measure = ui.new_child(egui::UiBuilder::new().id_salt("ai-composer-measure").max_rect(body).sizing_pass().invisible());
        composer(app, &mut measure, photo, false);
        let compact = measure.min_rect().height() + 96.0 > body.height();
        let height = if compact { body.height().max(0.0) } else { measure.min_rect().height() };
        let footer_rect = egui::Rect::from_min_max(egui::pos2(body.left(), body.bottom() - height), body.max);
        let mut footer = ui.new_child(egui::UiBuilder::new().id_salt("ai-composer").max_rect(footer_rect));
        footer.set_clip_rect(footer_rect.intersect(ui.clip_rect()));
        if compact {
            // Supported short/floating/stacked Tools panes scroll their compact body.
            // Stop and consent precede the draft; full-size panes retain a fixed footer.
            egui::ScrollArea::vertical()
                .id_salt(("ai-compact-composer", app.session.library_generation(), photo.0, app.ai.jobs.contains_key(&photo)))
                .auto_shrink([false, false])
                .min_scrolled_height(0.0)
                .max_height(height)
                .show(&mut footer, |ui| {
                    composer(app, ui, photo, true);
                    ui.separator();
                    message_area(app, ui, photo, 160.0);
                    health::details(app, ui);
                });
        } else {
            composer(app, &mut footer, photo, false);
            let bottom = (footer_rect.top() - 8.0).max(body.top());
            let messages = egui::Rect::from_min_max(body.min, egui::pos2(body.right(), bottom));
            let mut messages_ui = ui.new_child(egui::UiBuilder::new().id_salt("ai-message-viewport").max_rect(messages));
            messages_ui.set_clip_rect(messages.intersect(ui.clip_rect()));
            message_area(app, &mut messages_ui, photo, messages.height());
        }
        composer_widget(ui, "ai:composer", footer_rect);
        ui.advance_cursor_after_rect(body);
    });
}
fn readonly_task_body(app: &mut LightcraftApp, ui: &mut egui::Ui, scope: PhotoScope, compact: bool) {
    if let Some(task) = &mut app.ai.task {
        task.unread = false;
        ui.label(&task.name);
    }
    if app.ai.jobs.contains_key(&scope.photo) {
        let stop = ui.button(tr("Stop Generating"));
        composer_widget(ui, "button:aiStop", stop.rect);
        if stop.clicked() {
            if let Err(error) = app.run("ai.stop", json!({"photo":scope.photo.0})) {
                app.ai.text = error;
            }
            app.ai.defer_local_saves_for_feedback(ui.ctx());
            app.session.defer_companion_retry(true);
        }
    }
    let notice = ui.label(tr("The task photo is unavailable. Its saved conversation is kept."));
    composer_widget(ui, "indicator:aiTaskReadOnly", notice.rect);
    let back = ui.button(tr("View Selected Photo Conversation"));
    composer_widget(ui, "button:aiSelectedConversation", back.rect);
    if back.clicked() {
        let _ = app.run("ai.returnCurrent", json!({}));
    }
    if let Some(trace) = app.ai.conversations.get(&scope.photo).and_then(errors::diagnostic).map(str::to_owned) {
        let copy = ui.button(tr("Copy Diagnostic ID"));
        composer_widget(ui, "button:aiCopyTrace", copy.rect);
        if copy.clicked() {
            ui.ctx().copy_text(trace);
        }
    }
    let clear =
        ui.add_enabled(!app.ai.jobs.contains_key(&scope.photo) && !app.ai.worker_busy(scope.photo), egui::Button::new(tr("Delete Conversation")));
    composer_widget(ui, "button:aiClear", clear.rect);
    if clear.clicked()
        && let Err(error) = app.run("ai.clear", json!({"photo":scope.photo.0}))
    {
        app.ai.text = error;
    }
    save_recovery(app, ui, scope.photo);
    let height = if compact { 240.0 } else { ui.available_height().max(0.0) };
    message_area(app, ui, scope.photo, height);
}
fn save_recovery(app: &mut LightcraftApp, ui: &mut egui::Ui, photo: PhotoId) {
    let sizing = ui.is_sizing_pass();
    let cleanup_id = egui::Id::new(("ai-cleanup-shown", app.session.library_generation(), photo.0));
    let hold_geometry = app.ai.feedback_needs_paint(ui.ctx()) || ui.input(|input| input.pointer.any_down() || input.pointer.any_released());
    let cleanup_shown = if hold_geometry {
        // A pressed control must remain at its painted position through release.
        // Defer new notices, but retain an existing row even if cleanup finishes.
        ui.ctx().data(|data| data.get_temp::<bool>(cleanup_id).unwrap_or(false))
    } else {
        app.session.companion_cleanup_error(photo).is_some()
    };
    if !sizing {
        ui.ctx().data_mut(|data| data.insert_temp(cleanup_id, cleanup_shown));
    }
    if cleanup_shown {
        let notice = ui.label(tr("File cleanup is still pending. Current conversation data is kept."));
        composer_widget(ui, "indicator:aiCleanupPending", notice.rect);
    }
    let protection_error = app.session.companion_revert_save_error(photo);
    // Ordinary dirty drafts and queued saves are not storage failures.
    let unsaved = protection_error.is_some()
        || app.session.unsaved().is_some()
        || app.ai.local_save_retry_at.is_some()
        || app.ai.conversations.get(&photo).is_some_and(|c| c.storage_notice);
    let cause = protection_error.unwrap_or("Local saving needs attention. Photo changes are kept.");
    // New recovery geometry must not move a held control or squeeze out the
    // first cancellation feedback, including discarded reruns of that frame.
    // Existing recovery geometry stays until acknowledgment.
    if unsaved
        && !app.ai.save_retry_feedback.contains_key(&photo)
        && (app.ai.feedback_needs_paint(ui.ctx()) || ui.input(|input| input.pointer.any_down() || input.pointer.any_released()))
    {
        ui.ctx().request_repaint();
        return;
    }
    if unsaved && !sizing {
        app.ai.save_retry_feedback.entry(photo).or_insert(cause);
    }
    if unsaved || app.ai.save_retry_feedback.contains_key(&photo) {
        // Retain the original cause and geometry until acknowledgment. An
        // automatic save between pointer press/release must not move the button.
        ui.label(tr(app.ai.save_retry_feedback.get(&photo).copied().unwrap_or(cause)));
        let retry = ui.button(tr("Retry Saving"));
        composer_widget(ui, "button:aiRetrySave", retry.rect);
        if !sizing
            && retry.clicked()
            && let Err(error) = app.run("ai.retrySave", json!({"photo":photo.0,"library_generation":app.session.library_generation()}))
        {
            app.ai.conversations.entry(photo).or_default().notice = error;
        }
    } else if app.ai.conversations.get(&photo).is_some_and(|c| c.notice == tr("Saved. Retry the edit explicitly when ready.")) {
        ui.label(tr("Saved. Retry the edit explicitly when ready."));
    }
}
fn storage_recovery(app: &LightcraftApp, ui: &mut egui::Ui, photo: PhotoId) -> bool {
    let outer = app.session.companion_archive_protected_reason();
    let inner = app.ai.conversations.get(&photo).and_then(|conversation| conversation.history_error.as_ref());
    let Some(reason) = outer.or_else(|| inner.map(HistoryError::reason)) else { return false };
    let guidance = if outer.is_some() { ROOT_RECOVERY_GUIDANCE } else { inner.map(HistoryError::guidance).unwrap_or(ROOT_RECOVERY_GUIDANCE) };
    ui.label(tr(guidance));
    let copy = ui.button(tr("Copy Storage Diagnostic"));
    composer_widget(ui, "button:aiCopyStorageDiagnostic", copy.rect);
    if !ui.is_sizing_pass() && copy.clicked() {
        ui.ctx().copy_text(
            json!({"kind":"ai-storage-recovery","library_generation":app.session.library_generation(),"photo":photo.0,"reason":reason}).to_string(),
        );
    }
    true
}
fn message_area(app: &mut LightcraftApp, ui: &mut egui::Ui, photo: PhotoId, height: f32) {
    let mut locate = false;
    let mut compare = None;
    let mut revert = None;
    let mut legacy_navigation = None;
    let mut preview_navigation = None;
    let controls_top = ui.cursor().top();
    history_controls(app, ui, photo);
    if app.ai.editors.get(&photo).is_some_and(|e| !e.data.follow_tail) {
        let bottom = ui.button(tr("Back to bottom"));
        composer_widget(ui, "button:aiBackToBottom", bottom.rect);
        if bottom.clicked() {
            let _ = app.run("ai.bottom", json!({"photo":photo.0}));
        }
    }
    let controls_height = (ui.cursor().top() - controls_top).max(0.0);
    let height = (height - controls_height).max(0.0);
    let expected = app.ai.editors.get(&photo).and_then(|e| e.data.reading_cursor);
    let ready = !app.ai.paging.awaiting_page(photo)
        && app.ai.conversations.get(&photo).is_none_or(|c| {
            c.prefix.is_none()
                || c.view.as_ref().is_some_and(|view| {
                    view.card_cursor.is_none()
                        && Some(view.cursor) == expected
                        && (view.bytes > 0 || c.prefix.as_ref().is_some_and(|h| h.message_count == 0))
                })
        });
    let editor = app.ai.editors.get_mut(&photo);
    let tail = editor.as_ref().is_none_or(|e| e.data.follow_tail);
    let restore = editor.and_then(|e| (ready && std::mem::replace(&mut e.restore_scroll, false)).then_some(e.data.scroll_offset));
    let mut area = egui::ScrollArea::vertical()
        .id_salt(("ai-messages", app.session.library_generation(), photo.0))
        .auto_shrink([false, false])
        .stick_to_bottom(tail)
        .max_height(height)
        .min_scrolled_height(height);
    if let Some(offset) = restore
        && !tail
    {
        area = area.vertical_scroll_offset(offset);
    }
    let scroll = area.show(ui, |ui| {
        let protected = storage_recovery(app, ui, photo);
        if let Some(conversation) = app.ai.conversations.get_mut(&photo) {
            app.ai.attachments.sync(app.session.library_generation(), photo, conversation);
            reading::show(ui, conversation, &mut app.ai.attachments);
            if let Some(preview) = conversation.view.as_ref().and_then(|v| v.preview.as_ref()) {
                preview_navigation = historical::show(ui, preview, &mut app.ai.attachments);
            }
            if let Some(header) = conversation.prefix.as_ref().filter(|h| h.legacy_plan_count() > 0) {
                let total = header.legacy_plan_count();
                if total > 0 {
                    legacy_navigation =
                        legacy::show(ui, conversation.view.as_ref().and_then(|v| v.legacy_plan.as_ref()), conversation.legacy_cursor, total);
                }
            }
            let cards: Vec<_> = conversation
                .view
                .iter()
                .flat_map(|v| v.cards.iter())
                .cloned()
                .chain(conversation.records.iter().filter_map(|record| card::from_record(record, None)))
                .collect();
            for card in cards {
                let card = present_card(app, &app.ai, photo, card);
                match card::show(ui, &card, app.ai.readonly_task.is_none() && app.session.active() == Some(photo)) {
                    Some(card::Action::Compare) => compare = Some(card.local_round_id),
                    Some(card::Action::History) => locate = true,
                    Some(card::Action::Undo) => revert = Some(card.local_round_id),
                    None => {}
                }
            }
        } else if !protected && !app.ai.text.is_empty() {
            ui.label(&app.ai.text);
        }
        if restore.is_some() && tail {
            ui.scroll_to_cursor_animation(Some(egui::Align::BOTTOM), egui::style::ScrollAnimation::none());
        }
    });
    composer_widget(ui, "ai:messages", scroll.inner_rect);
    if let Some(index) = preview_navigation {
        let _ = app.run("ai.historicalImage", json!({"index":index}));
    }
    if let Some(index) = legacy_navigation {
        let _ = app.run("ai.legacyPlan", json!({"index":index}));
    }
    // The scroll area consumes its smooth delta. The delivered raw wheel event
    // still expresses user intent in this frame; consider only the visible layer.
    let history_rect = egui::Rect::from_min_max(scroll.inner_rect.min, egui::pos2(scroll.inner_rect.right() + 16.0, scroll.inner_rect.bottom()))
        .intersect(ui.clip_rect());
    let over_history = ui.rect_contains_pointer(history_rect);
    let upward_intent = over_history
        && ui.input(|input| {
            input.raw.events.iter().any(|event| matches!(event,egui::Event::MouseWheel{delta,..} if delta.y>0.0))
                || input.smooth_scroll_delta.y > 0.0
                || (input.pointer.any_down() && app.ai.editors.get(&photo).is_some_and(|e| scroll.state.offset.y + 0.5 < e.data.scroll_offset))
        });
    if upward_intent {
        if app.ai.bottom_reading.remove(&photo) {
            app.ai.paging.cancel();
        }
        drafts::scroll_position(app, photo, scroll.state.offset.y, false);
    } else if ready {
        drafts::scroll_position(app, photo, scroll.state.offset.y, tail);
    }
    if locate {
        let _ = app.run("panel.activity", json!({}));
    }
    if let Some(round) = compare
        && let Err(error) = app.run("ai.compare", json!({"local_round_id":round}))
    {
        app.ai.conversations.entry(photo).or_default().notice = error;
    }
    if let Some(round) = revert
        && let Err(error) = app.run("ai.undo", json!({"photo":photo.0,"local_round_id":round}))
    {
        app.ai.conversations.entry(photo).or_default().notice = error;
    }
}

fn phase_text(phase: &str) -> &'static str {
    match phase {
        "submitting" => "Submitting…",
        "queued" => "Waiting in the queue…",
        "awaiting_input" => "Uploading photo context…",
        "analyzing_initial" => "AI is analyzing the photo…",
        "awaiting_initial_execution" => "Applying the edit plan…",
        "analyzing_review" => "AI is reviewing the result…",
        "awaiting_correction_execution" => "Applying the correction…",
        _ => "Processing this photo…",
    }
}

fn composer_widget(ui: &egui::Ui, id: &str, rect: egui::Rect) {
    let visible = rect.intersect(ui.clip_rect());
    if !ui.is_sizing_pass() && visible.is_positive() {
        crate::widgets::register(ui.ctx(), id, visible);
    }
}

fn composer(app: &mut LightcraftApp, ui: &mut egui::Ui, photo: PhotoId, compact: bool) {
    if app.ai.comparison.is_some() {
        ui.label(tr("Historical preview"));
        ui.label(tr("Return to the selected photo before starting a new AI task."));
        comparison::return_button(app, ui);
        return;
    }
    let sizing = ui.is_sizing_pass();
    let busy = app.ai.jobs.contains_key(&photo);
    let editor_ready = app.ai.editor_photo == Some(photo) && app.ai.editors.contains_key(&photo);
    let worker = app.ai.worker_busy(photo);
    let stage = ui.horizontal_wrapped(|ui| {
        if app.session.companion_migration_pending() {
            if app.session.companion_migration_error().is_some() {
                ui.label(tr("Legacy conversation upgrade paused. Original history is retained."));
                let retry = ui.add_enabled(!app.session.companion_migration_busy(), egui::Button::new(tr("Retry conversation upgrade")));
                composer_widget(ui, "button:aiRetryMigration", retry.rect);
                if !sizing && retry.clicked() {
                    let _ = app.run("ai.retryMigration", json!({}));
                    app.ai.defer_local_saves_for_feedback(ui.ctx());
                    app.session.defer_companion_retry(true);
                }
            } else {
                ui.spinner();
                ui.label(tr("Upgrading legacy conversation…"));
            }
        } else if busy {
            ui.spinner();
            let phase = app
                .ai
                .jobs
                .get(&photo)
                .map(|job| if job.stopped { "Cancelling…" } else { phase_text(&job.state) })
                .unwrap_or("Processing this photo…");
            let phase_label = ui.label(tr(phase));
            if app.ai.telemetry.recent_heartbeat() && app.ai.connection.as_ref().is_some_and(|c| c.alive.load(Ordering::Relaxed) && !c.is_closing()) {
                phase_label.on_hover_text(tr("Connection active; heartbeat acknowledged. Model output is separate."));
            }
            let stop = ui.button(tr("Stop Generating"));
            composer_widget(ui, "button:aiStop", stop.rect);
            if !sizing && stop.clicked() {
                if let Err(error) = app.run("ai.stop", json!({})) {
                    app.ai.text = error;
                }
                app.ai.defer_local_saves_for_feedback(ui.ctx());
                app.session.defer_companion_retry(true);
            }
        } else if worker {
            ui.label(tr("Finishing processing for this photo…"));
        } else if !editor_ready {
            ui.label(tr("Loading draft…"));
        } else {
            ui.label(tr("Ready for your request"));
        }
    });
    composer_widget(ui, "ai:stage", stage.response.rect);
    save_recovery(app, ui, photo);
    if compact {
        consent_control(app, ui, photo);
    }
    if app.session.companion_task_photo() == Some(photo) {
        ui.label(tr("Manual editing is paused while AI processes this photo; stop it or wait for completion."));
    }
    // The draft viewport is fixed, so measuring needs no copy of user text.
    let mut measured_prompt = String::new();
    let prompt = if sizing { &mut measured_prompt } else { &mut app.ai.prompt };
    let input = egui::ScrollArea::vertical()
        .id_salt(("ai-prompt-scroll", app.session.library_generation(), photo.0))
        .min_scrolled_height(48.0)
        .max_height(48.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_enabled(
                !busy && editor_ready,
                egui::TextEdit::multiline(prompt)
                    .id_salt(("ai-draft", app.session.library_generation(), photo.0))
                    .desired_width(f32::INFINITY)
                    .desired_rows(2)
                    .hint_text(tr("For example: review this photo, then naturally optimize it")),
            )
        });
    composer_widget(ui, "field:aiPrompt", input.inner_rect);
    if !sizing && input.inner.changed() {
        drafts::capture(app);
    }
    if !compact {
        consent_control(app, ui, photo);
    }
    health::show(app, ui, compact);
    // Reserve the optional diagnostic control before a cold history read completes,
    // so its arrival cannot invalidate a held consent/submit gesture.
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(tr("Copy Diagnostic ID").into(), font, ui.visuals().text_color());
    let size = egui::vec2(
        (galley.size().x + 2.0 * ui.spacing().button_padding.x).min(ui.available_width()),
        ui.spacing().interact_size.y.max(galley.size().y + 2.0 * ui.spacing().button_padding.y),
    );
    let (slot, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    // Allocate the child in both states so later buttons retain their egui identities.
    let mut diagnostic_row = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("ai-diagnostic", app.session.library_generation(), photo.0))
            .max_rect(slot)
            .layout(egui::Layout::centered_and_justified(egui::Direction::TopDown)),
    );
    if let Some(trace) = app.ai.conversations.get(&photo).and_then(errors::diagnostic).map(str::to_owned) {
        let copy = diagnostic_row.add(egui::Button::new(tr("Copy Diagnostic ID")));
        composer_widget(ui, "button:aiCopyTrace", copy.rect);
        if !sizing && copy.clicked() {
            ui.ctx().copy_text(trace);
        }
    }
    if !busy {
        let admission = app.ai.admission(app, None);
        if let Err(message) = &admission {
            ui.label(message);
        }
        let start = ui.add_enabled(editor_ready && admission.is_ok(), egui::Button::new(tr("Analyze and Optimize")));
        composer_widget(ui, "button:aiStart", start.rect);
        if !sizing
            && start.clicked()
            && let Err(error) = app.run("ai.start", json!({}))
        {
            app.ai.text = error.clone();
            app.ai.conversations.entry(photo).or_default().notice = error;
        }
        ui.horizontal_wrapped(|ui| {
            let button = ui.add_enabled(
                !worker
                    && !app.session.companion_migration_pending()
                    && app.session.companion_archive_protected_reason().is_none()
                    && !app
                        .ai
                        .conversations
                        .get(&photo)
                        .and_then(|conversation| conversation.history_error.as_ref())
                        .is_some_and(HistoryError::ownership_unknown),
                egui::Button::new(tr("Delete Conversation")),
            );
            composer_widget(ui, "button:aiClear", button.rect);
            if !sizing
                && button.clicked()
                && let Err(error) = app.run("ai.clear", json!({}))
            {
                app.ai.conversations.entry(photo).or_default().notice = error;
            }
        });
    }
}

fn consent_control(app: &mut LightcraftApp, ui: &mut egui::Ui, photo: PhotoId) {
    let sizing = ui.is_sizing_pass();
    let mut consent = app.ai.consent;
    let checkbox = ui
        .checkbox(&mut consent, tr("Allow compressed previews to be sent to the cloud model"))
        .on_hover_text(tr("Allow compressed previews to be sent to the cloud model"));
    composer_widget(ui, "checkbox:aiConsent", checkbox.rect);
    if !sizing && checkbox.changed() {
        if let Err(error) = app.ai.set_consent(consent) {
            app.ai.text = error.clone();
            app.ai.conversations.entry(photo).or_default().notice = error;
        }
        app.ai.defer_local_saves_for_feedback(ui.ctx());
        app.session.defer_companion_retry(true);
    }
}

fn history_controls(app: &mut LightcraftApp, ui: &mut egui::Ui, photo: PhotoId) {
    let paging = app.ai.conversations.get(&photo).and_then(|c| c.view.as_ref()).map(|v| (v.cursor, v.next));
    let Some((cursor, next)) = paging else {
        if app.ai.conversations.get(&photo).is_some_and(|c| c.prefix.is_some()) {
            ui.label(tr("Loading AI history…"));
        }
        return;
    };
    ui.horizontal_wrapped(|ui| {
        let older = ui.add_enabled(cursor.record > 0 || cursor.offset > 0, egui::Button::new(tr("Older AI history")));
        composer_widget(ui, "button:aiHistoryOlder", older.rect);
        if !ui.is_sizing_pass() && older.clicked() {
            // Return to the beginning of an oversize message, or move back a
            // bounded page of records. Forward continuation preserves exact text.
            let record = if cursor.offset > 0 { cursor.record } else { cursor.record.saturating_sub(32) };
            let offset = 0;
            let _ = app.run("ai.history", json!({"photo":photo.0,"record":record,"offset":offset}));
        }
        let newer = ui.add_enabled(next.is_some(), egui::Button::new(tr("Newer AI history")));
        composer_widget(ui, "button:aiHistoryNewer", newer.rect);
        if !ui.is_sizing_pass()
            && newer.clicked()
            && let Some(next) = next
        {
            let _ = app.run("ai.history", json!({"photo":photo.0,"record":next.record,"offset":next.offset}));
        }
    });
    if app.ai.paging.busy()
        || app.ai.paging.awaiting_page(photo)
        || app.ai.conversations.get(&photo).and_then(|c| c.view.as_ref()).is_some_and(|v| v.card_cursor.is_some())
    {
        ui.label(tr("Loading AI history…"));
    }
}
