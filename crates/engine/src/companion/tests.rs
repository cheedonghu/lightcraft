use super::*;
use lightcraft_develop::DevelopSettings;
use serde_json::json;

#[test]
fn companion_white_balance_uses_the_upstream_kelvin_scale() {
    use lightcraft_develop::{WbMode, WbScale};
    let mut settings = DevelopSettings::default();
    settings.wb.mode = WbMode::Custom;
    settings.wb.temp = 5800.0;
    settings.wb.tint = 3.0;
    let info = lightcraft_pipeline::SourceInfo { raw: true, relative_wb: false, as_shot_temp: 4986.0, as_shot_tint: -2.0, ..Default::default() };
    let context = CandidateContext::new(Arc::new(Rgb32f::filled(60, 40, [0.2; 3])), info, &settings, "current");
    let (_, expected_tint) = lightcraft_pipeline::auto::current_wb(&context.info, &settings);
    let changed = candidate(&settings, &json!({"operations":[{"type":"set_controls","values":{"wb.temp":6100}}]}), &context).unwrap();
    assert_eq!(changed.wb.scale, WbScale::Kelvin);
    assert_eq!(changed.wb.temp, 6100.0);
    assert_eq!(changed.wb.tint, expected_tint);
    let changed = candidate(&settings, &json!({"operations":[{"type":"set_white_balance","mode":"custom","temp":6100,"tint":4}]}), &context).unwrap();
    assert_eq!(changed.wb.scale, WbScale::Kelvin);
    assert_eq!((changed.wb.temp, changed.wb.tint), (6100.0, 4.0));
    assert_eq!(settings.wb.scale, WbScale::Legacy, "the original is unchanged");
}

#[test]
fn separate_white_balance_controls_preserve_each_other() {
    let settings = DevelopSettings::default();
    let context = CandidateContext::new(Arc::new(Rgb32f::filled(60, 40, [0.2, 0.1, 0.05])), Default::default(), &settings, "current");
    let changed = candidate(
        &settings,
        &json!({"operations":[{"type":"set_controls","values":{"wb.temp":6100}},{"type":"set_controls","values":{"wb.tint":4}}]}),
        &context,
    )
    .unwrap();
    assert_eq!(changed.wb.temp, 6100.0);
    assert_eq!(changed.wb.tint, 4.0);
}

#[test]
fn tight_crop_context_keeps_actual_frame_but_bounds_retained_sampling_pixels() {
    let mut s = DevelopSettings::default();
    s.crop.geometry.rect = Rect::new(0.4, 0.4, 0.6, 0.6);
    let context = CandidateContext::new(Arc::new(Rgb32f::filled(2000, 1600, [0.2; 3])), Default::default(), &s, "frame");
    assert_eq!(context.frame.src_w, 2000);
    assert!(context.source.width.max(context.source.height) <= 1280);
    let sample = sample(
        &context,
        &s,
        &json!({"purpose":"white_balance","point":{"x":0.5,"y":0.5},"frame_id":"frame","coordinate_space":"preview_normalized"}),
    )
    .unwrap();
    assert!(sample["sample"]["temp"].as_f64().unwrap().is_finite());
}

