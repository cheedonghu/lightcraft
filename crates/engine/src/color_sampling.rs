//! Pure pixel probes shared by interactive tools and the photo-bound agent.
use lightcraft_color::perceptual::{lab_to_lch, oklab_from_2020};
use lightcraft_color::transfer::srgb_to_linear;
use lightcraft_color::{REC2020, SRGB};
use lightcraft_develop::{DevelopSettings, Treatment};
use lightcraft_geom::Point;
use lightcraft_pipeline::{RenderRequest, SourceInfo};
use lightcraft_raster::Rgb32f;

pub fn probe(src: &Rgb32f, info: &SourceInfo, settings: &DevelopSettings, p: Point, neutral: impl FnOnce(&mut DevelopSettings)) -> [f32; 3] {
    let mut d = settings.clone();
    neutral(&mut d);
    let req = RenderRequest::fit(384, 384);
    let plan = lightcraft_pipeline::plan(src, info, &d, &req);
    let q = plan.frame.norm_to_out(plan.w, plan.h).apply(p);
    let img = lightcraft_pipeline::render(src, info, &d, &req).image;
    let (cx, cy) = (q.x.floor() as isize, q.y.floor() as isize);
    let mut acc = [0.0; 3];
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (px, py) = (
                (cx + dx).clamp(0, img.width.saturating_sub(1) as isize) as usize,
                (cy + dy).clamp(0, img.height.saturating_sub(1) as isize) as usize,
            );
            let pixel = img.get(px, py);
            for (channel, value) in acc.iter_mut().enumerate() {
                *value += f32::from(pixel.get(channel).copied().unwrap_or(0)) / 255.0 / 9.0;
            }
        }
    }
    acc
}
pub fn encoded_to_oklch(e: [f32; 3]) -> [f32; 3] {
    let lin = SRGB.to_space(&REC2020).apply_f32(e.map(srgb_to_linear));
    lab_to_lch(oklab_from_2020(lin))
}
pub fn after_mixer_neutral(d: &mut DevelopSettings) {
    d.color.vibrance = 0.0;
    d.color.saturation = 0.0;
    d.point_colors.clear();
    d.treatment = Treatment::Color;
    d.grading = Default::default();
    d.vignette.amount = 0.0;
    d.grain.amount = 0.0;
    d.curve = Default::default();
}
