//! Pure, photo-bound candidate transformations used by the companion bridge.
pub(crate) mod archive;
mod editor;
pub use editor::EditorState;
mod identity;
mod mutation_guard;
#[cfg(test)]
mod pagination_tests;
#[cfg(test)]
mod recovery_tests;
pub(crate) mod revert_guard;
pub mod storage;
pub use identity::{LocalRoundId, PhotoScope, RoundIdentity};
pub(crate) use mutation_guard::MutationGuard;
pub use mutation_guard::{LeaseToken, RoundRevertState, VerifiedRevert};
mod ranges;
use ranges::LOCAL_RANGES;
mod sampling;
mod schemas;
use lightcraft_develop::{DevelopSettings, Mask, MaskComponent, MaskOp, MaskShape, PointColor, Treatment, WbMode, controls};
use lightcraft_geom::{CropGeometry, Point, Rect};
use lightcraft_pipeline::{RenderRequest, SourceInfo, geometry::Frame};
use lightcraft_raster::Rgb32f;
pub use sampling::sample;
use schemas::validate_nested;
pub use schemas::{control_list, control_ranges, operation_schemas};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub struct CandidateContext {
    pub source: Arc<Rgb32f>,
    pub info: SourceInfo,
    pub frame: Frame,
    pub width: usize,
    pub height: usize,
    pub frame_id: String,
}
impl CandidateContext {
    pub fn new(source: Arc<Rgb32f>, info: SourceInfo, settings: &DevelopSettings, frame_id: &str) -> Self {
        let plan = lightcraft_pipeline::plan(&source, &info, settings, &RenderRequest::fit(640, 640));
        // Freeze the actual render frame, but retain only a bounded source-domain
        // proxy for later probes. A tight crop must not pin a full RAW per round.
        let source = if source.width.max(source.height) > 1280 {
            Arc::new(lightcraft_raster::resample::fit(&source, 1280, 1280, lightcraft_raster::resample::Filter::Bilinear))
        } else {
            source
        };
        Self { source, info, frame: plan.frame, width: plan.w, height: plan.h, frame_id: frame_id.into() }
    }
    fn point(&self, p: Point, space: &str, frame: &str) -> Result<Point, String> {
        if frame != self.frame_id {
            return Err("预览 frame 已过期。".into());
        }
        range(p.x, 0.0, 1.0)?;
        range(p.y, 0.0, 1.0)?;
        match space {
            "native_normalized" => Ok(p),
            "preview_normalized" => {
                Ok(self.frame.out_to_norm(self.width, self.height).apply(Point::new(p.x * self.width as f64, p.y * self.height as f64)))
            }
            _ => Err("未知坐标系。".into()),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Operation {
    #[serde(rename = "set_controls")]
    Controls { values: BTreeMap<String, f64> },
    #[serde(rename = "set_white_balance")]
    WhiteBalance { mode: WbMode, temp: Option<f64>, tint: Option<f64> },
    #[serde(rename = "set_treatment")]
    Treatment { treatment: Treatment },
    #[serde(rename = "set_curve")]
    Curve { channel: String, points: Vec<Point> },
    #[serde(rename = "replace_point_colors")]
    PointColors { colors: Vec<PointColor> },
    #[serde(rename = "set_crop")]
    Crop { rect: Rect, angle: f64, flip_h: bool, flip_v: bool },
    #[serde(rename = "reset_crop")]
    ResetCrop,
    #[serde(rename = "add_manual_mask")]
    AddMask { r#ref: String, name: String, frame_id: String, coordinate_space: String, shape: MaskShape },
    #[serde(rename = "update_manual_mask")]
    UpdateMask { r#ref: String, frame_id: String, coordinate_space: String, shape: MaskShape },
    #[serde(rename = "remove_manual_mask")]
    RemoveMask { r#ref: String },
    #[serde(rename = "set_mask_controls")]
    MaskControls { r#ref: String, values: BTreeMap<String, f64> },
}
fn range(value: f64, lo: f64, hi: f64) -> Result<(), String> {
    if !value.is_finite() || !(lo..=hi).contains(&value) { Err("参数越界，整份方案未执行。".into()) } else { Ok(()) }
}
fn once(seen: &mut BTreeSet<String>, name: String) -> Result<(), String> {
    if seen.insert(name) { Ok(()) } else { Err("方案包含重复或冲突操作。".into()) }
}
fn allowed_control(c: &controls::ControlSpec) -> bool {
    !c.id.starts_with("hdr.") && !c.id.starts_with("enhance.") && !c.id.starts_with("redeye.") && c.id != "crop.angle"
}
pub fn is_manual(mask: &Mask) -> bool {
    !mask.components.is_empty()
        && mask.components.iter().all(|c| {
            matches!(
                c.shape,
                MaskShape::Brush { .. }
                    | MaskShape::Linear { .. }
                    | MaskShape::Radial { .. }
                    | MaskShape::ColorRange { .. }
                    | MaskShape::LuminanceRange { .. }
            )
        })
}

/// Transform the whole plan on a copy. The caller renders and checks ownership before committing.
pub fn candidate(original: &DevelopSettings, plan: &Value, context: &CandidateContext) -> Result<DevelopSettings, String> {
    let values = plan.get("operations").and_then(Value::as_array).filter(|a| !a.is_empty() && a.len() <= 32).ok_or("方案动作数量无效。")?;
    if serde_json::to_vec(values).map_err(|e| e.to_string())?.len() > 64 * 1024 {
        return Err("方案过大。".into());
    }
    for v in values {
        validate_nested(v)?;
    }
    let operations: Vec<Operation> = values.iter().map(|v| serde_json::from_value(v.clone()).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
    let mut candidate = original.clone();
    let mut seen = BTreeSet::new();
    let mut references: BTreeMap<String, u32> = original.masks.iter().filter(|m| is_manual(m)).map(|m| (format!("mask-{}", m.id), m.id)).collect();
    let changes_frame = operations
        .iter()
        .any(|op| matches!(op, Operation::Controls{values} if values.keys().any(|k| k.starts_with("geometry.") || k.starts_with("optics."))));
    if changes_frame && operations.iter().any(|op| matches!(op, Operation::AddMask { .. } | Operation::UpdateMask { .. })) {
        return Err("同方案几何/镜头变换与蒙版结构组合尚未支持。".into());
    }
    let mut replaced_colors = false;
    let mut indexed_colors = false;
    for operation in operations {
        match operation {
            Operation::Controls { values } => {
                if values.is_empty() {
                    return Err("参数目标为空。".into());
                }
                if values.keys().any(|k| k == "wb.temp" || k == "wb.tint") {
                    if seen.contains("white_balance") {
                        return Err("白平衡模式与数值控件冲突。".into());
                    }
                    if seen.insert("white_balance_controls".into()) {
                        lightcraft_pipeline::local::normalize_wb(&mut candidate, &context.info);
                        let (t, ti) = lightcraft_pipeline::auto::current_wb(&context.info, &candidate);
                        candidate.wb.temp = t;
                        candidate.wb.tint = ti;
                        candidate.wb.mode = WbMode::Custom;
                    }
                }
                for (name, value) in values {
                    once(&mut seen, format!("control:{name}"))?;
                    let spec = controls::find(&name).filter(|c| allowed_control(c)).ok_or("未开放控件。")?;
                    if controls::get(original, &name).is_none() {
                        return Err("当前照片不存在该索引。".into());
                    }
                    if name.starts_with("pointColor.") {
                        indexed_colors = true;
                    }
                    range(value, spec.min, spec.max)?;
                    if !controls::set(&mut candidate, &name, value) {
                        return Err("参数设置失败。".into());
                    }
                }
            }
            Operation::WhiteBalance { mode, temp, tint } => {
                if seen.contains("white_balance_controls") {
                    return Err("白平衡模式与数值控件冲突。".into());
                }
                once(&mut seen, "white_balance".into())?;
                if mode != WbMode::Custom && (temp.is_some() || tint.is_some()) {
                    return Err("非 custom 白平衡不能覆盖目标数值。".into());
                }
                let (t, ti) = match mode {
                    WbMode::Custom => (temp.ok_or("custom 需要 temp")?, tint.ok_or("custom 需要 tint")?),
                    WbMode::AsShot => (context.info.as_shot_temp, context.info.as_shot_tint),
                    WbMode::Auto => lightcraft_pipeline::auto::auto_wb(&context.source, &context.info),
                    _ => {
                        let (t, ti) = mode.preset().ok_or("未知白平衡模式")?;
                        (if context.info.raw && !context.info.relative_wb { t } else { 6500.0 * t / 5500.0 }, ti)
                    }
                };
                range(t, 2000.0, 50000.0)?;
                range(ti, -150.0, 150.0)?;
                lightcraft_pipeline::local::normalize_wb(&mut candidate, &context.info);
                candidate.wb = lightcraft_develop::WhiteBalance { mode, temp: t, tint: ti, ..candidate.wb };
            }
            Operation::Treatment { treatment } => {
                once(&mut seen, "treatment".into())?;
                candidate.treatment = treatment;
            }
            Operation::Curve { channel, points } => {
                once(&mut seen, format!("curve:{channel}"))?;
                if points.len() > 32 || points.len() == 1 {
                    return Err("曲线需要空列表或 2 到 32 个点。".into());
                }
                let mut previous = -1.0;
                for p in &points {
                    range(p.x, 0.0, 1.0)?;
                    range(p.y, 0.0, 1.0)?;
                    if p.x <= previous {
                        return Err("曲线点需要严格按 x 递增。".into());
                    }
                    previous = p.x;
                }
                let target = match channel.as_str() {
                    "master" => &mut candidate.curve.master,
                    "red" => &mut candidate.curve.red,
                    "green" => &mut candidate.curve.green,
                    "blue" => &mut candidate.curve.blue,
                    _ => return Err("未知曲线通道。".into()),
                };
                *target = points;
            }
            Operation::PointColors { colors } => {
                once(&mut seen, "point_colors".into())?;
                replaced_colors = true;
                if colors.len() > lightcraft_develop::MAX_POINT_COLORS {
                    return Err("Point Color 样本过多。".into());
                }
                for c in &colors {
                    range(c.lum, 0.0, 1.0)?;
                    range(c.chroma, 0.0, 0.5)?;
                    range(c.hue, 0.0, 360.0)?;
                    for n in [c.hue_shift, c.sat_shift, c.lum_shift, c.variance] {
                        range(n, -100.0, 100.0)?;
                    }
                    for n in [c.range, c.hue_range, c.sat_range, c.lum_range] {
                        range(n, 0.0, 100.0)?;
                    }
                }
                candidate.point_colors = colors;
            }
            Operation::Crop { rect, angle, flip_h, flip_v } => {
                once(&mut seen, "crop".into())?;
                for n in [rect.x0, rect.y0, rect.x1, rect.y1] {
                    range(n, 0.0, 1.0)?;
                }
                range(angle, -45.0, 45.0)?;
                let geometry = CropGeometry { rect, angle };
                if rect.width() < 0.001 || rect.height() < 0.001 || !geometry.is_within_image(context.frame.ow, context.frame.oh) {
                    return Err("裁剪越界或过小。".into());
                }
                candidate.crop = lightcraft_develop::Crop { geometry, aspect: None, flip_h, flip_v };
            }
            Operation::ResetCrop => {
                once(&mut seen, "crop".into())?;
                candidate.crop = Default::default();
            }
            Operation::AddMask { r#ref, name, frame_id, coordinate_space, shape } => {
                if references.contains_key(&r#ref) || r#ref.is_empty() || r#ref.len() > 64 || name.len() > 128 || candidate.masks.len() >= 64 {
                    return Err("蒙版引用或数量无效。".into());
                }
                once(&mut seen, format!("mask-create:{ref}"))?;
                let shape = manual_shape(shape, context, &coordinate_space, &frame_id)?;
                let id = candidate.masks.iter().map(|m| m.id).max().unwrap_or(0).checked_add(1).ok_or("蒙版编号耗尽。")?;
                references.insert(r#ref, id);
                candidate.masks.push(Mask {
                    id,
                    name,
                    components: vec![MaskComponent { name: None, op: MaskOp::Add, invert: false, shape }],
                    ..Default::default()
                });
            }
            Operation::UpdateMask { r#ref, frame_id, coordinate_space, shape } => {
                once(&mut seen, format!("mask-structure:{ref}"))?;
                let shape = manual_shape(shape, context, &coordinate_space, &frame_id)?;
                let mask = find_mask(&mut candidate, &references, &r#ref)?;
                mask.components = vec![MaskComponent { name: None, op: MaskOp::Add, invert: false, shape }];
            }
            Operation::RemoveMask { r#ref } => {
                once(&mut seen, format!("mask-structure:{ref}"))?;
                let id = find_mask(&mut candidate, &references, &r#ref)?.id;
                candidate.masks.retain(|m| m.id != id);
                references.remove(&r#ref);
            }
            Operation::MaskControls { r#ref, values } => {
                if values.is_empty() {
                    return Err("局部参数目标为空。".into());
                }
                let mask = find_mask(&mut candidate, &references, &r#ref)?;
                let mut adjust = serde_json::to_value(mask.adjust).map_err(|e| e.to_string())?;
                for (name, value) in values {
                    once(&mut seen, format!("mask-control:{ref}:{name}"))?;
                    let &(_, lo, hi) = LOCAL_RANGES.iter().find(|(id, _, _)| *id == name).ok_or("未知局部参数。")?;
                    range(value, lo, hi)?;
                    adjust[name] = json!(value);
                }
                mask.adjust = serde_json::from_value(adjust).map_err(|e| e.to_string())?;
            }
        }
    }
    if replaced_colors && indexed_colors {
        return Err("完整 Point Color 替换与索引调整冲突。".into());
    }
    if candidate.curve.split_shadows >= candidate.curve.split_mid || candidate.curve.split_mid >= candidate.curve.split_highlights {
        return Err("参数曲线分区顺序无效。".into());
    }
    Ok(candidate)
}
fn find_mask<'a>(s: &'a mut DevelopSettings, refs: &BTreeMap<String, u32>, name: &str) -> Result<&'a mut Mask, String> {
    let id = refs.get(name).ok_or("蒙版引用不存在或已删除。")?;
    s.masks.iter_mut().find(|m| m.id == *id && is_manual(m)).ok_or_else(|| "蒙版类型未开放。".into())
}
fn manual_shape(mut shape: MaskShape, context: &CandidateContext, space: &str, frame: &str) -> Result<MaskShape, String> {
    if context.frame_id != frame {
        return Err("预览 frame 已过期。".into());
    }
    match &mut shape {
        MaskShape::Linear { start, end } => {
            *start = context.point(*start, space, frame)?;
            *end = context.point(*end, space, frame)?;
            if start == end {
                return Err("渐变端点不能相同。".into());
            }
        }
        MaskShape::Radial { center, rx, ry, angle, feather, .. } => {
            *center = context.point(*center, space, frame)?;
            range(*rx, 0.0001, 1.0)?;
            range(*ry, 0.0001, 1.0)?;
            range(*angle, -180.0, 180.0)?;
            range(*feather, 0.0, 100.0)?;
        }
        MaskShape::Brush { strokes } => {
            if strokes.is_empty() || strokes.len() > 32 || strokes.iter().map(|s| s.points.len()).sum::<usize>() > 2048 {
                return Err("笔刷点数无效。".into());
            }
            for stroke in strokes {
                if stroke.points.is_empty() || stroke.auto_mask {
                    return Err("空笔刷或自动蒙版未开放。".into());
                }
                range(stroke.size, 0.0001, 1.0)?;
                for n in [stroke.feather, stroke.flow, stroke.density] {
                    range(n, 0.0, 100.0)?;
                }
                for p in &mut stroke.points {
                    *p = context.point(*p, space, frame)?;
                }
            }
        }
        MaskShape::LuminanceRange { lo, hi, lo_feather, hi_feather } => {
            for n in [*lo, *hi, *lo_feather, *hi_feather] {
                range(n, 0.0, 1.0)?;
            }
            if lo >= hi {
                return Err("亮度范围无效。".into());
            }
        }
        MaskShape::ColorRange { samples, refine } => {
            range(*refine, 0.0, 100.0)?;
            if samples.is_empty() || samples.len() > 8 {
                return Err("颜色样本数无效。".into());
            }
            for sample in samples {
                range(sample[0], 0.0, 1.0)?;
                range(sample[1], -0.5, 0.5)?;
                range(sample[2], -0.5, 0.5)?;
            }
        }
        _ => return Err("自动或未验证蒙版类型未开放。".into()),
    }
    Ok(shape)
}

fn closed(value: &Value, names: &[&str]) -> Result<(), String> {
    if value.as_object().is_none_or(|o| o.len() != names.len() || o.keys().any(|k| !names.contains(&k.as_str()))) {
        Err("结构字段缺失或包含未知字段。".into())
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests;

/// Validate historical model images using the same actual MIME/dimension/decode
/// bounds as the v1 service. This does not read or publish the original photo.
pub fn validate_history_image(mime: &str, data: &[u8]) -> Result<(), String> {
    let expected = match mime {
        "image/jpeg" => lightcraft_codecs::Format::Jpeg,
        "image/png" => lightcraft_codecs::Format::Png,
        _ => return Err("unsupported historical image MIME".into()),
    };
    if data.is_empty() || data.len() > 128 * 1024 || lightcraft_codecs::sniff(data) != Some(expected) {
        return Err("invalid historical image".into());
    }
    let decoded = lightcraft_codecs::validate_complete_image(data, 640 * 640).map_err(|e| e.to_string())?;
    if decoded.width == 0 || decoded.height == 0 || decoded.width > 640 || decoded.height > 640 {
        return Err("historical image dimensions exceed service limits".into());
    }
    Ok(())
}