#[test]
fn rejects_entire_candidate_for_stale_frames_unknown_fields_and_geometry_conflicts() {
    let original = DevelopSettings::default();
    let context = CandidateContext::new(Arc::new(Rgb32f::filled(60, 40, [0.2; 3])), Default::default(), &original, "current");
    let mask = json!({"type":"add_manual_mask","ref":"a","name":"A","frame_id":"current","coordinate_space":"preview_normalized","shape":{"kind":"linear","start":{"x":0.2,"y":0.2},"end":{"x":0.8,"y":0.8}}});
    let mut stale = mask.clone();
    stale["frame_id"] = json!("old");
    for bad in [
        json!({"operations":[{"type":"set_controls","values":{"light.exposure":0.2}},stale]}),
        json!({"operations":[{"type":"set_controls","values":{"geometry.vertical":10}},mask]}),
        json!({"operations":[{"type":"set_controls","values":{"light.exposure":9}}]}),
        json!({"operations":[{"type":"set_treatment","treatment":"bw","toggle":true}]}),
        json!({"operations":[{"type":"set_curve","channel":"red","points":[{"x":0.8,"y":0.2},{"x":0.2,"y":0.8}]}]}),
        json!({"operations":[{"type":"set_white_balance","mode":"asShot","temp":6000}]}),
        json!({"operations":[{"type":"set_controls","values":{"pointColor.0.hueShift":5}}]}),
    ] {
        assert!(candidate(&original, &bad, &context).is_err(), "{bad}");
        assert_eq!(original, DevelopSettings::default());
    }
}
#[test]
fn manual_mask_add_update_remove_and_brush_luminance_are_pure() {
    let original = DevelopSettings::default();
    let context = CandidateContext::new(Arc::new(Rgb32f::filled(60, 40, [0.2; 3])), Default::default(), &original, "current");
    let plan = json!({"operations":[
        {"type":"add_manual_mask","ref":"a","name":"A","frame_id":"current","coordinate_space":"preview_normalized","shape":{"kind":"brush","strokes":[{"points":[{"x":0.2,"y":0.3}],"size":0.1,"feather":50,"flow":100,"density":100,"erase":false,"auto_mask":false}]}},
        {"type":"update_manual_mask","ref":"a","frame_id":"current","coordinate_space":"native_normalized","shape":{"kind":"luminanceRange","lo":0.1,"hi":0.8,"lo_feather":0.1,"hi_feather":0.1}},
        {"type":"set_mask_controls","ref":"a","values":{"exposure":0.3}},
    ]});
    let result = candidate(&original, &plan, &context).unwrap();
    assert_eq!(result.masks.len(), 1);
    assert!(original.masks.is_empty());
    let removed = candidate(&result, &json!({"operations":[{"type":"remove_manual_mask","ref":"mask-1"}]}), &context).unwrap();
    assert!(removed.masks.is_empty());
    let invalid =
        json!({"operations":[{"type":"remove_manual_mask","ref":"mask-1"},{"type":"set_mask_controls","ref":"mask-1","values":{"exposure":1}}]});
    assert!(candidate(&result, &invalid, &context).is_err());
}
#[test]
fn sampling_requires_current_frame_and_never_uses_display_jpeg_values() {
    let original = DevelopSettings::default();
    let context = CandidateContext::new(Arc::new(Rgb32f::filled(60, 40, [0.2, 0.1, 0.05])), Default::default(), &original, "current");
    for (purpose, space) in [("point_color", "OkLCh"), ("color_range", "OkLab"), ("white_balance", "source_linear")] {
        let args = json!({"purpose":purpose,"point":{"x":0.5,"y":0.5},"frame_id":"current","coordinate_space":"preview_normalized"});
        assert_eq!(sample(&context, &original, &args).unwrap()["space"], space);
        let mut stale = args;
        stale["frame_id"] = json!("old");
        assert!(sample(&context, &original, &stale).is_err());
    }
}

#[test]
fn complete_candidate_preserves_unnamed_settings_and_maps_frozen_mask_frame() {
    let mut original = DevelopSettings::default();
    original.effects.clarity = 12.0;
    original.crop.geometry.rect = lightcraft_geom::Rect::new(0.25, 0.0, 0.75, 1.0);
    let context =
        CandidateContext::new(std::sync::Arc::new(lightcraft_raster::Rgb32f::filled(80, 60, [0.18; 3])), Default::default(), &original, "frame-1");
    let plan = json!({"operations":[
        {"type":"set_controls","values":{"light.exposure":0.4,"color.vibrance":15}},
        {"type":"set_crop","rect":{"x0":0.1,"y0":0.1,"x1":0.9,"y1":0.9},"angle":0,"flip_h":false,"flip_v":false},
        {"type":"add_manual_mask","ref":"background","frame_id":"frame-1","coordinate_space":"preview_normalized","name":"背景","shape":{"kind":"radial","center":{"x":0.0,"y":0.5},"rx":0.2,"ry":0.2,"angle":0,"feather":50,"invert":false}},
        {"type":"set_mask_controls","ref":"background","values":{"exposure":-0.5}}
    ]});
    let result = candidate(&original, &plan, &context).unwrap();
    assert_eq!(original.light.exposure, 0.0);
    assert_eq!(result.effects.clarity, 12.0);
    assert_eq!(result.color.vibrance, 15.0);
    assert_eq!(result.masks[0].adjust.exposure, -0.5);
    let lightcraft_develop::MaskShape::Radial { center, .. } = result.masks[0].components[0].shape else { panic!("radial") };
    assert!((center.x - 0.25).abs() < 1e-6, "start frame must survive same-plan crop");
    let duplicate =
        json!({"operations":[{"type":"set_controls","values":{"light.exposure":1}},{"type":"set_controls","values":{"light.exposure":2}}]});
    assert!(candidate(&original, &duplicate, &context).is_err());
}

#[test]
fn historical_model_images_validate_actual_encoding_and_dimensions() {
    let options = crate::export::ExportOptions { format: crate::export::ExportFormat::Png, ..Default::default() };
    let image = lightcraft_raster::Rgba8::filled(16, 16, [12, 40, 60, 255]);
    let bytes = crate::export::encode_image(&image, &options).unwrap();
    assert!(super::validate_history_image("image/png", &bytes).is_ok());
    assert!(super::validate_history_image("image/jpeg", &bytes).is_err());
    assert!(super::validate_history_image("image/png", &bytes[..bytes.len() / 2]).is_err());
    let wide = lightcraft_raster::Rgba8::filled(641, 1, [12, 40, 60, 255]);
    let bytes = crate::export::encode_image(&wide, &options).unwrap();
    assert!(super::validate_history_image("image/png", &bytes).is_err());
}
