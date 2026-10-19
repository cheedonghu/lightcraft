//! Presentation of caller-owned bounded history; never changes model records.
use super::*;
mod markdown;
mod progress;

#[derive(Clone, Default)]
pub(super) struct RoundSummary {
    pub grouped: bool,
    pub legacy_completed: bool,
    pub final_message: Option<String>,
    pub notice: Option<String>,
}
impl RoundSummary {
    pub(super) fn from_record(record: &Value) -> Self {
        let terminal = record["local_terminal"].as_str();
        Self {
            grouped: terminal.is_some() || record.get("final_message_id").is_some(),
            legacy_completed: terminal == Some("completed") && record.get("final_message_id").is_none(),
            final_message: record["final_message_id"].as_str().filter(|id| id.len() <= 512 && !id.chars().any(char::is_control)).map(str::to_owned),
            notice: record["terminal_notice"].as_str().map(|notice| notice.chars().take(1024).collect()),
        }
    }
    pub(super) fn bytes(&self) -> usize {
        self.final_message.as_ref().map_or(0, String::capacity).saturating_add(self.notice.as_ref().map_or(0, String::capacity))
    }
}

struct Row<'a> {
    message: Option<&'a Value>,
    fragment: Option<&'a str>,
    fallback: String,
    fragment_control: Option<String>,
}
impl Row<'_> {
    fn round(&self) -> Option<&str> {
        self.message?.get("local_round_id")?.as_str()
    }
    fn request(&self) -> bool {
        self.message.is_some_and(|message| message["role"] == "user" && !execution_feedback(message))
    }
    fn id(&self) -> &str {
        self.message.and_then(|m| m["local_message_id"].as_str()).unwrap_or(&self.fallback)
    }
    fn show(&self, ui: &mut egui::Ui, attachments: &mut attachments::Attachments) {
        match (self.message, self.fragment) {
            (Some(message), Some(fragment)) => fragment_row(ui, message, fragment, &self.fallback, attachments),
            (Some(message), None) => message_row(ui, message, &self.fallback, attachments, true),
            (None, Some(fragment)) => {
                let header = egui::CollapsingHeader::new(tr("Message details unavailable"))
                    .id_salt(("unverified-fragment", &self.fallback))
                    .show(ui, |ui| markdown(ui, fragment));
                if let Some(id) = &self.fragment_control {
                    composer_widget(ui, id, header.header_response.rect);
                }
            }
            (None, None) => {
                ui.label(tr("Message unavailable. Original data is retained."));
            }
        }
    }
}

