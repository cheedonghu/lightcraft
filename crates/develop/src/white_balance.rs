//! Pure source-aware white-balance representation normalization shared by rendering and catalog.

use lightcraft_color::cct::{temp_tint_to_xy, xy_to_temp_tint};

use crate::{WbMode, WbScale, WhiteBalance};

/// Source facts needed to interpret a stored WB without decoding or depending on the engine.
#[derive(Clone, Copy, Debug)]
pub struct WbReference {
    /// This source edits in absolute Kelvin (raw with known colour matrices).
    pub kelvin: bool,
    /// Its Legacy values were written relative to the camera's as-shot look.
    pub legacy_relative: bool,
    pub as_shot: (f64, f64),
}

impl WhiteBalance {
    /// Canonical representation on the source's current editing scale. This changes only the
    /// representation: AsShot still reads its reference, and a Legacy preset converted to Kelvin
    /// retains its previous effective look as Custom, just like the renderer's control writes.
    pub fn normalized(mut self, reference: WbReference) -> Self {
        if !reference.kelvin {
            self.scale = WbScale::Legacy;
            return self;
        }
        if reference.legacy_relative && self.scale == WbScale::Legacy && self.mode != WbMode::AsShot {
            (self.temp, self.tint) = legacy_to_kelvin(self.temp, self.tint, reference.as_shot);
            self.mode = WbMode::Custom;
        }
        self.scale = WbScale::Kelvin;
        self
    }
}

/// The white that renders exactly as `(temp, tint)` did on the relative scale, where 6500 K / 0 meant *as shot* and
/// the source's real as-shot white is `as_shot`.
///
/// The relative render adapted the picture from 6500 K / 0 to `(temp, tint)` (Bradford, [`lightcraft_color::cct::wb_matrix`]); the
/// Kelvin render adapts it from `as_shot` to the result. Bradford adaptations are von Kries scalings in one cone
/// space, so they compose exactly: the new white's cone response is the old target's times the as-shot white's
/// over the old reference's, and the two renders give the same matrix (up to the scale the luminance
/// normalisation removes). `None` when the arithmetic has nowhere to go (a non-finite input, a white outside the
/// chromaticity plane).
pub fn legacy_white(temp: f64, tint: f64, as_shot: (f64, f64)) -> Option<lightcraft_color::Xy> {
    use lightcraft_color::{BRADFORD, Xy};
    if !(temp.is_finite() && tint.is_finite() && as_shot.0.is_finite() && as_shot.1.is_finite()) {
        return None;
    }
    let cones = |t: f64, tint: f64| BRADFORD.apply(temp_tint_to_xy(t, tint).to_xyz());
    let (old, reference, shot) = (cones(temp, tint), cones(6500.0, 0.0), cones(as_shot.0, as_shot.1));
    let inverse = BRADFORD.inverse()?;
    let mut new = [0.0; 3];
    for i in 0..3 {
        if reference[i].abs() < 1e-12 {
            return None;
        }
        new[i] = old[i] * shot[i] / reference[i];
    }
    let xy = Xy::from_xyz(inverse.apply(new));
    (xy.x.is_finite() && xy.y.is_finite() && xy.y > 0.0).then_some(xy)
}

/// The Kelvin / tint of [`legacy_white`]: what a white balance written on the relative scale reads as, and becomes
/// when it is next edited ([`WhiteBalance::normalized`]). Exact wherever the white has a Kelvin value (1000–50000 K after the
/// composition; the relative slider reached 2000–13550 K around 6500 K, so only a far-blue as-shot white pushed
/// to the slider's blue end lands beyond, and clamps), to the precision of the Kelvin / tint parametrisation
/// (better than 0.01 % away from its 4000 K seam). Unchanged when the arithmetic has nowhere to go.
pub fn legacy_to_kelvin(temp: f64, tint: f64, as_shot: (f64, f64)) -> (f64, f64) {
    if temp == 6500.0 && tint == 0.0 && as_shot.0.is_finite() && as_shot.1.is_finite() {
        // the relative scale's as-shot reference, exactly (a Custom value left where As Shot was stays as shot)
        return as_shot;
    }
    match legacy_white(temp, tint, as_shot).map(xy_to_temp_tint) {
        Some((t, tint)) if t.is_finite() && tint.is_finite() => (t, tint),
        _ => (temp, tint),
    }
}
