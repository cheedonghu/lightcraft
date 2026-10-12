//! Strict companion parameter limits; ordinary mask commands retain upstream clamping.

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
