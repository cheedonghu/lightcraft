//! Shared local adjustment ranges for UI commands and strict companion candidates.
use crate::LocalAdjustments;

pub const LOCAL_RANGES: &[(&str, f64, f64)] = &[
    ("exposure", -4.0, 4.0),
    ("temp", -100.0, 100.0),
    ("tint", -100.0, 100.0),
    ("contrast", -100.0, 100.0),
    ("highlights", -100.0, 100.0),
    ("shadows", -100.0, 100.0),
    ("whites", -100.0, 100.0),
    ("blacks", -100.0, 100.0),
    ("texture", -100.0, 100.0),
    ("clarity", -100.0, 100.0),
    ("dehaze", -100.0, 100.0),
    ("hue", -100.0, 100.0),
    ("saturation", -100.0, 100.0),
    ("sharpness", -100.0, 100.0),
    ("noise", -100.0, 100.0),
    ("moire", -100.0, 100.0),
    ("defringe", -100.0, 100.0),
    ("color_hue", 0.0, 360.0),
    ("color_sat", 0.0, 100.0),
    ("amount", 0.0, 200.0),
];

pub fn clamp(a: LocalAdjustments) -> LocalAdjustments {
    let mut value = serde_json::to_value(a).unwrap_or_default();
    for &(name, lo, hi) in LOCAL_RANGES {
        let n = value.get(name).and_then(serde_json::Value::as_f64).filter(|n| n.is_finite()).unwrap_or(0.0).clamp(lo, hi);
        value[name] = serde_json::json!(n);
    }
    serde_json::from_value(value).unwrap_or_default()
}
