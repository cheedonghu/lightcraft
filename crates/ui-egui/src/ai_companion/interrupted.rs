//! Saved activity is display evidence only; it never recreates a live job or lease.
use super::*;
use lightcraft_engine::companion::storage::{DisplayRecord, DisplayScan, HistoryReader, PhotoHeader};

pub(super) struct Scan {
    header: PhotoHeader,
    scan: DisplayScan,
}
pub(super) struct Record {
    identity: RoundIdentity,
    phase: String,
}
fn latest(header: &PhotoHeader) -> Option<u64> {
    (if header.record_count > 0 { header.record_count } else { header.round_count }).checked_sub(1)
}
fn verified_record(record: &Value, scope: PhotoScope) -> Option<Record> {
    if record.get("local_terminal").is_some()
        || record.get("backend_terminal").is_some()
        || record["photo_id"].as_u64() != Some(scope.photo.0)
        || record["request_text"].as_str().is_none_or(|text| text.trim().is_empty())
        || !record["stages"].is_array()
        || record["runtime_library_generation"].as_u64().is_none_or(|generation| generation == 0)
    {
        return None;
    }
    let phase = record["phase"].as_str()?;
    if !matches!(
        phase,
        "submitting"
            | "queued"
            | "awaiting_input"
            | "analyzing_initial"
            | "awaiting_initial_execution"
            | "analyzing_review"
            | "awaiting_correction_execution"
    ) {
        return None;
    }
    let submission_seq = record["submission_seq"].as_u64().filter(|value| *value > 0)?;
    let connection_generation = record["connection_generation"].as_u64().filter(|value| *value > 0)?;
    // This schema's producer generates a 128-bit local nonce. A transparent
    // serde wrapper alone does not make an arbitrary saved string reliable.
    let nonce = record["local_round_id"].as_str()?.strip_prefix("local-")?;
    if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let local_round_id = serde_json::from_value(record["local_round_id"].clone()).ok()?;
    Some(Record { identity: RoundIdentity { scope, local_round_id, submission_seq, connection_generation }, phase: phase.into() })
}
pub(super) fn read(reader: &mut HistoryReader, scope: PhotoScope) -> Option<Record> {
    let index = latest(reader.header())?;
    let page = reader.rounds_display(index, 1).ok()?;
    verified_record(page.records.first()?, scope)
}
impl State {
    pub(super) fn needs_interruption_check(&self, app: &LightcraftApp, photo: PhotoId) -> bool {
        !self.exiting
            && self.task.is_none()
            && self.jobs.is_empty()
            && self.conversations.get(&photo).is_some_and(|conversation| {
                !conversation.recovery_checked
                    && !conversation.dirty
                    && !conversation.legacy_readonly
                    && conversation.schema_version == 2
                    && conversation.history_error.is_none()
                    && conversation.scope == Some(PhotoScope { photo, library_generation: app.session.library_generation() })
                    && conversation.prefix.as_ref().is_some_and(|header| latest(header).is_some())
            })
    }
    pub(super) fn begin_interruption_check(&mut self, app: &LightcraftApp, photo: PhotoId) {
        if !self.needs_interruption_check(app, photo) {
            return;
        }
        if let Some(conversation) = self.conversations.get_mut(&photo)
            && conversation.recovery_scan.is_none()
            && let Some(header) = conversation.prefix.clone()
            && let Some(index) = latest(&header)
        {
            conversation.recovery_scan = Some(Scan { scan: DisplayScan::new(header.clone(), DisplayRecord::Round(index)), header });
        }
    }
    pub(super) fn accept_interruption(&mut self, app: &LightcraftApp, header: &PhotoHeader, record: Option<Record>) {
        let photo = header.photo_id;
        if !self.needs_interruption_check(app, photo) {
            return;
        }
        let Some(conversation) = self.conversations.get_mut(&photo).filter(|conversation| conversation.prefix.as_ref() == Some(header)) else {
            return;
        };
        conversation.recovery_checked = true;
        conversation.recovery_scan = None;
        let Some(record) = record else {
            return;
        };
        conversation.notice = tr("The previous task was interrupted. It was not resumed.").into();
        let mut task =
            task_entry::Task::new(record.identity, app.session.catalog.photo(photo).map(|photo| photo.file_name.clone()).unwrap_or_default());
        task.phase = record.phase;
        task.terminal = Some("interrupted".into());
        self.task = Some(task);
    }
    pub(super) fn poll_interruption(&mut self, app: &mut LightcraftApp, ctx: &egui::Context) {
        let Some(photo) = self.displayed_photo(app) else {
            return;
        };
        let Some(mut scan) = self.conversations.get_mut(&photo).and_then(|conversation| conversation.recovery_scan.take()) else {
            return;
        };
        if !self.needs_interruption_check(app, photo)
            || self.conversations.get(&photo).and_then(|conversation| conversation.prefix.as_ref()) != Some(&scan.header)
        {
            return;
        }
        match app.session.companion_display_step(&mut scan.scan) {
            Ok(Some(record)) => self.accept_interruption(
                app,
                &scan.header,
                verified_record(&record, PhotoScope { photo, library_generation: app.session.library_generation() }),
            ),
            Err(_) => self.accept_interruption(app, &scan.header, None),
            Ok(None) => {
                if let Some(conversation) = self.conversations.get_mut(&photo) {
                    conversation.recovery_scan = Some(scan);
                }
                ctx.request_repaint();
            }
        }
    }
}
