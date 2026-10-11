//! Strict schemas projected from the native control and settings model.
use super::*;
pub fn control_ranges(s: &DevelopSettings) -> Value {
    let mut result = serde_json::Map::new();
    for (name, spec) in
        lightcraft_develop::CONTROLS.iter().filter(|c| allowed_control(c)).map(|c| (c.id.to_owned(), c)).chain(controls::indexed_instances(s))
    {
        result.insert(name, json!({"minimum":spec.min,"maximum":spec.max}));
    }
    Value::Object(result)
}
pub fn control_list(s: &DevelopSettings, info: &SourceInfo) -> Value {
    let (temp, tint) = lightcraft_pipeline::auto::current_wb(info, s);
    let ranges = control_ranges(s);
    Value::Array(ranges.as_object().into_iter().flat_map(|o| o.iter()).map(|(id,r)|json!({"id":id,"minimum":r["minimum"],"maximum":r["maximum"],"current":match id.as_str(){"wb.temp"=>Some(temp),"wb.tint"=>Some(tint),_=>controls::get(s,id)}})).collect())
}
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn number(lo: f64, hi: f64) -> Value {
    json!({"type":"number","minimum":lo,"maximum":hi})
}
fn text(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn array(items: Value, min: usize, max: usize) -> Value {
    json!({"type":"array","items":items,"minItems":min,"maxItems":max})
}
fn point_schema() -> Value {
    object(json!({"x":number(0.0,1.0),"y":number(0.0,1.0)}), &["x", "y"])
}
fn shape_schemas() -> Value {
    let mut shapes = Vec::new();
    let mut add = |kind: &str, mut properties: Value, required: &[&str]| {
        properties["kind"] = json!({"type":"string","enum":[kind]});
        let mut req = required.to_vec();
        req.push("kind");
        shapes.push(object(properties, &req));
    };
    add("linear", json!({"start":point_schema(),"end":point_schema()}), &["start", "end"]);
    add(
        "radial",
        json!({"center":point_schema(),"rx":number(0.0001,1.0),"ry":number(0.0001,1.0),"angle":number(-180.0,180.0),"feather":number(0.0,100.0),"invert":{"type":"boolean"}}),
        &["center", "rx", "ry", "angle", "feather", "invert"],
    );
    let stroke = object(
        json!({"points":array(point_schema(),1,2048),"size":number(0.0001,1.0),"feather":number(0.0,100.0),"flow":number(0.0,100.0),"density":number(0.0,100.0),"erase":{"type":"boolean"},"auto_mask":{"type":"boolean","enum":[false]}}),
        &["points", "size", "feather", "flow", "density", "erase", "auto_mask"],
    );
    add("brush", json!({"strokes":array(stroke,1,32)}), &["strokes"]);
    add(
        "luminanceRange",
        json!({"lo":number(0.0,1.0),"hi":number(0.0,1.0),"lo_feather":number(0.0,1.0),"hi_feather":number(0.0,1.0)}),
        &["lo", "hi", "lo_feather", "hi_feather"],
    );
    // OkLab, not RGB or Point Color's OkLCh.
    add("colorRange", json!({"samples":array(array(number(-0.5,1.0),3,3),1,8),"refine":number(0.0,100.0)}), &["samples", "refine"]);
    json!({"oneOf":shapes})
}
/// Schemas project only implemented operations. Their type tags are also filtered by the Go gate.
pub fn operation_schemas(s: &DevelopSettings) -> Value {
    let mut schemas = serde_json::Map::new();
    let mut add = |kind: &str, mut properties: Value, required: &[&str]| {
        properties["type"] = json!({"type":"string","enum":[kind]});
        let mut req = required.to_vec();
        req.push("type");
        schemas.insert(kind.into(), object(properties, &req));
    };
    let ranges = control_ranges(s);
    let controls: serde_json::Map<_, _> = ranges
        .as_object()
        .into_iter()
        .flat_map(|o| o.iter())
        .map(|(id, r)| (id.clone(), number(r["minimum"].as_f64().unwrap_or(0.0), r["maximum"].as_f64().unwrap_or(0.0))))
        .collect();
    let targets = json!({"type":"object","properties":controls,"minProperties":1,"additionalProperties":false});
    add("set_controls", json!({"values":targets}), &["values"]);
    add(
        "set_white_balance",
        json!({"mode":{"type":"string","enum":["asShot","auto","daylight","cloudy","shade","tungsten","fluorescent","flash","custom"]},"temp":number(2000.0,50000.0),"tint":number(-150.0,150.0)}),
        &["mode"],
    );
    add("set_treatment", json!({"treatment":{"type":"string","enum":["color","bw"]}}), &["treatment"]);
    add(
        "set_curve",
        json!({"channel":{"type":"string","enum":["master","red","green","blue"]},"points":array(point_schema(),0,32)}),
        &["channel", "points"],
    );
    let color = object(
        json!({"lum":number(0.0,1.0),"chroma":number(0.0,0.5),"hue":number(0.0,360.0),"hue_shift":number(-100.0,100.0),"sat_shift":number(-100.0,100.0),"lum_shift":number(-100.0,100.0),"variance":number(-100.0,100.0),"range":number(0.0,100.0),"hue_range":number(0.0,100.0),"sat_range":number(0.0,100.0),"lum_range":number(0.0,100.0)}),
        &["lum", "chroma", "hue", "hue_shift", "sat_shift", "lum_shift", "variance", "range", "hue_range", "sat_range", "lum_range"],
    );
    add("replace_point_colors", json!({"colors":array(color,0,lightcraft_develop::MAX_POINT_COLORS)}), &["colors"]);
    let rect = object(json!({"x0":number(0.0,1.0),"y0":number(0.0,1.0),"x1":number(0.0,1.0),"y1":number(0.0,1.0)}), &["x0", "y0", "x1", "y1"]);
    add(
        "set_crop",
        json!({"rect":rect,"angle":number(-45.0,45.0),"flip_h":{"type":"boolean"},"flip_v":{"type":"boolean"}}),
        &["rect", "angle", "flip_h", "flip_v"],
    );
    add("reset_crop", json!({}), &[]);
    let shape = shape_schemas();
    let coords = json!({"type":"string","enum":["native_normalized","preview_normalized"]});
    add(
        "add_manual_mask",
        json!({"ref":text(64),"name":text(128),"frame_id":text(128),"coordinate_space":coords,"shape":shape}),
        &["ref", "name", "frame_id", "coordinate_space", "shape"],
    );
    add(
        "update_manual_mask",
        json!({"ref":text(64),"frame_id":text(128),"coordinate_space":coords,"shape":shape}),
        &["ref", "frame_id", "coordinate_space", "shape"],
    );
    add("remove_manual_mask", json!({"ref":text(64)}), &["ref"]);
    let local: serde_json::Map<_, _> = LOCAL_RANGES.iter().map(|&(id, lo, hi)| (id.into(), number(lo, hi))).collect();
    add(
        "set_mask_controls",
        json!({"ref":text(64),"values":{"type":"object","properties":local,"minProperties":1,"additionalProperties":false}}),
        &["ref", "values"],
    );
    Value::Object(schemas)
}

pub(super) fn validate_nested(op: &Value) -> Result<(), String> {
    if let Some(v) = op.get("rect") {
        closed(v, &["x0", "y0", "x1", "y1"])?;
    }
    for (key, fields) in [
        ("points", &["x", "y"][..]),
        ("colors", &["lum", "chroma", "hue", "hue_shift", "sat_shift", "lum_shift", "variance", "range", "hue_range", "sat_range", "lum_range"][..]),
    ] {
        if let Some(values) = op.get(key).and_then(Value::as_array) {
            for v in values {
                closed(v, fields)?;
            }
        }
    }
    if let Some(shape) = op.get("shape") {
        let fields = match shape.get("kind").and_then(Value::as_str) {
            Some("linear") => vec!["kind", "start", "end"],
            Some("radial") => vec!["kind", "center", "rx", "ry", "angle", "feather", "invert"],
            Some("brush") => vec!["kind", "strokes"],
            Some("luminanceRange") => vec!["kind", "lo", "hi", "lo_feather", "hi_feather"],
            Some("colorRange") => vec!["kind", "samples", "refine"],
            _ => return Err("未开放蒙版类型。".into()),
        };
        closed(shape, &fields)?;
        for field in ["start", "end", "center"] {
            if let Some(v) = shape.get(field) {
                closed(v, &["x", "y"])?;
            }
        }
        if let Some(strokes) = shape.get("strokes").and_then(Value::as_array) {
            for stroke in strokes {
                closed(stroke, &["points", "size", "feather", "flow", "density", "erase", "auto_mask"])?;
                validate_nested(stroke)?;
            }
        }
    }
    Ok(())
}
