//! The drag selection that Copy and Crop work on: drawing it, then moving it and
//! resizing it by its edges and corners.
//!
//! Everything here is in image pixel coordinates, except `grip_at`, which works on
//! the selection as drawn on screen so the grab distance stays the same at any zoom.

use egui::{CursorIcon, Pos2, Rect, Vec2};

use crate::edit::PixelRect;

/// How close the pointer must be to an edge (in screen points) to grab it.
pub const GRAB_DISTANCE: f32 = 8.0;

/// A rectangle between two corners, in image pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub start: Pos2,
    pub end: Pos2,
}

impl Selection {
    pub fn rect(self) -> Rect {
        Rect::from_two_pos(self.start, self.end)
    }

    pub fn to_pixel_rect(self) -> PixelRect {
        let r = self.rect();
        // Coordinates are already clamped to the image, so they are non-negative.
        let (x, y) = (r.min.x.round() as u32, r.min.y.round() as u32);
        PixelRect {
            x,
            y,
            width: (r.max.x.round() as u32).saturating_sub(x),
            height: (r.max.y.round() as u32).saturating_sub(y),
        }
    }

    pub fn is_empty(self) -> bool {
        let r = self.to_pixel_rect();
        r.width == 0 || r.height == 0
    }

    /// The same rectangle with `start` top-left and `end` bottom-right.
    fn normalized(self) -> Self {
        let r = self.rect();
        Self { start: r.min, end: r.max }
    }
}

/// Which part of an existing selection the pointer is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grip {
    /// Inside: dragging moves the whole selection.
    Move,
    /// On one edge, or on a corner (two edges): dragging moves those edges.
    Sides { left: bool, right: bool, top: bool, bottom: bool },
}

impl Grip {
    /// The mouse pointer that tells the user what a drag will do.
    pub fn cursor(self) -> CursorIcon {
        match self {
            Grip::Move => CursorIcon::Move,
            Grip::Sides { left, right, top, bottom } => {
                let horizontal = left || right;
                let vertical = top || bottom;
                if horizontal && vertical {
                    if (left && top) || (right && bottom) { CursorIcon::ResizeNwSe } else { CursorIcon::ResizeNeSw }
                } else if horizontal {
                    CursorIcon::ResizeHorizontal
                } else {
                    CursorIcon::ResizeVertical
                }
            }
        }
    }
}

/// The part of `on_screen` (the selection as drawn) under the pointer `p`, if any.
/// When the selection is so small that both edges are in reach, the nearer one wins.
pub fn grip_at(on_screen: Rect, p: Pos2) -> Option<Grip> {
    if !on_screen.expand(GRAB_DISTANCE).contains(p) {
        return None;
    }
    let (to_left, to_right) = ((p.x - on_screen.left()).abs(), (p.x - on_screen.right()).abs());
    let (to_top, to_bottom) = ((p.y - on_screen.top()).abs(), (p.y - on_screen.bottom()).abs());
    let left = to_left <= GRAB_DISTANCE && to_left <= to_right;
    let right = !left && to_right <= GRAB_DISTANCE;
    let top = to_top <= GRAB_DISTANCE && to_top <= to_bottom;
    let bottom = !top && to_bottom <= GRAB_DISTANCE;
    if left || right || top || bottom {
        Some(Grip::Sides { left, right, top, bottom })
    } else if on_screen.contains(p) {
        Some(Grip::Move)
    } else {
        None
    }
}

/// A move or resize in progress: what was grabbed, and the selection and pointer
/// (image coordinates) when the drag began.
#[derive(Clone, Copy, Debug)]
pub struct Adjust {
    pub grip: Grip,
    from: Selection,
    anchor: Pos2,
}

impl Adjust {
    pub fn begin(grip: Grip, selection: Selection, anchor: Pos2) -> Self {
        Self { grip, from: selection.normalized(), anchor }
    }

