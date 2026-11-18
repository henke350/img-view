//! Zoom and pan: how big the image is drawn and where.

use egui::{Pos2, Vec2};

pub const MIN_SCALE: f32 = 0.02;
pub const MAX_SCALE: f32 = 32.0;
/// Each zoom step multiplies or divides the scale by this.
const STEP: f32 = 1.25;

/// How the image is sized in the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    /// Shrink to fit the window; small images stay at 100%.
    Fit,
    /// A fixed scale, where 1.0 is one image pixel per screen point.
    Scale(f32),
}

/// The zoom state plus how far the image is panned from the centre of the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub zoom: Zoom,
    pub pan: Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: Zoom::Fit, pan: Vec2::ZERO }
    }
}

impl View {
    /// The scale actually used for an image of `image` size in an `area`.
    pub fn scale(&self, image: Vec2, area: Vec2) -> f32 {
        match self.zoom {
            Zoom::Fit => fit_scale(image, area),
            Zoom::Scale(s) => s,
        }
    }

    /// Zooms `steps` steps in (positive) or out (negative), keeping the image point under
    /// `anchor` in place.
    pub fn zoom_by(&mut self, steps: f32, image: Vec2, area_center: Pos2, area: Vec2, anchor: Pos2) {
        let old = self.scale(image, area);
        let new = (old * STEP.powf(steps)).clamp(MIN_SCALE, MAX_SCALE);
        self.set_scale(new, image, area_center, area, anchor);
    }

    /// Sets an exact scale, keeping the image point under `anchor` in place.
    pub fn set_scale(&mut self, new: f32, image: Vec2, area_center: Pos2, area: Vec2, anchor: Pos2) {
        let old = self.scale(image, area);
        let center = area_center + self.pan;
        let new_center = anchor - (anchor - center) * (new / old);
        self.zoom = Zoom::Scale(new);
        self.pan = new_center - area_center;
        self.clamp_pan(image, area);
    }

    /// Moves the image by `delta` screen points, without letting it leave the window.
    pub fn pan_by(&mut self, delta: Vec2, image: Vec2, area: Vec2) {
        self.pan += delta;
        self.clamp_pan(image, area);
    }

    /// Keeps the image centred on an axis where it fits, and stops it being dragged out of
    /// view where it does not.
    pub fn clamp_pan(&mut self, image: Vec2, area: Vec2) {
        let shown = image * self.scale(image, area);
        let slack = ((shown - area) * 0.5).max(Vec2::ZERO);
        self.pan = self.pan.clamp(-slack, slack);
    }
}

/// The largest scale, at most 1.0, at which `image` fits inside `area`.
pub fn fit_scale(image: Vec2, area: Vec2) -> f32 {
    if image.x <= 0.0 || image.y <= 0.0 {
        return 1.0;
    }
    (area.x / image.x).min(area.y / image.y).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Vec2 = Vec2::new(800.0, 600.0);
    const CENTER: Pos2 = Pos2::new(400.0, 300.0);

    #[test]
    fn fit_scale_should_shrink_large_image() {
        assert_eq!(fit_scale(Vec2::new(1600.0, 1000.0), AREA), 0.5);
    }

    #[test]
    fn fit_scale_should_not_enlarge_small_image() {
        assert_eq!(fit_scale(Vec2::new(100.0, 100.0), AREA), 1.0);
    }

    #[test]
    fn zoom_by_one_step_in_should_enlarge_by_step() {
        let mut view = View { zoom: Zoom::Scale(1.0), pan: Vec2::ZERO };
        view.zoom_by(1.0, Vec2::new(4000.0, 3000.0), CENTER, AREA, CENTER);
        assert_eq!(view.zoom, Zoom::Scale(1.25));
    }

    #[test]
    fn zoom_by_should_start_from_fit_scale() {
        let mut view = View::default();
        view.zoom_by(-1.0, Vec2::new(1600.0, 1000.0), CENTER, AREA, CENTER);
        assert_eq!(view.zoom, Zoom::Scale(0.4));
    }

    #[test]
    fn zoom_by_should_stop_at_max_scale() {
        let mut view = View { zoom: Zoom::Scale(30.0), pan: Vec2::ZERO };
        view.zoom_by(5.0, Vec2::new(100.0, 100.0), CENTER, AREA, CENTER);
        assert_eq!(view.zoom, Zoom::Scale(MAX_SCALE));
    }

    #[test]
    fn set_scale_should_keep_point_under_anchor_fixed() {
        // A 2000x2000 image at 1:1, centred. The anchor sits 100 points right of centre.
        let image = Vec2::splat(2000.0);
        let mut view = View { zoom: Zoom::Scale(1.0), pan: Vec2::ZERO };
        let anchor = CENTER + Vec2::new(100.0, 0.0);
        view.set_scale(2.0, image, CENTER, AREA, anchor);
        // Image point under anchor was 100 right of the image centre; at 2x it is 200 right,
        // so the centre must move 100 left to keep it under the anchor.
        assert_eq!(view.pan, Vec2::new(-100.0, 0.0));
    }

    #[test]
    fn clamp_pan_should_centre_image_that_fits() {
        let mut view = View { zoom: Zoom::Scale(1.0), pan: Vec2::new(50.0, -20.0) };
        view.clamp_pan(Vec2::new(100.0, 100.0), AREA);
        assert_eq!(view.pan, Vec2::ZERO);
    }

    #[test]
    fn pan_by_should_stop_at_image_edge() {
        // 1000 wide at 1:1 in an 800 wide area: 100 points of slack on each side.
        let mut view = View { zoom: Zoom::Scale(1.0), pan: Vec2::ZERO };
        view.pan_by(Vec2::new(500.0, 0.0), Vec2::new(1000.0, 600.0), AREA);
        assert_eq!(view.pan, Vec2::new(100.0, 0.0));
    }
}
