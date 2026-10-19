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

struct Activity<'a> {
    owner: &'a Value,
    call: &'a Value,
    result: Option<&'a Value>,
}
impl Activity<'_> {
    fn name(&self) -> &str {
        self.call["name"].as_str().or_else(|| self.call["function"]["name"].as_str()).unwrap_or_default()
    }
    fn matches(&self, result: &Value) -> bool {
        result["tool_call_id"] == self.call["id"]
            && result["local_round_id"] == self.owner["local_round_id"]
            && result["stage"] == self.owner["stage"]
            && result["model_call_id"] == self.owner["model_call_id"]
    }
    fn succeeded(&self) -> bool {
        // Completion alone also describes cancelled/unexecuted protocol results.
        self.result.is_some_and(|result| result["display_outcome"] == "succeeded")
    }
}

pub(super) fn show(ui: &mut egui::Ui, rows: &[&Row<'_>], attachments: &mut attachments::Attachments, continuing: bool) {
    let mut index = 0;
    while let Some(row) = rows.get(index) {
        let Some(message) = row.message.filter(|message| row.fragment.is_none() && message["role"] == "assistant" && has_tools(message)) else {
            row.show(ui, attachments);
            index += 1;
            continue;
        };
        if message["text"].as_str().is_some_and(|text| !text.trim().is_empty()) {
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
        segment(ui, row, &activities, continuing && end == rows.len());
        index = end;
    }
}

fn segment(ui: &mut egui::Ui, first: &Row<'_>, activities: &[Activity<'_>], continuing: bool) {
    let categories: BTreeSet<_> =
        activities.iter().filter(|activity| activity.succeeded()).filter_map(|activity| category(activity.name())).collect();
    let pending = activities.iter().find(|activity| activity.result.is_none());
    let label = if let Some(activity) = pending.filter(|_| continuing) {
        tr(planned(activity.name())).to_owned()
    } else if !categories.is_empty() {
        let template = match (categories.len() == 1, continuing) {
            (true, true) => "Viewed 1 type of photo information; continuing analysis",
            (false, true) => "Viewed {count} types of photo information; continuing analysis",
            (true, false) => "Viewed 1 type of photo information",
            (false, false) => "Viewed {count} types of photo information",
        };
        tr(template).replace("{count}", &categories.len().to_string())
    } else if activities.iter().any(|activity| category(activity.name()).is_some()) {
        tr("Photo information activity").to_owned()
    } else if let Some(activity) = activities.iter().rev().find(|activity| matches!(activity.name(), "skill" | "read_skill_resource")) {
        let source = match (activity.name(), activity.result.is_some(), continuing) {
            ("skill", true, true) => "Photography guidance response recorded; continuing analysis",
            ("skill", true, false) => "Photography guidance response recorded",
            ("read_skill_resource", true, true) => "Reference material response recorded; continuing analysis",
            ("read_skill_resource", true, false) => "Reference material response recorded",
            _ => planned(activity.name()),
        };
        tr(source).to_owned()
    } else {
        tr("Tool activity").to_owned()
    };
    let header = egui::CollapsingHeader::new(single_line(ui, label.as_str()))
        .id_salt(("ai-tool-segment", first.round(), first.id()))
        .default_open(false)
        .show(ui, |ui| {
            for activity in activities {
                let label = if activity.succeeded() {
                    completed(activity.name())
                } else if activity.result.is_none() {
                    planned(activity.name())
                } else {
                    tool_activity(activity.name())
                };
                ui.label(tr(label));
            }
        });
    composer_widget(ui, &format!("button:aiToolSegment:{}", first.id()), header.header_response.rect);
    let _ = header.header_response.on_hover_text(label);
}

fn category(name: &str) -> Option<&str> {
    match name {
        "get_photo_state" | "list_edit_controls" | "get_current_preview" | "get_photo_histogram" | "list_manual_masks" | "sample_photo_color" => {
            Some(name)
        }
        _ => None,
    }
}
fn planned(name: &str) -> &str {
    match name {
        "get_photo_state" => "Preparing to view photo state",
        "list_edit_controls" => "Preparing to view adjustable parameters",
        "get_current_preview" => "Preparing to view current appearance",
        "get_photo_histogram" => "Preparing to read histogram",
        "list_manual_masks" => "Preparing to view local masks",
        "sample_photo_color" => "Preparing to sample colors",
        "skill" => "Preparing to read photography guidance",
        "read_skill_resource" => "Preparing to read reference material",
        _ => tool_activity(name),
    }
}
fn completed(name: &str) -> &str {
    match name {
        "get_photo_state" => "Viewed photo state",
        "list_edit_controls" => "Viewed adjustable parameters",
        "get_current_preview" => "Viewed current appearance",
        "get_photo_histogram" => "Read histogram",
        "list_manual_masks" => "Viewed local masks",
        "sample_photo_color" => "Sampled colors",
        _ => tool_activity(name),
    }
}
