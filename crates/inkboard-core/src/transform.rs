//! Handle math: resizing, rotating, snapping. Tested outside the interface.

use crate::geom::{Frame, Pt, Rect};
use crate::model::Object;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Handle {
    pub const ALL: [Handle; 8] = [Handle::NW, Handle::N, Handle::NE, Handle::E, Handle::SE, Handle::S, Handle::SW, Handle::W];
    pub fn is_corner(self) -> bool {
        matches!(self, Handle::NE | Handle::SE | Handle::SW | Handle::NW)
    }
    fn dirs(self) -> (i8, i8) {
        match self {
            Handle::N => (0, -1),
            Handle::NE => (1, -1),
            Handle::E => (1, 0),
            Handle::SE => (1, 1),
            Handle::S => (0, 1),
            Handle::SW => (-1, 1),
            Handle::W => (-1, 0),
            Handle::NW => (-1, -1),
        }
    }
    /// Handle position in the local frame (origin at the center).
    pub fn local(self, w: f64, h: f64) -> Pt {
        let (dx, dy) = self.dirs();
        Pt::new(f64::from(dx) * w / 2.0, f64::from(dy) * h / 2.0)
    }
}

/// New frame after dragging handle `h` to the world point `pointer`.
/// The opposite side or corner stays fixed; also works for a rotated frame.
pub fn resize_frame(orig: &Frame, h: Handle, pointer: Pt, keep_ratio: bool, min: f64) -> Frame {
    let p = orig.to_local(pointer);
    let (dx, dy) = h.dirs();
    let (mut left, mut right, mut top, mut bottom) = (-orig.w / 2.0, orig.w / 2.0, -orig.h / 2.0, orig.h / 2.0);
    if dx > 0 {
        right = p.x.max(left + min);
    } else if dx < 0 {
        left = p.x.min(right - min);
    }
    if dy > 0 {
        bottom = p.y.max(top + min);
    } else if dy < 0 {
        top = p.y.min(bottom - min);
    }
    let (mut nw, mut nh) = (right - left, bottom - top);
    if keep_ratio && h.is_corner() && orig.w > 0.0 && orig.h > 0.0 {
        let k = (nw / orig.w).max(nh / orig.h);
        nw = orig.w * k;
        nh = orig.h * k;
        if dx > 0 {
            right = left + nw
        } else {
            left = right - nw
        }
        if dy > 0 { bottom = top + nh } else { top = bottom - nh }
    }
    let c = Frame { angle: orig.angle, ..*orig }.to_world(Pt::new((left + right) / 2.0, (top + bottom) / 2.0));
    Frame { x: c.x - nw / 2.0, y: c.y - nh / 2.0, w: nw, h: nh, angle: orig.angle }
}

/// Same operation for a non-rotated box (multiple selection).
pub fn resize_rect(orig: Rect, h: Handle, pointer: Pt, keep_ratio: bool, min: f64) -> Rect {
    resize_frame(&Frame::from_rect(orig), h, pointer, keep_ratio, min).rect()
}

/// Resizes a set of objects from a box `from` to a box `to`.
pub fn scale_objects(objs: &mut [Object], from: Rect, to: Rect) {
    let (sx, sy) = (to.w / from.w.max(1e-9), to.h / from.h.max(1e-9));
    for o in objs {
        // scale around the top-left corner of the old box, then translate
        o.scale_about(Pt::new(from.x, from.y), sx, sy);
        o.translate(Pt::new(to.x - from.x, to.y - from.y));
    }
}

/// Angle such that the rotation handle (above the frame) follows the pointer.
pub fn rotation_toward(center: Pt, pointer: Pt) -> f64 {
    (pointer.y - center.y).atan2(pointer.x - center.x) + std::f64::consts::FRAC_PI_2
}

/// Rounds an angle to the nearest multiple of `step`.
pub fn snap_angle(a: f64, step: f64) -> f64 {
    (a / step).round() * step
}

/// Snaps a point to a grid.
pub fn snap_point(p: Pt, grid: f64) -> Pt {
    Pt::new((p.x / grid).round() * grid, (p.y / grid).round() * grid)
}

