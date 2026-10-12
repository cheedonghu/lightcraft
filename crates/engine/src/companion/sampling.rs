//! Source-domain sampling bound to a frozen preview frame.
use super::*;
pub fn sample(context: &CandidateContext, settings: &DevelopSettings, args: &Value) -> Result<Value, String> {
    closed(args, &["purpose", "point", "frame_id", "coordinate_space"])?;
    let p: Point = serde_json::from_value(args.get("point").cloned().ok_or("缺少取样位置。")?).map_err(|e| e.to_string())?;
    let p = context.point(
        p,
        args.get("coordinate_space").and_then(Value::as_str).ok_or("缺少坐标系。")?,
        args.get("frame_id").and_then(Value::as_str).ok_or("缺少 frame。")?,
    )?;
    match args.get("purpose").and_then(Value::as_str) {
        Some("point_color") => {
            let rgb = crate::cmd::color::probe_source(&context.source, &context.info, settings, p, crate::cmd::color::after_mixer_neutral);
            let [lum, chroma, hue] = crate::cmd::color::encoded_to_oklch(rgb);
            Ok(json!({"space":"OkLCh","frame_id":context.frame_id,"sample":{"lum":lum,"chroma":chroma,"hue":hue.to_degrees().rem_euclid(360.0)}}))
        }
        Some("color_range") => {
            let sample = lightcraft_pipeline::color_range_sample(&context.source, &context.info, settings, &RenderRequest::fit(384, 384), p)
                .ok_or("不能取样。")?;
            Ok(json!({"space":"OkLab","frame_id":context.frame_id,"sample":sample}))
        }
        Some("white_balance") => {
            // Probe the warped source before color development, so no JPEG/display transform enters WB.
            let plan = lightcraft_pipeline::plan(&context.source, &context.info, settings, &RenderRequest::fit(640, 640));
            let image = plan.frame.sample(&context.source, plan.w, plan.h);
            let q = plan.frame.norm_to_out(plan.w, plan.h).apply(p);
            let mut acc = [0.0; 3];
            for dy in -2..=2 {
                for dx in -2..=2 {
                    let pixel = image.get_clamped(q.x.floor() as isize + dx, q.y.floor() as isize + dy);
                    for (i, a) in acc.iter_mut().enumerate() {
                        *a += pixel.get(i).copied().unwrap_or(0.0) / 25.0;
                    }
                }
            }
            let (temp, tint) = lightcraft_pipeline::auto::auto_wb(&Rgb32f::filled(4, 4, acc), &context.info);
            Ok(json!({"space":"source_linear","frame_id":context.frame_id,"sample":{"temp":temp,"tint":tint}}))
        }
        _ => Err("未知取样用途。".into()),
    }
}
