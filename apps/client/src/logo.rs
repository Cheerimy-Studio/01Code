//! 01 Logo 原生矢量绘制 — 无图片资源,与 logo/*.svg 同构
//! (0=天蓝圆环 Agent 循环;1=浅绿旗形笔画执行;无终端下划线)

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Rounding, Shape, Stroke};

use crate::theme;

fn oval_points(center: Pos2, rx: f32, ry: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let t = (i as f32) / (n as f32) * std::f32::consts::TAU;
            Pos2::new(center.x + rx * t.cos(), center.y + ry * t.sin())
        })
        .collect()
}

/// 在 rect 内按 512 基准坐标绘制字形(建议接近正方形的 rect)
pub fn glyphs(p: &Painter, rect: Rect, sky: Color32, grn: Color32) {
    let s = rect.width().min(rect.height()) / 512.0;
    let ox = rect.left() + (rect.width() - 512.0 * s) * 0.5;
    let oy = rect.top() + (rect.height() - 512.0 * s) * 0.5;
    let m = |x: f32, y: f32| Pos2::new(ox + x * s, oy + y * s);

    // 0 — 圆环
    let ring_c = m(170.0, 256.0);
    let pts = oval_points(ring_c, 91.0 * s, 120.0 * s, 40);
    p.add(Shape::line(pts, Stroke::new(54.0 * s, sky)));

    // 1 — 竖干(圆角矩形,含圆帽延展;与 0 保持呼吸间距)
    let half = 27.0 * s;
    let stem = Rect::from_min_max(
        Pos2::new(m(364.0, 0.0).x - half, m(0.0, 113.0).y),
        Pos2::new(m(364.0, 0.0).x + half, m(0.0, 407.0).y),
    );
    p.add(Shape::rect_filled(stem, Rounding::same(half), grn));

    // 1 — 旗形起笔(圆帽斜线)
    let w = 44.0 * s;
    let a = m(360.0, 141.0);
    let b = m(311.0, 172.0);
    p.add(Shape::line_segment([a, b], Stroke::new(w, grn)));
    p.add(Shape::circle_filled(a, w * 0.5, grn));
    p.add(Shape::circle_filled(b, w * 0.5, grn));
}

/// 标题栏/侧栏小图标:圆角底 + 品牌字形
pub fn paint_tile(p: &egui::Painter, rect: Rect) {
    let size = rect.width().min(rect.height());
    let r = size * 0.24;
    p.add(Shape::rect_filled(rect, Rounding::same(r), Color32::WHITE));
    p.add(Shape::rect_stroke(
        rect,
        Rounding::same(r),
        Stroke::new(1.0_f32, theme::BORDER),
    ));
    glyphs(
        p,
        rect.shrink(size * 0.1),
        theme::SKY,
        Color32::from_rgb(0x34, 0xD3, 0x99),
    );
}

// ───────────────────────── 窗口 / 任务栏图标 ─────────────────────────

pub fn window_icon() -> egui::IconData {
    icon_data(256, false)
}

fn icon_data(size: usize, white: bool) -> egui::IconData {
    egui::IconData {
        rgba: icon_rgba(size, white),
        width: size as u32,
        height: size as u32,
    }
}

fn icon_rgba(size: usize, white: bool) -> Vec<u8> {
    let mut buf = vec![0u8; size * size * 4];
    let size_f = size as f32;
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0f32; 4];
            // 2x2 超采样抗锯齿
            for sy in 0..2 {
                for sx in 0..2 {
                    let px = (x as f32 + (sx as f32 + 0.5) * 0.5) * 512.0 / size_f;
                    let py = (y as f32 + (sy as f32 + 0.5) * 0.5) * 512.0 / size_f;
                    let c = icon_sample(px, py, white);
                    for i in 0..4 {
                        acc[i] += c[i];
                    }
                }
            }
            let o = (y * size + x) * 4;
            for i in 0..4 {
                buf[o + i] = (acc[i] / 4.0).round() as u8;
            }
        }
    }
    buf
}

fn lerp3(a: [u8; 3], b: [u8; 3], k: f32) -> [f32; 3] {
    let k = k.clamp(0.0, 1.0);
    [
        a[0] as f32 + (b[0] as f32 - a[0] as f32) * k,
        a[1] as f32 + (b[1] as f32 - a[1] as f32) * k,
        a[2] as f32 + (b[2] as f32 - a[2] as f32) * k,
    ]
}

fn in_round_rect(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    let cx = px.clamp(x0 + r, x1 - r);
    let cy = py.clamp(y0 + r, y1 - r);
    let dx = px - cx;
    let dy = py - cy;
    px >= x0 && px <= x1 && py >= y0 && py <= y1 && dx * dx + dy * dy <= r * r + 0.0001
}

fn in_capsule(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32) -> bool {
    let vx = bx - ax;
    let vy = by - ay;
    let wx = px - ax;
    let wy = py - ay;
    let len2 = vx * vx + vy * vy;
    let t = if len2 > 0.0 {
        ((wx * vx + wy * vy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = px - (ax + vx * t);
    let dy = py - (ay + vy * t);
    dx * dx + dy * dy <= r * r
}

/// 512 基准采样;white=true 为白底图标(标题栏),false 为深色底(任务栏)
fn icon_sample(px: f32, py: f32, white: bool) -> [f32; 4] {
    if !in_round_rect(px, py, 8.0, 8.0, 504.0, 504.0, 112.0) {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let k = py / 512.0;
    let (bg, sky, green) = if white {
        (
            lerp3([0xFF, 0xFF, 0xFF], [0xEA, 0xF4, 0xFB], k),
            lerp3([0x28, 0xAE, 0xF0], [0x03, 0x6F, 0xAE], k),
            lerp3([0x57, 0xE1, 0x9B], [0x12, 0xB8, 0x7A], k),
        )
    } else {
        (
            lerp3([0x0F, 0x24, 0x40], [0x08, 0x10, 0x20], k),
            lerp3([0xA5, 0xE3, 0xFF], [0x02, 0x84, 0xC7], k),
            lerp3([0xCC, 0xFA, 0xE0], [0x2F, 0xCE, 0x8F], k),
        )
    };

    // 0:椭圆环(外椭圆内、内椭圆外)
    let in_outer = ((px - 170.0) / 118.0).powi(2) + ((py - 256.0) / 147.0).powi(2) <= 1.0;
    let in_inner = ((px - 170.0) / 64.0).powi(2) + ((py - 256.0) / 93.0).powi(2) <= 1.0;
    if in_outer && !in_inner {
        return [sky[0], sky[1], sky[2], 255.0];
    }
    // 1:竖干
    if in_round_rect(px, py, 337.0, 113.0, 391.0, 407.0, 27.0) {
        return [green[0], green[1], green[2], 255.0];
    }
    // 1:旗形起笔
    if in_capsule(px, py, 360.0, 141.0, 311.0, 172.0, 22.0) {
        return [green[0], green[1], green[2], 255.0];
    }
    [bg[0], bg[1], bg[2], 255.0]
}
