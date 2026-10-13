//! Last run ownership survives its active Job; only an explicit click navigates.
use super::{LightcraftApp, RoundIdentity, State, phase_text, tr};
use serde_json::{Value, json};
pub(super) struct Task {
    pub identity: RoundIdentity,
    pub name: String,
    pub phase: String,
    pub terminal: Option<String>,
    pub unread: bool,
}
impl Task {
    pub fn new(identity: RoundIdentity, name: String) -> Self {
        Self { identity, name, phase: "submitting".into(), terminal: None, unread: false }
    }
    pub fn status(&self) -> Value {
        json!({"photo": self.identity.scope.photo.0, "libraryGeneration": self.identity.scope.library_generation,
            "localRoundId": self.identity.local_round_id, "name": self.name, "phase": self.phase, "terminal": self.terminal, "unread": self.unread})
    }
}
fn terminal_text(terminal: &str) -> &'static str {
    match terminal {
        "completed" => "Completed",
        "cancelled" => "Stopped",
        "timed_out" => "Timed out",
        "disconnected" => "Disconnected",
        _ => "Failed",
    }
}
pub(super) fn navigate(state: &mut State, app: &mut LightcraftApp) -> Result<Value, String> {
    let task = state.task.as_ref().ok_or_else(|| tr("No AI task to view.").to_owned())?;
    let scope = task.identity.scope;
    if scope.library_generation != app.session.library_generation() {
        return Err(tr("This AI task belongs to a closed library.").into());
    }
    state.sync_editor(app)?;
    let unavailable = app.session.catalog.photo(scope.photo).is_none_or(|p| {
        p.deleted
            || match &p.source {
                lightcraft_catalog::Source::File { path } => app.session.media.availability.exists(path) == Some(false),
                _ => false,
            }
    });
    if unavailable {
        state.readonly_task = Some(scope);
        if let Err(error) = state.load(app, scope.photo) {
            state.text = error;
        }
    } else {
        state.readonly_task = None;
        app.session.execute("library.select", &json!({"ids":[scope.photo.0],"active":scope.photo.0})).map_err(|e| e.to_string())?;
    }
    app.ui.right = crate::state::RightPanel::Ai;
    app.ui.tool.clear();
    state.open = true;
    crate::docking::reveal_command(app, "ai.open")?;
    if let Some(task) = &mut state.task {
        task.unread = false;
    }
    Ok(json!({"photo":scope.photo.0,"opened":true}))
}
pub(super) fn show(app: &mut LightcraftApp, ui: &mut egui::Ui) {
    let Some(task) = app.ai.task.as_ref().filter(|t| t.identity.scope.library_generation == app.session.library_generation()) else {
        return;
    };
    let name = task.name.clone();
    let stopped = app.ai.jobs.get(&task.identity.scope.photo).is_some_and(|j| j.stopped);
    let stage = tr(if task.terminal.is_some() {
        match task.phase.as_str() {
            "submitting" => "Submission",
            "queued" => "Queue",
            "awaiting_input" => "Photo upload",
            "analyzing_initial" => "Photo analysis",
            "awaiting_initial_execution" => "Initial edit",
            "analyzing_review" => "Result review",
            "awaiting_correction_execution" => "Correction",
            _ => "AI task",
        }
    } else if stopped {
        "Cancelling…"
    } else {
        phase_text(&task.phase)
    })
    .to_owned();
    let terminal = task.terminal.as_deref().map(|s| tr(terminal_text(s)).to_owned());
    let unread = task.unread;
    let t = crate::theme::Tokens::get(ui.ctx());
    egui::Panel::top("ai-global-task").exact_size(30.0).frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(12, 0))).show(
        ui,
        |ui| {
            let rect = ui.max_rect();
            let response = ui.interact(rect, egui::Id::new("ai-global-task-entry"), egui::Sense::click());
            crate::widgets::register(ui.ctx(), "button:aiTaskEntry", rect.intersect(ui.clip_rect()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if unread {
                    ui.label(egui::RichText::new(tr("New result")).color(t.accent));
                }
                if let Some(terminal) = &terminal {
                    ui.label(terminal);
                }
                ui.label(&stage);
                ui.add(egui::Label::new(&name).truncate());
                ui.label(tr("AI Task"));
            });
            if response.on_hover_text(crate::i18n::tr_format!("View AI task for {photo}", photo = name)).clicked() {
                if let Err(error) = app.run("ai.task", json!({})) {
                    app.ai.text = error;
                }
            }
        },
    );
}