/// Constrains a move to an angle that is a multiple of `step` (e.g. 15°) from `from`.
pub fn constrain_angle(from: Pt, to: Pt, step: f64) -> Pt {
    let d = to - from;
    let a = snap_angle(d.y.atan2(d.x), step);
    let l = d.len();
    Pt::new(from.x + l * a.cos(), from.y + l * a.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_corner_keeps_opposite_corner_fixed() {
        let f = Frame::new(100.0, 100.0, 200.0, 100.0);
        let n = resize_frame(&f, Handle::SE, Pt::new(500.0, 400.0), false, 4.0);
        assert_eq!((n.x, n.y, n.w, n.h), (100.0, 100.0, 400.0, 300.0));
        let n = resize_frame(&f, Handle::NW, Pt::new(50.0, 60.0), false, 4.0);
        assert_eq!((n.x, n.y, n.w, n.h), (50.0, 60.0, 250.0, 140.0));
    }

    #[test]
    fn resize_edge_changes_one_axis_only() {
        let f = Frame::new(0.0, 0.0, 100.0, 50.0);
        let n = resize_frame(&f, Handle::E, Pt::new(300.0, 999.0), false, 4.0);
        assert_eq!((n.w, n.h, n.x, n.y), (300.0, 50.0, 0.0, 0.0));
        let n = resize_frame(&f, Handle::N, Pt::new(-999.0, -30.0), false, 4.0);
        assert_eq!((n.w, n.h, n.y), (100.0, 80.0, -30.0));
    }

    #[test]
    fn minimum_size_is_respected_and_no_flip() {
        let f = Frame::new(0.0, 0.0, 100.0, 50.0);
        let n = resize_frame(&f, Handle::E, Pt::new(-500.0, 0.0), false, 8.0);
        assert_eq!(n.w, 8.0);
        assert_eq!(n.x, 0.0);
    }

    #[test]
    fn keep_ratio_on_corner() {
        let f = Frame::new(0.0, 0.0, 200.0, 100.0);
        let n = resize_frame(&f, Handle::SE, Pt::new(400.0, 120.0), true, 4.0);
        assert!((n.w / n.h - 2.0).abs() < 1e-9);
        assert_eq!((n.x, n.y), (0.0, 0.0));
    }

    #[test]
    fn rotated_frame_resizes_along_its_own_axes() {
        let f = Frame { x: 0.0, y: 0.0, w: 100.0, h: 50.0, angle: std::f64::consts::FRAC_PI_2 };
        // rotated by 90°: the local x axis points down; dragging E to (cx, cy + 100) lengthens the width
        let c = f.center();
        let n = resize_frame(&f, Handle::E, Pt::new(c.x, c.y + 100.0), false, 4.0);
        assert!((n.w - 150.0).abs() < 1e-9, "w = {}", n.w);
        assert!((n.h - 50.0).abs() < 1e-9);
        // the opposite edge (local west = top on screen) has not moved
        let west_before = f.to_world(Pt::new(-50.0, 0.0));
        let west_after = n.to_world(Pt::new(-n.w / 2.0, 0.0));
        assert!(west_before.dist(west_after) < 1e-9);
    }

    #[test]
    fn scale_objects_maps_between_boxes() {
        use crate::model::{BoxGeom, Label};
        use crate::style::Style;
        let mut objs = vec![Object::new_box(BoxGeom::Rect, Frame::new(10.0, 10.0, 50.0, 50.0), Style::default(), Label::default())];
        scale_objects(&mut objs, Rect::new(0.0, 0.0, 100.0, 100.0), Rect::new(0.0, 0.0, 200.0, 300.0));
        let f = objs[0].frame().unwrap();
        assert_eq!((f.x, f.y, f.w, f.h), (20.0, 30.0, 100.0, 150.0));
    }

    #[test]
    fn rotation_handle_math() {
        let c = Pt::new(0.0, 0.0);
        assert!(rotation_toward(c, Pt::new(0.0, -100.0)).abs() < 1e-9, "handle above = zero angle");
        assert!((rotation_toward(c, Pt::new(100.0, 0.0)) - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!((snap_angle(0.30, std::f64::consts::PI / 12.0) - std::f64::consts::PI / 12.0).abs() < 1e-9);
    }

    #[test]
    fn snapping_helpers() {
        assert_eq!(snap_point(Pt::new(37.0, 61.0), 24.0), Pt::new(48.0, 72.0));
        let p = constrain_angle(Pt::ZERO, Pt::new(100.0, 10.0), std::f64::consts::PI / 12.0);
        assert!(p.y.abs() < 1e-6 && (p.x - Pt::new(100.0, 10.0).len()).abs() < 1e-9);
    }
}
