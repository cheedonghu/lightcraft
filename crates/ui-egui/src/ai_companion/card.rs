//! Historical execution facts, presented without confusing proposals with publication.
//! Callers pass one bounded round record. The module owns grouping, labels and values;
//! it never mutates a photo or derives historical display values from today's source.
use crate::i18n::{tr, tr_format};
use lightcraft_develop::{DevelopSettings, controls};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Card {
    pub local_round_id: String,
    pub stages: Vec<Stage>,
    pub current_photo_has_later_edits: bool,
}
#[derive(Serialize)]
pub(super) struct Stage {
    pub kind: String,
    pub reason: String,
    pub status: String,
    pub feedback: String,
    pub changes: Vec<Change>,
}
#[derive(Serialize)]
pub(super) struct Change {
    pub control: String,
    pub label: String,
    pub before: String,
    pub after: String,
    pub target: String,
    pub actual: String,
    pub unit: String,
}
pub(super) fn from_record(record: &Value, current: Option<&DevelopSettings>) -> Option<Card> {
    let id = record.get("local_round_id")?.as_str()?;
    let stages = record.get("stages")?.as_array()?;
    if stages.is_empty() {
        return None;
    }
    let expected = record
        .get("recent_complete_result")
        .or_else(|| stages.iter().rev().find_map(|s| s.get("applied_state")))
        .cloned()
        .and_then(|v| serde_json::from_value::<DevelopSettings>(v).ok());
    Some(Card {
        local_round_id: id.into(),
        current_photo_has_later_edits: expected.as_ref().zip(current).is_some_and(|(a, b)| a != b),
        stages: stages
            .iter()
            .map(|s| {
                let before = s.get("before_state").cloned().and_then(|s| serde_json::from_value::<DevelopSettings>(s).ok());
                let after = s
                    .get("applied_state")
                    .or_else(|| s.get("proposed_state"))
                    .cloned()
                    .and_then(|s| serde_json::from_value::<DevelopSettings>(s).ok());
                let mut changes = Vec::new();
                if let (Some(a), Some(b)) = (before, after) {
                    let actual = if s["locally_published"] == true { &b } else { &a };
                    changes.extend(changed_rows(&composition(&a), &composition(&b), &composition(actual)));
                    for spec in lightcraft_develop::CONTROLS {
                        if spec.id.starts_with("wb.") {
                            continue;
                        }
                        if let (Some(x), Some(y)) = (controls::get(&a, spec.id), controls::get(&b, spec.id))
                            && x != y
                        {
                            changes.push(Change {
                                control: spec.id.into(),
                                label: spec.label.into(),
                                before: spec.format(x),
                                after: spec.format(y),
                                target: spec.format(y),
                                actual: spec.format(if s["locally_published"] == true { y } else { x }),
                                unit: if spec.id == "light.exposure" { "EV" } else { "" }.into(),
                            });
                        }
                    }
                }
                if let Some(a) = s["display_before"]["values"].as_array() {
                    changes.clear();
                    if let Some(b) = s["display_target"]["values"].as_array() {
                        for row in b {
                            let Some(previous) = a.iter().find(|r| r["control"] == row["control"]) else { continue };
                            let actual = s["display_actual"]["values"]
                                .as_array()
                                .and_then(|v| v.iter().find(|r| r["control"] == row["control"]))
                                .unwrap_or(previous);
                            if previous["value"] != row["value"] || previous["value"] != actual["value"] {
                                let value = |r: &Value| r["value"].as_str().unwrap_or_default().to_owned();
                                changes.push(Change {
                                    control: row["control"].as_str().unwrap_or_default().into(),
                                    label: row["label"].as_str().unwrap_or_default().into(),
                                    before: value(previous),
                                    after: value(if s["locally_published"] == true { actual } else { row }),
                                    target: value(row),
                                    actual: value(actual),
                                    unit: row["unit"].as_str().unwrap_or_default().into(),
                                });
                            }
                        }
                    }
                }
                Stage {
                    kind: s["stage"].as_str().unwrap_or("initial").into(),
                    reason: s["reason"].as_str().unwrap_or_default().into(),
                    status: if s["locally_published"] == true { "applied" } else { "not_applied" }.into(),
                    feedback: if s["delivery_state"] == "confirmed" {
                        "confirmed"
                    } else if record["local_terminal"] == "failed" || s["delivery_state"] == "failed" {
                        "unconfirmed"
                    } else {
                        "pending"
                    }
                    .into(),
                    changes,
                }
            })
            .collect(),
    })
}
pub(super) fn show(ui: &mut egui::Ui, card: &Card) -> bool {
    let mut history = false;
    let frame = egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.strong(tr("Round result"));
        ui.small(tr("This card records the round; the editor shows the current photo."));
        if card.current_photo_has_later_edits {
            ui.label(tr("The current photo has later edits."));
        }
        for stage in &card.stages {
            let row = ui.horizontal_wrapped(|ui| {
                ui.strong(tr(if stage.kind == "correction" { "Review correction" } else { "Initial optimization" }));
                ui.label(tr(if stage.status == "applied" { "Applied" } else { "Not applied" }));
            });
            super::composer_widget(ui, &format!("ai:stage:{}:{}", card.local_round_id, stage.kind), row.response.rect);
            if stage.status == "applied" && stage.feedback == "unconfirmed" {
                ui.label(tr("Applied result retained; feedback was not confirmed."));
            }
            if !stage.reason.is_empty() {
                ui.label(&stage.reason);
            }
        }
        let details = egui::CollapsingHeader::new(tr("View parameter changes")).id_salt(("ai-parameters", &card.local_round_id)).show(ui, |ui| {
            for stage in &card.stages {
                ui.strong(tr(if stage.kind == "correction" { "Review correction" } else { "Initial optimization" }));
                for change in &stage.changes {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr(&change.label));
                        ui.label(tr_format!("{before} → {after}", before = tr(&change.before), after = tr(&change.after)));
                        if !change.unit.is_empty() {
                            ui.label(tr(&change.unit));
                        }
                        if stage.status != "applied" {
                            ui.label(tr("Proposed target"));
                            ui.label(tr("Actual retained value"));
                            ui.label(&change.actual);
                        }
                    });
                }
            }
        });
        super::composer_widget(ui, &format!("button:aiParameters:{}", card.local_round_id), details.header_response.rect);
        let locate = ui.button(tr("Open ordinary History"));
        super::composer_widget(ui, &format!("button:aiHistory:{}", card.local_round_id), locate.rect);
        history = locate.clicked();
    });
    super::composer_widget(ui, &format!("ai:card:{}", card.local_round_id), frame.response.rect);
    history
}
/// Freeze the same effective WB scale used by the editor, before the source can change.
pub(super) fn capture(settings: &DevelopSettings, info: &lightcraft_engine::pipeline::SourceInfo) -> Value {
    let mut rows = Vec::new();
    let kelvin = info.raw && !info.relative_wb;
    let (temp, tint) = if kelvin {
        lightcraft_engine::pipeline::local::effective_wb(info, settings)
    } else if settings.wb.mode == lightcraft_develop::WbMode::AsShot {
        (6500.0, 0.0)
    } else {
        (settings.wb.temp, settings.wb.tint)
    };
    for (id, spec) in lightcraft_develop::CONTROLS.iter().map(|s| (s.id.to_owned(), s)).chain(controls::indexed_instances(settings)) {
        let (value, unit) = match id.as_str() {
            "wb.temp" => (
                if kelvin {
                    format!("{temp:.0}")
                } else {
                    let v = ((1e6 / 6500.0 - 1e6 / temp) / 0.8).clamp(-100.0, 100.0);
                    if v > 0.0 { format!("+{v:.0}") } else { format!("{v:.0}") }
                },
                if kelvin { "K" } else { "Relative" },
            ),
            "wb.tint" => (
                if kelvin {
                    spec.format(tint)
                } else {
                    let v = tint.clamp(-100.0, 100.0);
                    if v > 0.0 { format!("+{v:.0}") } else { format!("{v:.0}") }
                },
                if kelvin { "" } else { "Relative" },
            ),
            _ => {
                let Some(v) = controls::get(settings, &id) else { continue };
                (spec.format(v), if id == "light.exposure" { "EV" } else { "" })
            }
        };
        rows.push(serde_json::json!({"control":id,"label":spec.label,"value":value,"unit":unit}));
    }
    rows.extend(composition(settings));
    rows.push(serde_json::json!({"control":"wb.mode","label":"White Balance","value":settings.wb.mode.label(),"unit":""}));
    serde_json::json!({"values":rows,"white_balance_context":{"kelvin":kelvin,"as_shot_temp":info.as_shot_temp,"as_shot_tint":info.as_shot_tint,"legacy_relative_wb":info.legacy_relative_wb}})
}
fn composition(settings: &DevelopSettings) -> Vec<Value> {
    let c = &settings.crop;
    let mut rows = Vec::new();
    for (id, label, v, unit) in [
        ("crop.left", "Crop left", c.geometry.rect.x0 * 100.0, "%"),
        ("crop.top", "Crop top", c.geometry.rect.y0 * 100.0, "%"),
        ("crop.right", "Crop right", c.geometry.rect.x1 * 100.0, "%"),
        ("crop.bottom", "Crop bottom", c.geometry.rect.y1 * 100.0, "%"),
        ("crop.angle", "Straighten", c.geometry.angle, "°"),
    ] {
        rows.push(serde_json::json!({"control":id,"label":label,"value":format!("{v:.1}"),"unit":unit}));
    }
    for (id, label, value) in [("crop.flip_h", "Flip Horizontal", c.flip_h), ("crop.flip_v", "Flip Vertical", c.flip_v)] {
        rows.push(serde_json::json!({"control":id,"label":label,"value":if value {"On"} else {"Off"},"unit":""}));
    }
    rows
}
fn changed_rows(before: &[Value], target: &[Value], actual: &[Value]) -> Vec<Change> {
    let mut changes = Vec::new();
    for row in target {
        let Some(previous) = before.iter().find(|r| r["control"] == row["control"]) else { continue };
        let actual = actual.iter().find(|r| r["control"] == row["control"]).unwrap_or(previous);
        if previous["value"] != row["value"] || previous["value"] != actual["value"] {
            let value = |r: &Value| r["value"].as_str().unwrap_or_default().to_owned();
            changes.push(Change {
                control: row["control"].as_str().unwrap_or_default().into(),
                label: row["label"].as_str().unwrap_or_default().into(),
                before: value(previous),
                after: value(row),
                target: value(row),
                actual: value(actual),
                unit: row["unit"].as_str().unwrap_or_default().into(),
            });
        }
    }
    changes
}
