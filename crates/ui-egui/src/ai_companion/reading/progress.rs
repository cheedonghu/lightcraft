//! A presentation-only projection of contiguous, associated tool records.
//! Original messages remain intact for storage and the model conversation.
use super::*;

pub(super) fn single_line(ui: &egui::Ui, text: impl Into<egui::WidgetText>) -> Arc<egui::Galley> {
    text.into().into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (ui.available_width() - ui.spacing().indent - 2.0 * ui.spacing().button_padding.x).max(0.0),
        egui::TextStyle::Button,
    )
}

pub(super) fn has_tools(message: &Value) -> bool {
    message["role"] == "tool" || message["tool_calls"].as_array().is_some_and(|calls| !calls.is_empty())
}

pub(super) fn failed(message: &Value) -> bool {
    message["display_outcome"] == "failed"
        || matches!(message["display_write_state"].as_str(), Some("validation_failed" | "render_failed"))
        || message["display_delivery_state"] == "failed"
}

pub(super) fn failure_notice(message: &Value) -> Option<&str> {
    if !failed(message) {
        return None;
    }
    Some(match message["display_write_state"].as_str() {
        Some("validation_failed") => "Adjustments were not applied: plan rejected",
        Some("render_failed") => "Adjustments were not applied: rendering failed",
        Some("published") if message["display_delivery_state"] == "failed" => "Adjustments applied; feedback not delivered",
        _ => failure_label(message["tool_name"].as_str().unwrap_or_default()),
    })
}

/// Set an existing egui fold once for each newly observed failure, preserving later manual choices.
pub(super) fn open_for_failure(ui: &egui::Ui, id: egui::Id, failed: bool) -> Option<bool> {
    if failed && !ui.ctx().data(|data| data.get_temp::<bool>(id)).unwrap_or(false) {
        ui.ctx().data_mut(|data| data.insert_temp(id, true));
        Some(true)
    } else {
        None
    }
}

struct Activity<'a> {
    owner: &'a Value,
    call: &'a Value,
    result: Option<&'a Value>,
}

#[derive(Clone, Copy)]
enum WriteStatus {
    Preparing,
    Applying,
    Applied,
    AwaitingConfirmation,
    Unconfirmed,
    FeedbackNotDelivered,
    PlanRejected,
    RenderFailed,
    NotApplied,
    Unknown,
}
impl WriteStatus {
    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Preparing => ("Preparing adjustments", "Preparing edits"),
            Self::Applying => ("Applying adjustments", "Applying edits"),
            Self::Applied => ("Adjustments applied", "Adjustments applied"),
            Self::AwaitingConfirmation => ("Adjustments applied; awaiting confirmation", "Adjustments applied"),
            Self::Unconfirmed => ("Adjustments applied; feedback unconfirmed", "Adjustments applied; feedback unconfirmed"),
            Self::FeedbackNotDelivered => ("Adjustments applied; feedback not delivered", "Adjustments applied; feedback failed"),
            Self::PlanRejected => ("Adjustments were not applied: plan rejected", "Adjustments not applied"),
            Self::RenderFailed => ("Adjustments were not applied: rendering failed", "Adjustments failed"),
            Self::NotApplied => ("Adjustments were not applied", "Not applied"),
            Self::Unknown => ("Adjustment activity", "Adjustment activity"),
        }
    }
    fn full_label(self) -> &'static str {
        self.labels().0
    }
    fn compact_label(self) -> &'static str {
        self.labels().1
    }
}

