//! Historical execution facts, presented without confusing proposals with publication.
//! Callers pass one bounded round record. The module owns grouping, labels and values;
//! it never mutates a photo or derives historical display values from today's source.
use crate::i18n::{tr, tr_format};
use lightcraft_catalog::PhotoId;
use lightcraft_develop::{DevelopSettings, controls};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Card {
    pub local_round_id: String,
    #[serde(skip)]
    pub photo: Option<PhotoId>,
    pub can_undo: bool,
    pub reverted: bool,
    pub stages: Vec<Stage>,
    pub current_photo_has_later_edits: bool,
    #[serde(skip)]
    expected_hash: Option<u64>,
    pub binding: Option<RoundBinding>,
    pub retained_result: Option<String>,
}
#[derive(Clone, Serialize)]
pub(super) struct Stage {
    pub kind: String,
    pub reason: String,
    pub status: String,
    pub feedback: String,
    pub changes: Vec<Change>,
    #[serde(rename = "parameterDetailsUnavailable")]
    pub parameter_details_unavailable: bool,
}
#[derive(Clone, Serialize)]
pub(super) struct Change {
    pub control: String,
    pub label: String,
    pub before: String,
    pub after: String,
    pub target: String,
    pub actual: String,
    pub unit: String,
    pub context: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RoundBinding {
    pub photo: PhotoId,
    pub local_round_id: lightcraft_engine::companion::LocalRoundId,
    pub before_preview_message_id: String,
    pub after_preview_message_id: String,
}
impl Card {
    pub(super) fn for_current(mut self, current: Option<&DevelopSettings>, invalidated: bool) -> Self {
        self.current_photo_has_later_edits = invalidated || self.expected_hash.zip(current).is_some_and(|(hash, s)| hash != s.hash64());
        self
    }
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
        photo: record.get("photo_id").and_then(Value::as_u64).map(PhotoId),
        can_undo: false,
        reverted: record["revert_state"] == "reverted",
        retained_result: if matches!(record["local_terminal"].as_str(), Some("failed" | "cancelled")) {
            Some(
                if stages.iter().any(|s| s["locally_published"] == true && s["stage"] == "correction") {
                    "The latest complete result was retained."
                } else if stages.iter().any(|s| s["locally_published"] == true) {
                    "The initial complete result was retained."
                } else {
                    "The round starting state was retained."
                }
                .into(),
            )
        } else {
            None
        },
        expected_hash: record.get("recent_complete_hash").and_then(Value::as_u64).or_else(|| expected.as_ref().map(DevelopSettings::hash64)),
        binding: record
            .get("photo_id")
            .and_then(Value::as_u64)
            .zip(record.get("before_preview_message_id").and_then(Value::as_str))
            .zip(stages.iter().rev().find(|s| s["locally_published"] == true).and_then(|s| s["preview_message_id"].as_str()))
            .and_then(|((photo, before), after)| {
                Some(RoundBinding {
                    photo: PhotoId(photo),
                    local_round_id: serde_json::from_value(record["local_round_id"].clone()).ok()?,
                    before_preview_message_id: before.into(),
                    after_preview_message_id: after.into(),
                })
            }),
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
                    changes.extend(changed_rows(
                        &composition(&a, Some(&b)),
                        &composition(&b, Some(&a)),
                        &composition(actual, Some(if s["locally_published"] == true { &a } else { &b })),
                    ));
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
                                context: String::new(),
                            });
                        }
                    }
                }
                if let Some(a) = s["display_before"]["values"].as_array()
                    && let Some(b) = s["display_target"]["values"].as_array()
                {
                    let actual = s["display_actual"]["values"].as_array().unwrap_or(a);
                    changes = changed_rows(a, b, actual);
                    if s["locally_published"] == true {
                        for c in &mut changes {
                            c.after = c.actual.clone();
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
                    parameter_details_unavailable: s["parameter_details_unavailable"] == true
                        && !(s["display_before"]["values"].is_array() && s["display_target"]["values"].is_array()),
                }
            })
            .collect(),
    })
}
pub(super) enum Action {
    Compare,
    History,
    Undo,
}
pub(super) fn show(ui: &mut egui::Ui, card: &Card, navigation_available: bool) -> Option<Action> {
    let mut action = None;
    let frame = egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.strong(tr("Round result"));
        ui.small(tr("This card records the round; the editor shows the current photo."));
        if let Some(retained) = &card.retained_result {
            ui.label(tr(retained));
        }
        if card.reverted {
            ui.strong(tr("This round was reverted."));
        }
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
            if stage.parameter_details_unavailable {
                ui.label(tr("Historical parameter details are unavailable."));
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
                        if !change.context.is_empty() {
                            let context = if let Some(index) =
                                change.control.strip_prefix("pointColor.").and_then(|id| id.split('.').next()).and_then(|n| n.parse::<usize>().ok())
                            {
                                tr_format!("Point Color {n}", n = index.saturating_add(1))
                            } else if change.context == "Tone Curve" {
                                tr("Tone Curve").into()
                            } else {
                                change.context.clone()
                            };
                            ui.label(context);
                        }
                        ui.label(tr(&change.label));
                        ui.label(tr_format!("{before} → {after}", before = tr(&change.before), after = tr(&change.after)));
                        if !change.unit.is_empty() {
                            ui.label(tr(&change.unit));
                        }
                        if stage.status != "applied" {
                            ui.label(tr("Proposed target"));
                            ui.label(tr("Actual retained value"));
                            ui.label(tr(&change.actual));
                        }
                    });
                }
            }
        });
        super::composer_widget(ui, &format!("button:aiParameters:{}", card.local_round_id), details.header_response.rect);
        if card.binding.is_some() {
            let compare = ui.button(tr("Historical preview"));
            super::composer_widget(ui, &format!("button:aiCompare:{}", card.local_round_id), compare.rect);
            if compare.clicked() {
                action = Some(Action::Compare);
            }
        }
        let locate = ui.add_enabled(navigation_available, egui::Button::new(tr("Open ordinary History")));
        super::composer_widget(ui, &format!("button:aiHistory:{}", card.local_round_id), locate.rect);
        if locate.clicked() {
            action = Some(Action::History);
        }
        let undo = ui.add_enabled(navigation_available && card.can_undo, egui::Button::new(tr("Undo AI Round")));
        super::composer_widget(ui, &format!("button:aiUndo:{}", card.local_round_id), undo.rect);
        if undo.clicked() {
            action = Some(Action::Undo);
        }
        if !card.can_undo && !card.reverted {
            ui.small(tr("Revert unavailable; use ordinary History."));
        }
    });
    super::composer_widget(ui, &format!("ai:card:{}", card.local_round_id), frame.response.rect);
    action
}
/// Freeze the same effective WB scale used by the editor, before the source can change.
pub(super) fn capture(settings: &DevelopSettings, info: &lightcraft_engine::pipeline::SourceInfo, compare: &DevelopSettings) -> Value {
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
    rows.extend(composition(settings, Some(compare)));
    rows.push(serde_json::json!({"control":"wb.mode","label":"White Balance","value":settings.wb.mode.label(),"unit":""}));
    serde_json::json!({"values":rows,"white_balance_context":{"kelvin":kelvin,"as_shot_temp":info.as_shot_temp,"as_shot_tint":info.as_shot_tint,"legacy_relative_wb":info.legacy_relative_wb}})
}
fn composition(settings: &DevelopSettings, compare: Option<&DevelopSettings>) -> Vec<Value> {
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
    rows.push(serde_json::json!({"control":"treatment","label":"Treatment","value":if settings.treatment==lightcraft_develop::Treatment::Bw {"B&W"} else {"Color"},"unit":""}));
    rows.push(serde_json::json!({"control":"pointColor.count","label":"Point Color","value":settings.point_colors.len().to_string(),"unit":""}));
    rows.push(serde_json::json!({"control":"mask.count","label":"Masks","value":settings.masks.len().to_string(),"unit":""}));
    for (channel, label, points) in [
        ("master", "RGB", &settings.curve.master),
        ("red", "Red", &settings.curve.red),
        ("green", "Green", &settings.curve.green),
        ("blue", "Blue", &settings.curve.blue),
    ] {
        let unchanged=compare.is_some_and(|s| match channel {"master"=>&s.curve.master,"red"=>&s.curve.red,"green"=>&s.curve.green,_=>&s.curve.blue}==points);
        if unchanged {
            continue;
        }
        let text = if points.is_empty() {
            "Linear".into()
        } else {
            points.iter().map(|p| format!("({:.1}, {:.1})", p.x * 100.0, p.y * 100.0)).collect::<Vec<_>>().join(", ")
        };
        rows.push(serde_json::json!({"control":format!("curve.{channel}"),"label":label,"value":text,"unit":"%","context":"Tone Curve"}));
    }
    for (i, color) in settings.point_colors.iter().enumerate() {
        if let Ok(Value::Object(fields)) = serde_json::to_value(color) {
            for (key, value) in fields {
                let Some(v) = value.as_f64() else { continue };
                let label = match key.as_str() {
                    "lum" => "Luminance",
                    "chroma" => "Chroma",
                    "hue" => "Hue",
                    "hue_shift" => "Hue Shift",
                    "sat_shift" => "Saturation Shift",
                    "lum_shift" => "Luminance Shift",
                    "variance" => "Variance",
                    "range" => "Range",
                    "hue_range" => "Hue Range",
                    "sat_range" => "Saturation Range",
                    "lum_range" => "Luminance Range",
                    _ => continue,
                };
                rows.push(serde_json::json!({"control":format!("pointColor.{i}.{key}"),"label":label,"value":format!("{v:.2}"),"unit":if key=="hue" {"°"} else {""},"context":format!("Point Color {}",i+1)}));
            }
        }
    }
    let old_masks: std::collections::BTreeMap<_, _> = compare.into_iter().flat_map(|s| s.masks.iter()).map(|m| (m.id, m)).collect();
    for mask in &settings.masks {
        if old_masks.get(&mask.id).is_some_and(|old| *old == mask) {
            continue;
        }
        for (i, component) in mask.components.iter().enumerate() {
            if old_masks.get(&mask.id).and_then(|m| m.components.get(i)) == Some(component) {
                continue;
            }
            let op = match component.op {
                lightcraft_develop::MaskOp::Add => "Add",
                lightcraft_develop::MaskOp::Subtract => "Subtract",
                lightcraft_develop::MaskOp::Intersect => "Intersect",
            };
            rows.push(serde_json::json!({"control":format!("mask.{}.component.{i}.op",mask.id),"label":"Combination","value":op,"unit":"","context":mask.name}));
            rows.push(serde_json::json!({"control":format!("mask.{}.component.{i}.component_invert",mask.id),"label":"Invert Component","value":if component.invert {"On"} else {"Off"},"unit":"","context":mask.name}));
            if let Ok(Value::Object(shape)) = serde_json::to_value(&component.shape) {
                if let Some(strokes) = shape.get("strokes").and_then(Value::as_array) {
                    for (stroke_index, stroke) in strokes.iter().enumerate() {
                        let prefix = format!("mask.{}.component.{i}.stroke.{stroke_index}", mask.id);
                        if let Some(points) = stroke["points"].as_array() {
                            let text = points
                                .iter()
                                .filter_map(|p| p["x"].as_f64().zip(p["y"].as_f64()))
                                .map(|(x, y)| format!("({:.1}, {:.1})", x * 100.0, y * 100.0))
                                .collect::<Vec<_>>()
                                .join(", ");
                            rows.push(serde_json::json!({"control":format!("{prefix}.points"),"label":"Points","value":text,"unit":"%","context":mask.name}));
                        }
                        for (field, label) in [("size", "Size"), ("feather", "Feather"), ("flow", "Flow"), ("density", "Density")] {
                            if let Some(v) = stroke[field].as_f64() {
                                rows.push(serde_json::json!({"control":format!("{prefix}.{field}"),"label":label,"value":format!("{:.1}",if field=="size"{v*100.0}else{v}),"unit":"%","context":mask.name}));
                            }
                        }
                        for (field, label) in [("erase", "Erase"), ("auto_mask", "Auto Mask")] {
                            if let Some(v) = stroke[field].as_bool() {
                                rows.push(serde_json::json!({"control":format!("{prefix}.{field}"),"label":label,"value":if v{"On"}else{"Off"},"unit":"","context":mask.name}));
                            }
                        }
                    }
                }
                for (key, value) in shape {
                    let label = match key.as_str() {
                        "kind" => "Mask type",
                        "start" => "Start",
                        "end" => "End",
                        "center" => "Center",
                        "rx" => "Radius X",
                        "ry" => "Radius Y",
                        "angle" => "Angle",
                        "feather" => "Feather",
                        "invert" => "Invert",
                        "lo" => "Low",
                        "hi" => "High",
                        "lo_feather" => "Low Feather",
                        "hi_feather" => "High Feather",
                        "samples" => "Samples",
                        "strokes" => "Brush Strokes",
                        "refine" => "Refine",
                        _ => continue,
                    };
                    let (text, unit) = if key == "kind" {
                        let name = match value.as_str() {
                            Some("linear") => "Linear Gradient",
                            Some("radial") => "Radial Gradient",
                            Some("brush") => "Brush",
                            Some("luminanceRange") => "Luminance Range",
                            Some("colorRange") => "Color Range",
                            _ => "Mask",
                        };
                        (name.into(), "")
                    } else if let Some(v) = value.as_f64() {
                        if matches!(key.as_str(), "rx" | "ry" | "lo" | "hi" | "lo_feather" | "hi_feather") {
                            (format!("{:.1}", v * 100.0), "%")
                        } else {
                            (format!("{v:.2}"), if key == "angle" { "°" } else { "" })
                        }
                    } else if let Some(v) = value.as_bool() {
                        (if v { "On" } else { "Off" }.into(), "")
                    } else if let (Some(x), Some(y)) = (value["x"].as_f64(), value["y"].as_f64()) {
                        (format!("({:.1}, {:.1})", x * 100.0, y * 100.0), "%")
                    } else if let Some(v) = value.as_array() {
                        if key == "samples" {
                            let text = v
                                .iter()
                                .filter_map(|s| s.as_array())
                                .filter_map(|s| {
                                    Some(format!("({:.2}, {:.2}, {:.2})", s.first()?.as_f64()?, s.get(1)?.as_f64()?, s.get(2)?.as_f64()?))
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            (text, "OkLab")
                        } else {
                            (v.len().to_string(), "")
                        }
                    } else {
                        continue;
                    };
                    rows.push(serde_json::json!({"control":format!("mask.{}.component.{i}.{key}",mask.id),"label":label,"value":text,"unit":unit,"context":mask.name}));
                }
            }
        }
        let value = serde_json::to_value(mask.adjust).unwrap_or(Value::Null);
        for (key, label) in [
            ("exposure", "Exposure"),
            ("temp", "Temp"),
            ("tint", "Tint"),
            ("contrast", "Contrast"),
            ("highlights", "Highlights"),
            ("shadows", "Shadows"),
            ("whites", "Whites"),
            ("blacks", "Blacks"),
            ("texture", "Texture"),
            ("clarity", "Clarity"),
            ("dehaze", "Dehaze"),
            ("hue", "Hue"),
            ("saturation", "Saturation"),
            ("sharpness", "Sharpness"),
            ("noise", "Noise"),
            ("moire", "Moire"),
            ("defringe", "Defringe"),
            ("color_hue", "Hue"),
            ("color_sat", "Saturation"),
            ("amount", "Amount"),
        ] {
            let Some(v) = value.get(key).and_then(Value::as_f64) else { continue };
            let text = if key == "exposure" {
                if v > 0.0 { format!("+{v:.2}") } else { format!("{v:.2}") }
            } else if v > 0.0 {
                format!("+{v:.0}")
            } else {
                format!("{v:.0}")
            };
            rows.push(serde_json::json!({"control":format!("mask.{}.{key}",mask.id),"label":label,"value":text,"unit":if key=="exposure" {"EV"} else {""},"context":mask.name}));
        }
    }
    rows
}
fn changed_rows(before: &[Value], target: &[Value], actual: &[Value]) -> Vec<Change> {
    let mut changes = Vec::new();
    for row in target.iter().chain(before.iter().filter(|old| !target.iter().any(|r| r["control"] == old["control"]))) {
        let missing = serde_json::json!({"value":"Not present"});
        let previous = before.iter().find(|r| r["control"] == row["control"]).unwrap_or(&missing);
        let target = target.iter().find(|r| r["control"] == row["control"]).unwrap_or(&missing);
        let actual = actual.iter().find(|r| r["control"] == row["control"]).unwrap_or(&missing);
        if previous["value"] != target["value"] || previous["value"] != actual["value"] {
            let value = |r: &Value| r["value"].as_str().unwrap_or_default().to_owned();
            changes.push(Change {
                control: row["control"].as_str().unwrap_or_default().into(),
                label: row["label"].as_str().unwrap_or_default().into(),
                before: value(previous),
                after: value(target),
                target: value(target),
                actual: value(actual),
                unit: row["unit"].as_str().unwrap_or_default().into(),
                context: row["context"].as_str().unwrap_or_default().into(),
            });
        }
    }
    changes
}

/// Persist only controls whose effective values changed, while retaining their source context.
pub(super) fn capture_pair(before: &DevelopSettings, after: &DevelopSettings, info: &lightcraft_engine::pipeline::SourceInfo) -> (Value, Value) {
    let mut a = capture(before, info, after);
    let mut b = capture(after, info, before);
    let left = a["values"].as_array().cloned().unwrap_or_default();
    let right = b["values"].as_array().cloned().unwrap_or_default();
    let missing = serde_json::json!({"value":"Not present"});
    let changed: std::collections::BTreeSet<_> = left
        .iter()
        .chain(right.iter())
        .filter_map(|r| r["control"].as_str())
        .filter(|id| {
            let x = left.iter().find(|r| r["control"].as_str() == Some(id)).unwrap_or(&missing);
            let y = right.iter().find(|r| r["control"].as_str() == Some(id)).unwrap_or(&missing);
            x["value"] != y["value"] || x["unit"] != y["unit"]
        })
        .map(str::to_owned)
        .collect();
    for display in [&mut a, &mut b] {
        if let Some(rows) = display["values"].as_array_mut() {
            rows.retain(|r| r["control"].as_str().is_some_and(|id| changed.contains(id)));
        }
    }
    (a, b)
}
