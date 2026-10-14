//! One captured original plan at a time. These are retained proposals, without
//! inferred execution stages, result, preview pair or runtime undo authority.
use super::*;
use crate::i18n::tr_format;
pub(super) fn show(ui: &mut egui::Ui, plan: Option<&Value>, index: u64, total: u64) -> Option<u64> {
    let mut navigation = None;
    let group = ui.group(|ui| {
        ui.strong(tr("Legacy plan: execution details unavailable"));
        ui.label(tr("This plan was retained from the old conversation. Stage, result, preview pairing and undo are unavailable."));
        ui.horizontal_wrapped(|ui| {
            let previous = ui.add_enabled(index > 0, egui::Button::new(tr("Previous legacy plan")));
            composer_widget(ui, "button:aiLegacyPrevious", previous.rect);
            if !ui.is_sizing_pass() && previous.clicked() {
                navigation = index.checked_sub(1);
            }
            ui.label(tr_format!("Legacy plan {n} of {total}", n = index.saturating_add(1), total = total));
            let next = ui.add_enabled(index.saturating_add(1) < total, egui::Button::new(tr("Next legacy plan")));
            composer_widget(ui, "button:aiLegacyNext", next.rect);
            if !ui.is_sizing_pass() && next.clicked() {
                navigation = index.checked_add(1);
            }
        });
        if let Some(plan) = plan {
            if let Some(reason) = plan.get("reason").and_then(Value::as_str) {
                ui.label(reason);
            }
            if let Some(operations) = plan.get("operations").and_then(Value::as_array) {
                ui.strong(tr("Original proposed operations"));
                for operation in operations {
                    if operation["type"] == "set_controls" {
                        if let Some(values) = operation["values"].as_object() {
                            for (path, value) in values {
                                if let Some(spec) = lightcraft_develop::controls::find(path)
                                    && let Some(value) = value.as_f64()
                                {
                                    let text = if value > 0.0 && spec.min < 0.0 { format!("+{value}") } else { value.to_string() };
                                    ui.label(tr_format!("{parameter}: {value}", parameter = tr(spec.label), value = text));
                                } else {
                                    ui.label(tr("Historical operation details are unavailable."));
                                }
                            }
                        }
                    } else {
                        // Complex original geometry and source-dependent colour
                        // context stay original; never derive targets from today.
                        ui.collapsing(tr("View original operation"), |ui| {
                            ui.label(serde_json::to_string_pretty(operation).unwrap_or_default());
                        });
                    }
                }
            } else {
                ui.label(tr("Historical operation details are unavailable."));
            }
        } else {
            ui.label(tr("Loading legacy plan…"));
        }
    });
    composer_widget(ui, "ai:legacyPlan", group.response.rect);
    navigation
}
