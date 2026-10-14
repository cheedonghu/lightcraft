//! Presentation of caller-owned bounded history; never changes model records.
use super::*;

pub(super) fn show(ui: &mut egui::Ui, conversation: &Conversation, attachments: &mut attachments::Attachments) {
    if let Some(view) = &conversation.view {
        for item in &view.items {
            match *item {
                lightcraft_engine::companion::storage::HistoryItem::Unavailable { record } => {
                    ui.push_id(("unavailable-message", record), |ui| {
                        ui.group(|ui| {
                            ui.label(tr("Message unavailable. Original data is retained."));
                        });
                    });
                }
                lightcraft_engine::companion::storage::HistoryItem::Record { record, index } => {
                    if let Some(message) = view.messages.get(index) {
                        message_row(ui, message, &format!("record-{record}"), attachments);
                    }
                }
                lightcraft_engine::companion::storage::HistoryItem::Fragment { record, index, .. } => {
                    if let Some(fragment) = view.fragments.get(index) {
                        if let Some(metadata) = view.fragment_messages.get(&record) {
                            fragment_row(ui, metadata, fragment, &format!("fragment-{record}"), attachments);
                        } else {
                            let header = egui::CollapsingHeader::new(tr("Message details unavailable"))
                                .id_salt(("unverified-fragment", record, index))
                                .default_open(false)
                                .show(ui, |ui| {
                                    markdown(ui, fragment);
                                });
                            composer_widget(ui, &format!("button:aiFragment:{record}:{index}"), header.header_response.rect);
                        }
                    }
                }
            }
        }
    }
    for (index, message) in conversation.messages.iter().enumerate() {
        message_row(ui, message, &format!("tail-{index}"), attachments);
    }
    for (id, text) in &conversation.pending {
        ui.push_id(("pending", id), |ui| {
            ui.label(egui::RichText::new(tr("Assistant")).strong());
            markdown(ui, text);
        });
    }
    if !conversation.notice.is_empty() {
        ui.label(egui::RichText::new(tr("System")).strong());
        ui.label(&conversation.notice);
    }
}

fn message_row(ui: &mut egui::Ui, message: &Value, fallback: &str, attachments: &mut attachments::Attachments) {
    let id = message["local_message_id"].as_str().unwrap_or(fallback);
    let role = message["role"].as_str().unwrap_or("system");
    let text = message["text"].as_str().unwrap_or_default();
    let execution = execution_feedback(message);
    ui.push_id(("message", id), |ui| {
        if role == "tool" {
            tool(ui, message, id);
            return;
        }
        ui.group(|ui| {
            let label = if execution {
                "Execution feedback"
            } else if message["display_kind"] == "round_reverted" {
                "System"
            } else {
                match role {
                    "user" => "You",
                    "assistant" => "Assistant",
                    _ => "System",
                }
            };
            ui.label(egui::RichText::new(tr(label)).strong());
            if execution {
                let outcome = text.strip_prefix("Actual execution feedback: ").and_then(|body| serde_json::from_str::<Value>(body).ok());
                let label = match outcome.as_ref().and_then(|v| v["outcome"].as_str()) {
                    Some("succeeded") => "Edits applied.",
                    Some("cancelled") => "Edits were not executed: cancelled.",
                    Some("failed") => "The edit failed. The last complete result was kept.",
                    _ => "Recorded execution feedback is available in this round's result.",
                };
                ui.label(tr(label));
            } else {
                let visible = user_text(message);
                markdown(ui, if message["display_kind"] == "round_reverted" { tr(visible) } else { visible });
            }
            if let Some(images) = message["images"].as_array() {
                for (index, image) in images.iter().enumerate() {
                    attachments.image(ui, id, index, image);
                }
            }
            if message["tool_calls"].as_array().is_some_and(|calls| !calls.is_empty()) {
                tool(ui, message, id);
            }
        });
        ui.add_space(4.0);
    });
}
fn tool(ui: &mut egui::Ui, message: &Value, id: &str) {
    let header = egui::CollapsingHeader::new(tr("Tool process")).id_salt(("tool", id)).default_open(false).show(ui, |ui| {
        if let Some(calls) = message["tool_calls"].as_array() {
            for call in calls {
                ui.label(call["name"].as_str().or_else(|| call["function"]["name"].as_str()).unwrap_or(tr("Tool")));
                if let Some(arguments) = call["arguments"].as_str().or_else(|| call["function"]["arguments"].as_str()) {
                    ui.label(arguments);
                }
            }
        } else {
            ui.label(message["text"].as_str().unwrap_or_default());
        }
    });
    composer_widget(ui, &format!("button:aiTool:{id}"), header.header_response.rect);
}
fn markdown(ui: &mut egui::Ui, text: &str) {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::TextStyle::Body.resolve(ui.style());
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            job.append("\n", 0.0, egui::TextFormat::simple(font.clone(), ui.visuals().text_color()));
        }
        let trimmed = line.trim_start();
        let line = if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            job.append("• ", 0.0, egui::TextFormat::simple(font.clone(), ui.visuals().text_color()));
            item
        } else {
            line
        };
        let mut rest = line;
        while let Some((plain, strong)) = rest.split_once("**") {
            let Some((bold, after)) = strong.split_once("**") else { break };
            job.append(plain, 0.0, egui::TextFormat::simple(font.clone(), ui.visuals().text_color()));
            let strong_font = egui::FontId::new(font.size, egui::FontFamily::Name(crate::theme::FONT_SEMIBOLD.into()));
            job.append(bold, 0.0, egui::TextFormat::simple(strong_font, ui.visuals().strong_text_color()));
            rest = after;
        }
        job.append(rest, 0.0, egui::TextFormat::simple(font.clone(), ui.visuals().text_color()));
    }
    ui.label(job);
}

fn fragment_row(ui: &mut egui::Ui, metadata: &Value, text: &str, fallback: &str, attachments: &mut attachments::Attachments) {
    let id = metadata["local_message_id"].as_str().unwrap_or(fallback);
    if metadata["role"] == "tool" || execution_feedback(metadata) {
        let label = if metadata["role"] == "tool" { "Tool process" } else { "Execution feedback" };
        let header = egui::CollapsingHeader::new(tr(label)).id_salt(("fragment-tool", id)).default_open(false).show(ui, |ui| {
            markdown(ui, text);
        });
        composer_widget(ui, &format!("button:aiTool:{id}"), header.header_response.rect);
    } else {
        ui.group(|ui| {
            let label = match metadata["role"].as_str() {
                Some("user") => "You",
                Some("assistant") => "Assistant",
                _ => "System",
            };
            ui.label(egui::RichText::new(tr(label)).strong());
            markdown(ui, text);
            if let Some(images) = metadata["images"].as_array() {
                for (index, image) in images.iter().enumerate() {
                    attachments.image(ui, id, index, image);
                }
            }
        });
    }
}

// Explicit saved kinds take precedence over legacy text markers.
pub(super) fn execution_feedback(message: &Value) -> bool {
    lightcraft_engine::companion::execution_feedback(message)
}
pub(super) fn user_text(message: &Value) -> &str {
    lightcraft_engine::companion::authored_message_text(message)
}
