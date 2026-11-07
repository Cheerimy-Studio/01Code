//! 原生小图标 — 全部矢量绘制,无字体/图片依赖
#![allow(dead_code)]

use eframe::egui::{Color32, Painter, Pos2, Rect, Rounding, Shape, Stroke, Vec2};

pub fn folder(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let w = s;
    let h = s * 0.75;
    let r = Rect::from_center_size(c, Vec2::new(w, h));
    let tab = Rect::from_min_max(
        Pos2::new(r.left(), r.top()),
        Pos2::new(r.left() + w * 0.42, r.top() + h * 0.32),
    );
    p.add(Shape::rect_filled(tab, Rounding::same(2.0), color));
    p.add(Shape::rect_filled(r, Rounding::same(2.5), color));
}

pub fn search(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let r = s * 0.32;
    let cc = Pos2::new(c.x - s * 0.12, c.y - s * 0.12);
    p.add(Shape::circle_stroke(cc, r, Stroke::new(1.6, color)));
    p.add(Shape::line_segment(
        [
            Pos2::new(cc.x + r * 0.72, cc.y + r * 0.72),
            Pos2::new(c.x + s * 0.38, c.y + s * 0.38),
        ],
        Stroke::new(1.6, color),
    ));
}

pub fn grid4(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let cell = s * 0.36;
    let gap = s * 0.14;
    for dx in [-1.0f32, 1.0] {
        for dy in [-1.0f32, 1.0] {
            let cc = Pos2::new(c.x + dx * (cell + gap) * 0.5, c.y + dy * (cell + gap) * 0.5);
            let r = Rect::from_center_size(cc, Vec2::splat(cell));
            p.add(Shape::rect_filled(r, Rounding::same(1.5), color));
        }
    }
}

pub fn clock(p: &Painter, c: Pos2, s: f32, color: Color32) {
    p.add(Shape::circle_stroke(c, s * 0.42, Stroke::new(1.6, color)));
    p.add(Shape::line_segment(
        [c, Pos2::new(c.x, c.y - s * 0.26)],
        Stroke::new(1.5, color),
    ));
    p.add(Shape::line_segment(
        [c, Pos2::new(c.x + s * 0.2, c.y + s * 0.08)],
        Stroke::new(1.5, color),
    ));
}

pub fn gear(p: &Painter, c: Pos2, s: f32, color: Color32) {
    p.add(Shape::circle_stroke(c, s * 0.26, Stroke::new(1.6, color)));
    for k in 0..8u32 {
        let a = k as f32 * std::f32::consts::TAU / 8.0;
        let (dx, dy) = (a.cos(), a.sin());
        p.add(Shape::line_segment(
            [
                Pos2::new(c.x + dx * s * 0.31, c.y + dy * s * 0.31),
                Pos2::new(c.x + dx * s * 0.44, c.y + dy * s * 0.44),
            ],
            Stroke::new(1.5, color),
        ));
    }
    p.add(Shape::circle_filled(c, s * 0.08, color));
}

pub fn sliders(p: &Painter, c: Pos2, s: f32, color: Color32) {
    for (i, dy) in [-0.28f32, 0.0, 0.28].iter().enumerate() {
        let y = c.y + dy * s;
        p.add(Shape::line_segment(
            [Pos2::new(c.x - s * 0.42, y), Pos2::new(c.x + s * 0.42, y)],
            Stroke::new(1.5, color),
        ));
        let x = match i {
            0 => c.x + s * 0.16,
            1 => c.x - s * 0.2,
            _ => c.x + s * 0.3,
        };
        p.add(Shape::circle_filled(Pos2::new(x, y), s * 0.1, color));
    }
}

pub fn plus(p: &Painter, c: Pos2, s: f32, color: Color32) {
    p.add(Shape::line_segment(
        [Pos2::new(c.x - s * 0.4, c.y), Pos2::new(c.x + s * 0.4, c.y)],
        Stroke::new(1.8, color),
    ));
    p.add(Shape::line_segment(
        [Pos2::new(c.x, c.y - s * 0.4), Pos2::new(c.x, c.y + s * 0.4)],
        Stroke::new(1.8, color),
    ));
}

/// 四角星/菱形 — 工具调用行图标
pub fn sparkle(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let r = s * 0.42;
    let pts = vec![
        Pos2::new(c.x, c.y - r),
        Pos2::new(c.x + r * 0.62, c.y - r * 0.62),
        Pos2::new(c.x + r, c.y),
        Pos2::new(c.x + r * 0.62, c.y + r * 0.62),
        Pos2::new(c.x, c.y + r),
        Pos2::new(c.x - r * 0.62, c.y + r * 0.62),
        Pos2::new(c.x - r, c.y),
        Pos2::new(c.x - r * 0.62, c.y - r * 0.62),
    ];
    p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

pub fn doc(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let r = Rect::from_center_size(c, Vec2::new(s * 0.62, s * 0.8));
    p.add(Shape::rect_filled(r, Rounding::same(2.0), color));
    p.add(Shape::line_segment(
        [Pos2::new(r.left() + s * 0.14, c.y - s * 0.12), Pos2::new(r.right() - s * 0.12, c.y - s * 0.12)],
        Stroke::new(1.2, Color32::WHITE),
    ));
    p.add(Shape::line_segment(
        [Pos2::new(r.left() + s * 0.14, c.y + s * 0.02), Pos2::new(r.right() - s * 0.12, c.y + s * 0.02)],
        Stroke::new(1.2, Color32::WHITE),
    ));
    p.add(Shape::line_segment(
        [Pos2::new(r.left() + s * 0.14, c.y + s * 0.16), Pos2::new(r.right() - s * 0.12, c.y + s * 0.16)],
        Stroke::new(1.2, Color32::WHITE),
    ));
}

/// 复制图标(双矩形)
pub fn copy(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let w = s * 0.48;
    let back = Rect::from_min_size(
        Pos2::new(c.x - s * 0.36, c.y - s * 0.36),
        Vec2::new(w, w),
    );
    let front = Rect::from_min_size(
        Pos2::new(c.x - s * 0.12, c.y - s * 0.12),
        Vec2::new(w, w),
    );
    p.add(Shape::rect_stroke(
        back,
        Rounding::same(1.8),
        Stroke::new(1.3, color),
    ));
    p.add(Shape::rect_stroke(
        front,
        Rounding::same(1.8),
        Stroke::new(1.3, color),
    ));
}

/// 三点更多
pub fn more(p: &Painter, c: Pos2, s: f32, color: Color32) {
    for dx in [-0.32f32, 0.0, 0.32] {
        p.add(Shape::circle_filled(
            Pos2::new(c.x + dx * s, c.y),
            s * 0.09,
            color,
        ));
    }
}

/// 下拉箭头
pub fn chevron_down(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let w = s * 0.34;
    p.add(Shape::line_segment(
        [
            Pos2::new(c.x - w, c.y - w * 0.5),
            Pos2::new(c.x, c.y + w * 0.5),
        ],
        Stroke::new(1.4, color),
    ));
    p.add(Shape::line_segment(
        [
            Pos2::new(c.x, c.y + w * 0.5),
            Pos2::new(c.x + w, c.y - w * 0.5),
        ],
        Stroke::new(1.4, color),
    ));
}

/// 会话状态点
pub fn status_dot(p: &Painter, c: Pos2, color: Color32) {
    p.add(Shape::circle_filled(c, 3.2, color));
}