impl Activity<'_> {
    fn name(&self) -> &str {
        self.call["name"].as_str().or_else(|| self.call["function"]["name"].as_str()).unwrap_or_default()
    }
    fn matches(&self, result: &Value) -> bool {
        self.call["id"].as_str().is_some_and(|id| !id.is_empty() && result["tool_call_id"].as_str() == Some(id))
            && self.owner["local_round_id"].as_str().is_some_and(|round| !round.is_empty() && result["local_round_id"].as_str() == Some(round))
            && result["stage"] == self.owner["stage"]
            && result["model_call_id"] == self.owner["model_call_id"]
            && result.get("tool_name").is_none_or(|name| name.as_str() == Some(self.name()))
    }
    fn succeeded(&self) -> bool {
        // Completion alone also describes cancelled/unexecuted protocol results.
        self.result.is_some_and(|result| result["display_outcome"] == "succeeded")
    }
    fn failed(&self) -> bool {
        self.result.is_some_and(failed)
    }
    fn not_executed(&self) -> bool {
        self.result.is_some_and(|result| result["display_outcome"] == "not_executed")
    }
    fn elapsed_us(&self) -> Option<u64> {
        let result = self.result?;
        // A stored duration alone cannot prove execution. Writes record their
        // actual publication separately from the proposal's protocol response.
        (self.succeeded() || result["display_outcome"] == "failed" || result["display_write_state"] == "published")
            .then(|| result["display_elapsed_us"].as_u64())
            .flatten()
    }
    fn write_status(&self, continuing: bool, running: bool) -> Option<WriteStatus> {
        if self.name() != "apply_edit_plan" {
            return None;
        }
        if self.not_executed() {
            return Some(WriteStatus::NotApplied);
        }
        Some(match self.result.and_then(|result| result["display_write_state"].as_str()) {
            Some("applying") => WriteStatus::Applying,
            Some("published") => match self.result.and_then(|result| result["display_delivery_state"].as_str()) {
                Some("failed") => WriteStatus::FeedbackNotDelivered,
                Some("confirmed") => WriteStatus::Applied,
                _ if running => WriteStatus::AwaitingConfirmation,
                _ => WriteStatus::Unconfirmed,
            },
            Some("validation_failed") => WriteStatus::PlanRejected,
            Some("render_failed") => WriteStatus::RenderFailed,
            Some("not_executed" | "not_published") => WriteStatus::NotApplied,
            _ if self.result.is_none() && continuing => WriteStatus::Preparing,
            _ => WriteStatus::Unknown,
        })
    }
}

pub(super) struct Range {
    pub start_known: bool,
    pub end_known: bool,
}

pub(super) fn boundary(row: &Row<'_>) -> bool {
    row.fragment.is_none()
        && row.message.is_some_and(|message| {
            message["role"] == "user"
                || execution_feedback(message)
                || (message["role"] == "assistant" && message["text"].as_str().is_some_and(|text| !text.trim().is_empty()))
        })
}