pub(super) fn show(
    ui: &mut egui::Ui,
    conversation: &Conversation,
    attachments: &mut attachments::Attachments,
    active: Option<&str>,
    mut round_end: impl FnMut(&mut egui::Ui, &str),
) {
    let mut rows = Vec::new();
    if let Some(view) = &conversation.view {
        for item in &view.items {
            use lightcraft_engine::companion::storage::HistoryItem;
            match *item {
                HistoryItem::Unavailable { record } => {
                    rows.push(Row { message: None, fragment: None, fallback: format!("unavailable-{record}"), fragment_control: None })
                }
                HistoryItem::Record { record, index } => {
                    if let Some(message) = view.messages.get(index) {
                        rows.push(Row { message: Some(message), fragment: None, fallback: format!("record-{record}"), fragment_control: None });
                    }
                }
                HistoryItem::Fragment { record, index, .. } => {
                    if let Some(fragment) = view.fragments.get(index) {
                        rows.push(Row {
                            message: view.fragment_messages.get(&record),
                            fragment: Some(fragment),
                            fallback: format!("fragment-{record}"),
                            fragment_control: Some(format!("button:aiFragment:{record}:{index}")),
                        });
                    }
                }
            }
        }
    }
    let history_len = rows.len();
    let history_continues = conversation.view.as_ref().is_some_and(|v| v.next.is_some());
    rows.extend(conversation.messages.iter().enumerate().map(|(index, message)| Row {
        message: Some(message),
        fragment: None,
        fallback: format!("tail-{index}"),
        fragment_control: None,
    }));
    // A round can span a history page and the in-memory tail. Attach its card
    // only after its last displayed row, never to the end of another round.
    let last_rows: BTreeMap<_, _> = rows
        .iter()
        .enumerate()
        // Reverting an older round appends a notice today, but must not move
        // that round's historical result below newer conversations.
        .filter(|(_, row)| row.message.is_none_or(|message| message["display_kind"] != "round_reverted"))
        .filter_map(|(index, row)| Some((row.round()?, index)))
        .collect();
    let local: BTreeMap<_, _> =
        conversation.records.iter().filter_map(|record| Some((record["local_round_id"].as_str()?, RoundSummary::from_record(record)))).collect();
    let summaries: BTreeMap<_, _> = conversation
        .view
        .iter()
        .flat_map(|v| v.process.iter().map(|(id, summary)| (id.as_str(), summary)))
        .chain(local.iter().map(|(id, summary)| (*id, summary)))
        .collect();
    let mut index = 0;
    let mut pending_shown = false;
    while let Some(row) = rows.get(index) {
        let Some(round) = row.round().filter(|round| !row.request() && (Some(*round) == active || summaries.get(*round).is_some_and(|s| s.grouped)))
        else {
            row.show(ui, attachments);
            if let Some(round) = row.round()
                && !rows.get(index + 1).is_some_and(|next| next.round() == Some(round))
                && let Some(notice) = summaries.get(round).and_then(|s| s.notice.as_deref()).filter(|notice| *notice != conversation.notice)
            {
                ui.label(errors::display_notice(notice));
            }
            if let Some(round) = row.round().filter(|round| last_rows.get(round) == Some(&index)) {
                round_end(ui, round);
            }
            index += 1;
            continue;
        };
        let mut end = index + 1;
        while !(end == history_len && history_continues) && rows.get(end).is_some_and(|row| row.round() == Some(round) && !row.request()) {
            end += 1;
        }
        let group = rows.get(index..end).unwrap_or_default();
        let summary = summaries.get(round);
        let final_id = summary.and_then(|s| s.final_message.as_deref()).or_else(|| {
            // Compatibility: only an observed end of the round can identify the
            // last old assistant reply. Never promote a page's intermediate fragment.
            let contiguous = end != history_len || !history_continues;
            let boundary = (contiguous && rows.get(end).and_then(Row::round).is_some_and(|next| next != round))
                || (end == rows.len() && (index >= history_len || !history_continues));
            (boundary && summary.is_some_and(|s| s.legacy_completed))
                .then(|| {
                    group
                        .iter()
                        .rev()
                        .find(|row| row.message.is_some_and(|m| m["role"] == "assistant"))
                        .filter(|row| {
                            row.fragment.is_none()
                                && row.message.is_some_and(|m| {
                                    m["tool_calls"].as_array().is_none_or(Vec::is_empty)
                                        && m["text"].as_str().is_some_and(|text| !text.trim().is_empty())
                                })
                        })
                        .map(Row::id)
                })
                .flatten()
        });
        let running = active == Some(round);
        let has_pending = running && !conversation.pending.is_empty();
        if group.iter().any(|row| Some(row.id()) != final_id) || has_pending {
            let has_tools = group.iter().any(|row| row.message.is_some_and(progress::has_tools));
            process(ui, round, running, has_tools, |ui| {
                let process_rows: Vec<_> = group.iter().filter(|row| Some(row.id()) != final_id).collect();
                progress::show(ui, &process_rows, attachments, running && conversation.pending.is_empty());
                if has_pending {
                    pending(ui, conversation);
                }
            });
        }
        pending_shown |= has_pending;
        for row in group.iter().filter(|row| Some(row.id()) == final_id) {
            row.show(ui, attachments);
        }
        if let Some(notice) = summary.and_then(|s| s.notice.as_deref()).filter(|notice| *notice != conversation.notice) {
            ui.label(errors::display_notice(notice));
        }
        if last_rows.get(round).is_some_and(|last| *last < end) {
            round_end(ui, round);
        }
        index = end;
    }
    if !pending_shown && !conversation.pending.is_empty() {
        if let Some(round) = active {
            process(ui, round, true, false, |ui| pending(ui, conversation));
        } else {
            pending(ui, conversation);
        }
    }
    if !conversation.notice.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(super::errors::display_notice(&conversation.notice)).weak());
    }
}

fn process(ui: &mut egui::Ui, round: &str, running: bool, has_tools: bool, body: impl FnOnce(&mut egui::Ui)) {
    // Changing the terminal part of the ID resets even a manually opened active
    // group exactly once at completion; subsequent manual expansion is retained.
    let title = if has_tools { tr("Processing history · contains tool activity") } else { tr("Processing history") };
    let header = egui::CollapsingHeader::new(progress::single_line(ui, egui::RichText::new(title).weak()))
        .id_salt(("ai-process", round, running))
        .default_open(running)
        .show(ui, |ui| {
            ui.scope(|ui| {
                ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("ai-process-running"), running));
                body(ui);
                ui.ctx().data_mut(|data| data.remove::<bool>(egui::Id::new("ai-process-running")));
            });
        });
    composer_widget(ui, &format!("button:aiProcess:{round}"), header.header_response.rect);
    let _ = header.header_response.on_hover_text(title);
    ui.add_space(10.0);
}
fn pending(ui: &mut egui::Ui, conversation: &Conversation) {
    for (id, text) in &conversation.pending {
        ui.push_id(("pending", id), |ui| markdown::show(ui, text));
    }
}