    /// The selection with the pointer now at `pointer` (image coordinates, not clamped),
    /// kept inside an image of size `bounds`. Edges follow the pointer by how far it has
    /// moved, so grabbing slightly beside an edge doesn't make it jump. Dragging an edge
    /// past the opposite one simply flips the selection.
    pub fn update(&self, pointer: Pos2, bounds: Vec2) -> Selection {
        let delta = pointer - self.anchor;
        let Selection { mut start, mut end } = self.from;
        match self.grip {
            Grip::Move => {
                let size = end - start;
                let max_start = (bounds - size).max(Vec2::ZERO).to_pos2();
                let start = (start + delta).clamp(Pos2::ZERO, max_start);
                Selection { start, end: start + size }
            }
            Grip::Sides { left, right, top, bottom } => {
                if left {
                    start.x = (start.x + delta.x).clamp(0.0, bounds.x);
                }
                if right {
                    end.x = (end.x + delta.x).clamp(0.0, bounds.x);
                }
                if top {
                    start.y = (start.y + delta.y).clamp(0.0, bounds.y);
                }
                if bottom {
                    end.y = (end.y + delta.y).clamp(0.0, bounds.y);
                }
                Selection { start, end }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(x0: f32, y0: f32, x1: f32, y1: f32) -> Selection {
        Selection { start: Pos2::new(x0, y0), end: Pos2::new(x1, y1) }
    }

    fn sides(left: bool, right: bool, top: bool, bottom: bool) -> Grip {
        Grip::Sides { left, right, top, bottom }
    }

    const SCREEN: Rect = Rect { min: Pos2::new(100.0, 100.0), max: Pos2::new(300.0, 200.0) };
    const BOUNDS: Vec2 = Vec2::new(400.0, 300.0);

    #[test]
    fn grip_at_should_find_move_inside() {
        assert_eq!(grip_at(SCREEN, Pos2::new(200.0, 150.0)), Some(Grip::Move));
    }

    #[test]
    fn grip_at_should_find_nothing_far_outside() {
        assert_eq!(grip_at(SCREEN, Pos2::new(50.0, 150.0)), None);
    }

    #[test]
    fn grip_at_should_find_edge_just_outside() {
        assert_eq!(grip_at(SCREEN, Pos2::new(305.0, 150.0)), Some(sides(false, true, false, false)));
    }

    #[test]
    fn grip_at_should_find_corner() {
        assert_eq!(grip_at(SCREEN, Pos2::new(102.0, 198.0)), Some(sides(true, false, false, true)));
    }

    #[test]
    fn grip_at_should_pick_nearer_edge_when_selection_is_tiny() {
        let tiny = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(104.0, 200.0));
        assert_eq!(grip_at(tiny, Pos2::new(105.0, 150.0)), Some(sides(false, true, false, false)));
    }

    #[test]
    fn cursor_should_match_corner_direction() {
        assert_eq!(sides(true, false, true, false).cursor(), CursorIcon::ResizeNwSe);
        assert_eq!(sides(false, true, true, false).cursor(), CursorIcon::ResizeNeSw);
        assert_eq!(sides(false, false, false, true).cursor(), CursorIcon::ResizeVertical);
    }

    #[test]
    fn move_should_shift_selection_by_pointer_delta() {
        let adjust = Adjust::begin(Grip::Move, sel(10.0, 10.0, 50.0, 30.0), Pos2::new(20.0, 20.0));
        assert_eq!(adjust.update(Pos2::new(25.0, 40.0), BOUNDS), sel(15.0, 30.0, 55.0, 50.0));
    }

    #[test]
    fn move_should_stop_at_image_edge_and_keep_size() {
        let adjust = Adjust::begin(Grip::Move, sel(10.0, 10.0, 50.0, 30.0), Pos2::new(20.0, 20.0));
        assert_eq!(adjust.update(Pos2::new(-500.0, 1000.0), BOUNDS), sel(0.0, 280.0, 40.0, 300.0));
    }

    #[test]
    fn edge_drag_should_move_only_that_edge() {
        let adjust = Adjust::begin(sides(false, true, false, false), sel(10.0, 10.0, 50.0, 30.0), Pos2::new(52.0, 20.0));
        assert_eq!(adjust.update(Pos2::new(82.0, 90.0), BOUNDS), sel(10.0, 10.0, 80.0, 30.0));
    }

    #[test]
    fn corner_drag_should_work_on_a_selection_drawn_backwards() {
        // Drawn from bottom-right to top-left; the top-left corner is still `left` + `top`.
        let adjust = Adjust::begin(sides(true, false, true, false), sel(50.0, 30.0, 10.0, 10.0), Pos2::new(10.0, 10.0));
        assert_eq!(adjust.update(Pos2::new(5.0, -20.0), BOUNDS), sel(5.0, 0.0, 50.0, 30.0));
    }

    #[test]
    fn edge_drag_past_opposite_edge_should_flip_not_vanish() {
        let adjust = Adjust::begin(sides(true, false, false, false), sel(10.0, 10.0, 50.0, 30.0), Pos2::new(10.0, 20.0));
        let flipped = adjust.update(Pos2::new(70.0, 20.0), BOUNDS);
        assert_eq!(flipped.to_pixel_rect(), PixelRect { x: 50, y: 10, width: 20, height: 20 });
    }
}