pub(super) fn show(
    ui: &mut egui::Ui,
    rows: &[&Row<'_>],
    attachments: &mut attachments::Attachments,
    continuing: bool,
    range: Range,
    ending: Option<&str>,
) {
    let mut index = 0;
    let mut beginning_known = range.start_known;
    while let Some(row) = rows.get(index) {
        let Some(message) = row.message.filter(|message| row.fragment.is_none() && message["role"] == "assistant" && has_tools(message)) else {
            beginning_known = boundary(row);
            row.show(ui, attachments);
            index += 1;
            continue;
        };
        if message["text"].as_str().is_some_and(|text| !text.trim().is_empty()) {
            beginning_known = true;
            message_row(ui, message, &row.fallback, attachments, false);
        }
        let mut activities = Vec::<Activity<'_>>::new();
        let mut end = index;
        while let Some(next) = rows.get(end) {
            let Some(message) = next.message.filter(|_| next.fragment.is_none()) else { break };
            if message["role"] == "assistant" {
                if end != index && message["text"].as_str().is_some_and(|text| !text.trim().is_empty()) {
                    break;
                }
                let Some(calls) = message["tool_calls"].as_array().filter(|calls| !calls.is_empty()) else { break };
                for call in calls {
                    if !activities.iter().any(|activity| {
                        activity.call["id"] == call["id"]
                            && activity.owner["stage"] == message["stage"]
                            && activity.owner["model_call_id"] == message["model_call_id"]
                    }) {
                        activities.push(Activity { owner: message, call, result: None });
                    }
                }
            } else if message["role"] == "tool" {
                let Some(activity) = activities.iter_mut().find(|activity| activity.matches(message)) else { break };
                if activity.result.is_none() {
                    activity.result = Some(message);
                }
            } else {
                break;
            }
            end += 1;
        }
        let ending_known = rows.get(end).map_or(range.end_known, |row| boundary(row));
        segment(ui, row, &activities, continuing && end == rows.len(), beginning_known && ending_known, ending);
        beginning_known = false;
        index = end;
    }
}

fn segment(ui: &mut egui::Ui, first: &Row<'_>, activities: &[Activity<'_>], continuing: bool, complete: bool, ending: Option<&str>) {
    let running = ui.ctx().data(|data| data.get_temp::<bool>(egui::Id::new("ai-process-running"))).unwrap_or(false);
    let categories: BTreeSet<_> =
        activities.iter().filter(|activity| complete && activity.succeeded()).filter_map(|activity| category(activity.name())).collect();
    let pending = activities.iter().find(|activity| activity.result.is_none());
    let write = activities.iter().rev().find(|activity| activity.name() == "apply_edit_plan");
    let count = categories.len();
    let short_count = || tr(if count == 1 { "Viewed 1 info type" } else { "Viewed {count} info types" }).replace("{count}", &count.to_string());
    let write_count = || tr(if count == 1 { "1 type" } else { "{count} types" }).replace("{count}", &count.to_string());
    let duration = if count == 0 {
        None
    } else {
        activities
            .iter()
            .try_fold(0u64, |total, activity| total.checked_add(activity.elapsed_us()?))
            .map(|total| format!("{:.1}s", total as f64 / 1_000_000.0))
    };
    let label = if let Some(activity) = pending.filter(|_| continuing) {
        match activity.name() {
            name if category(name).is_some() => tr("Viewing photo information…"),
            "apply_edit_plan" => tr("Preparing adjustments…"),
            "skill" => tr("Reading photography guidance…"),
            "read_skill_resource" => tr("Reading reference material…"),
            _ => tr("Working on the photo…"),
        }
        .to_owned()
    } else if count > 0 {
        tr(if count == 1 { "Viewed 1 type of photo information" } else { "Viewed {count} types of photo information" })
            .replace("{count}", &count.to_string())
    } else if activities.iter().any(|activity| category(activity.name()).is_some()) {
        tr(if ending.is_some() { "Photo information reading incomplete" } else { "Photo information activity" }).to_owned()
    } else if let Some(activity) = activities.iter().rev().find(|activity| matches!(activity.name(), "skill" | "read_skill_resource")) {
        let source = if activity.not_executed() {
            "Tool was not executed"
        } else {
            match (activity.name(), activity.result.is_some(), continuing) {
                ("skill", true, true) => "Reading photography guidance…",
                ("skill", true, false) => "Read photography guidance",
                ("read_skill_resource", true, true) => "Reading reference material…",
                ("read_skill_resource", true, false) => "Read reference material",
                _ => planned(activity.name()),
            }
        };
        tr(source).to_owned()
    } else {
        tr("Working on the photo").to_owned()
    };
    let write = write.filter(|activity| {
        activity.result.is_some_and(|result| result["display_write_state"] == "applying")
            || activities.last().is_some_and(|last| last.name() == "apply_edit_plan")
    });
    let write_status = write.and_then(|activity| activity.write_status(continuing, running));
    let mut label = if let Some(status) = write_status {
        let write_label = tr(status.full_label());
        if count == 0 { write_label.to_owned() } else { format!("{label} · {write_label}") }
    } else {
        label
    };
    let mut caption = if let Some(status) = write_status {
        let write_label = tr(status.compact_label());
        if count == 0 { write_label.to_owned() } else { format!("{} · {write_label}", write_count()) }
    } else if count > 0 {
        short_count()
    } else {
        label.clone()
    };
    if let Some(duration) = &duration {
        label.push_str(&format!(" · {duration}"));
        if count > 0 && write_status.is_none() {
            caption = tr(if count == 1 { "Viewed 1 info type · {time}" } else { "Viewed {count} info types · {time}" })
                .replace("{count}", &count.to_string())
                .replace("{time}", duration);
        }
    }
    let failure = activities.iter().find(|activity| activity.failed());
    if let Some(activity) = failure {
        let reason = tr(activity.write_status(false, running).map(WriteStatus::full_label).unwrap_or_else(|| failure_label(activity.name())));
        let photo_reads = activities.iter().any(|activity| category(activity.name()).is_some());
        if photo_reads {
            if categories.is_empty() && complete {
                label = tr("Photo information reading incomplete").to_owned();
                caption = label.clone();
            }
            label.push_str(&format!(" · {reason}"));
            caption.push_str(&format!(" · {reason}"));
        } else {
            label = reason.to_owned();
            caption = label.clone();
        }
    }
    if let Some(ending) = ending {
        label.push_str(&format!(" · {}", tr(ending)));
        caption.push_str(&format!(" · {}", tr(ending)));
    }
    let failures = activities.iter().filter(|activity| activity.failed()).count();
    let open = open_for_failure(ui, egui::Id::new(("ai-tool-failure-open", first.round(), first.id(), failures)), failure.is_some());
    let header = egui::CollapsingHeader::new(single_line(ui, caption.as_str()))
        .id_salt(("ai-tool-segment", first.round(), first.id()))
        .default_open(false)
        .open(open)
        .show(ui, |ui| {
            for activity in activities {
                if activity.not_executed() {
                    ui.label(format!("{} · {}", tr(tool_activity(activity.name())), tr("Tool was not executed")));
                    continue;
                }
                let label = if let Some(status) = activity.write_status(continuing, running) {
                    status.full_label()
                } else if activity.failed() {
                    failure_label(activity.name())
                } else if activity.succeeded() {
                    completed(activity.name())
                } else if activity.result.is_none() && continuing {
                    planned(activity.name())
                } else if activity.result.is_none() {
                    "Tool was not executed"
                } else {
                    tool_activity(activity.name())
                };
                ui.label(tr(label));
            }
        });
    composer_widget(ui, &format!("button:aiToolSegment:{}", first.id()), header.header_response.rect);
    let _ = header.header_response.on_hover_text(label);
}

