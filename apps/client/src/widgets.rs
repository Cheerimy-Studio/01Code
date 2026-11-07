//! 通用绘制组件:芯片 / 头像 / 上下文仪表 / 原生小图标
#![allow(dead_code)]

use eframe::egui::{self, Color32, Painter, Pos2, Rect, RichText, Rounding, Sense, Shape, Stroke, Ui, Vec2};

use crate::theme as t;

#[derive(Clone, Copy)]
pub enum Chip {
    Sky,
    Green,
    Plain,
}

pub fn chip(ui: &mut Ui, text: &str, kind: Chip) {
    let (bg, br, tx) = match kind {
        Chip::Sky => (t::SKY_CHIP_BG, t::SKY_CHIP_BR, t::SKY_CHIP_TX),
        Chip::Green => (t::GRN_CHIP_BG, t::GRN_CHIP_BR, t::GRN_CHIP_TX),
        Chip::Plain => (t::PANEL, t::BORDER, t::MUTED),
    };
    egui::Frame::none()
        .fill(bg)
        .stroke(Stroke::new(1.0, br))
        .rounding(Rounding::same(999.0))
        .inner_margin(egui::Margin::symmetric(9.0, 2.5))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).color(tx));
        });
}

pub fn lerp(a: Color32, b: Color32, k: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// 天蓝→浅绿 渐变圆角块(条带模拟,零依赖原生绘制)
pub fn gradient_rect(p: &Painter, rect: Rect, rounding: f32) {
    let steps = 10;
    let h = rect.height() / steps as f32;
    for i in 0..steps {
        let c = lerp(
            Color32::from_rgb(0x38, 0xBD, 0xF8),
            Color32::from_rgb(0x34, 0xD3, 0x99),
            i as f32 / (steps - 1) as f32,
        );
        let y0 = rect.top() + i as f32 * h;
        let r = if i == 0 || i == steps - 1 { rounding } else { 0.0 };
        p.add(Shape::rect_filled(
            Rect::from_min_max(
                Pos2::new(rect.left(), y0),
                Pos2::new(rect.right(), y0 + h + 0.5),
            ),
            Rounding::same(r),
            c,
        ));
    }
}

pub fn avatar(ui: &mut Ui, label: &str, ai: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
    let p = ui.painter();
    if ai {
        gradient_rect(p, rect, 8.0);
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
    } else {
        p.add(Shape::rect_filled(rect, Rounding::same(8.0), t::CODE_BG));
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            t::TEXT2,
        );
    }
}

pub fn meter(ui: &mut Ui, frac: f32) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 6.0), Sense::hover());
    let p = ui.painter();
    p.add(Shape::rect_filled(rect, Rounding::same(3.0), t::BORDER));
    if frac > 0.0 {
        let fill = Rect::from_min_size(
            rect.min,
            Vec2::new(rect.width() * frac.min(1.0), 6.0),
        );
        gradient_rect(p, fill, 3.0);
    }
}

pub fn icon_check(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let p1 = Pos2::new(c.x - s * 0.55, c.y + s * 0.05);
    let p2 = Pos2::new(c.x - s * 0.10, c.y + s * 0.42);
    let p3 = Pos2::new(c.x + s * 0.55, c.y - s * 0.40);
    p.add(Shape::line(vec![p1, p2, p3], Stroke::new(2.4, color)));
}

pub fn icon_x(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let d = s * 0.4;
    p.add(Shape::line_segment(
        [Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)],
        Stroke::new(2.4, color),
    ));
    p.add(Shape::line_segment(
        [Pos2::new(c.x + d, c.y - d), Pos2::new(c.x - d, c.y + d)],
        Stroke::new(2.4, color),
    ));
}

pub fn icon_circle(p: &Painter, c: Pos2, s: f32, color: Color32) {
    p.add(Shape::circle_stroke(c, s * 0.42, Stroke::new(1.8, color)));
}