fn message_row(ui: &mut egui::Ui, message: &Value, fallback: &str, attachments: &mut attachments::Attachments, show_tools: bool) {
    let id = message["local_message_id"].as_str().unwrap_or(fallback);
    let role = message["role"].as_str().unwrap_or("system");
    let text = message["text"].as_str().unwrap_or_default();
    let execution = execution_feedback(message);
    ui.push_id(("message", id), |ui| {
        if role == "tool" {
            tool(ui, message, id);
            return;
        }
        let request = role == "user" && !execution && message["display_kind"] != "round_reverted";
        message_frame(ui, request, |ui| {
            if execution {
                ui.label(egui::RichText::new(tr("Execution feedback")).strong());
            } else if message["display_kind"] == "round_reverted" || !matches!(role, "user" | "assistant") {
                ui.label(egui::RichText::new(tr("System")).strong());
            }
            if let Some(images) = message["images"].as_array() {
                for (index, image) in images.iter().enumerate() {
                    attachments.image(ui, id, index, image);
                }
            }
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
                if role == "assistant" && message["display_kind"] != "round_reverted" {
                    markdown::show(ui, visible);
                } else {
                    markdown(ui, if message["display_kind"] == "round_reverted" { tr(visible) } else { visible });
                }
            }
            if show_tools && message["tool_calls"].as_array().is_some_and(|calls| !calls.is_empty()) {
                tool(ui, message, id);
            }
        });
    });
}

// Right-align content without reversing text direction. Both complete
// messages and paged fragments use the same layout, without changing saved roles.
fn message_frame(ui: &mut egui::Ui, request: bool, body: impl FnOnce(&mut egui::Ui)) {
    if request {
        let width = ui.available_width() * 0.85;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Max), |ui| {
                egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(12).inner_margin(10).show(ui, |ui| {
                    ui.set_width((width - 20.0).max(0.0));
                    body(ui);
                });
            });
        });
    } else {
        egui::Frame::NONE.inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 6 }).show(ui, body);
    }
    ui.add_space(4.0);
}
fn tool(ui: &mut egui::Ui, message: &Value, id: &str) {
    let header = egui::CollapsingHeader::new(tr("Tool activity"))
        .id_salt(("tool", id))
        .default_open(ui.ctx().data(|data| data.get_temp::<bool>(egui::Id::new("ai-process-running")).unwrap_or(false)))
        .show(ui, |ui| {
            if let Some(calls) = message["tool_calls"].as_array() {
                for call in calls {
                    let name = call["name"].as_str().or_else(|| call["function"]["name"].as_str()).unwrap_or_default();
                    ui.label(tr(tool_activity(name)));
                }
            } else {
                ui.label(tr("Tool response recorded."));
            }
        });
    composer_widget(ui, &format!("button:aiTool:{id}"), header.header_response.rect);
}
fn tool_activity(name: &str) -> &str {
    match name {
        "skill" => "Read photography guidance",
        "read_skill_resource" => "Read reference material",
        "get_photo_state" | "get_current_state" => "Read photo settings",
        "list_edit_controls" => "Read available edit controls",
        "get_current_preview" => "Read photo preview",
        "get_photo_histogram" => "Read photo histogram",
        "list_manual_masks" => "Read manual masks",
        "sample_photo_color" => "Sample photo colors",
        "apply_edit_plan" => "Prepare photo adjustments",
        _ => "Use a photo tool",
    }
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
    if metadata["role"] == "tool" {
        tool(ui, metadata, id);
    } else if execution_feedback(metadata) {
        let header = egui::CollapsingHeader::new(tr("Execution feedback")).id_salt(("fragment-tool", id)).default_open(false).show(ui, |ui| {
            ui.label(tr("Recorded execution feedback is available in this round's result."));
        });
        composer_widget(ui, &format!("button:aiTool:{id}"), header.header_response.rect);
    } else {
        message_frame(ui, metadata["role"] == "user" && metadata["display_kind"] != "round_reverted", |ui| {
            if metadata["display_kind"] == "round_reverted" || !matches!(metadata["role"].as_str(), Some("user" | "assistant")) {
                ui.label(egui::RichText::new(tr("System")).strong());
            }
            if let Some(images) = metadata["images"].as_array() {
                for (index, image) in images.iter().enumerate() {
                    attachments.image(ui, id, index, image);
                }
            }
            if metadata["role"] == "assistant" {
                // A bounded history slice can start inside code or a table.
                // Its missing prefix cannot safely be inferred as Markdown.
                ui.add(egui::Label::new(text).wrap());
            } else {
                markdown(ui, text);
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