struct ToolDescriptor {
    photo_information: bool,
    planned: &'static str,
    completed: &'static str,
    failure: &'static str,
}

fn descriptor(name: &str) -> Option<ToolDescriptor> {
    let (photo_information, planned, completed, failure) = match name {
        "get_photo_state" => (true, "Preparing to view photo state", "Viewed photo state", "Reading photo state failed"),
        "list_edit_controls" => {
            (true, "Preparing to view adjustable parameters", "Viewed adjustable parameters", "Reading adjustable parameters failed")
        }
        "get_current_preview" => (true, "Preparing to view current appearance", "Viewed current appearance", "Reading current appearance failed"),
        "get_photo_histogram" => (true, "Preparing to read histogram", "Read histogram", "Reading histogram failed"),
        "list_manual_masks" => (true, "Preparing to view local masks", "Viewed local masks", "Reading local masks failed"),
        "sample_photo_color" => (true, "Preparing to sample colors", "Sampled colors", "Color sampling failed"),
        "skill" => (false, "Preparing to read photography guidance", "Read photography guidance", "Photography guidance unavailable"),
        "read_skill_resource" => (false, "Preparing to read reference material", "Read reference material", "Reference material unavailable"),
        "apply_edit_plan" => (false, "Preparing adjustments", "Prepare photo adjustments", "Tool execution failed"),
        _ => return None,
    };
    Some(ToolDescriptor { photo_information, planned, completed, failure })
}

fn category(name: &str) -> Option<&str> {
    descriptor(name).is_some_and(|tool| tool.photo_information).then_some(name)
}
fn planned(name: &str) -> &str {
    descriptor(name).map(|tool| tool.planned).unwrap_or_else(|| tool_activity(name))
}
fn completed(name: &str) -> &str {
    descriptor(name).map(|tool| tool.completed).unwrap_or_else(|| tool_activity(name))
}
fn failure_label(name: &str) -> &str {
    descriptor(name).map_or("Tool execution failed", |tool| tool.failure)
}
