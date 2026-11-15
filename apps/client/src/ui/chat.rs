use eframe::egui::{
    self, text::LayoutJob, Align2, Button, Color32, FontId, Key, Pos2, Rect, RichText, Rounding,
    Sense, Shape, Stroke, TextFormat, Vec2,
};

use crate::events::TodoState;
use crate::icons;
use crate::model::{Approval, Item, Model, Stream, ToolState};
use crate::theme as t;
use crate::widgets::{self, Chip};

pub fn show(ctx: &egui::Context, model: &mut Model) {
    // Ctrl+T：打开模型与思考强度弹层（与 ZCode 一致）
    if model.modal.is_none() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::T)) {
        model.open_model_picker = true;
    }
    // ── 底部:输入卡 + 状态栏(面板停靠,永不截断;无分隔线) ──
    let base_h = if !model.pending.is_empty() {
        150.0
    } else if model.steps() > 0 {
        124.0
    } else {
        96.0
    };
    // 长提示词/多行输入时给输入框留出高度，避免撑出面板造成错位
    let text = model.input.trim();
    let lines = text.lines().count().max(1);
    let wrapped = (text.chars().count() as f32 / 52.0).ceil() as usize;
    let extra = ((lines.max(wrapped).max(1) - 1) as f32 * 18.0).min(90.0);
    let bottom_h = base_h + extra;
    egui::TopBottomPanel::bottom("composer_panel")
        .exact_height(bottom_h)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::none()
                .fill(t::WHITE)
                .inner_margin(egui::Margin {
                    left: 16.0,
                    right: 16.0,
                    top: 2.0,
                    bottom: 4.0,
                }),
        )
        .show(ctx, |ui| {
            composer(ui, model);
            status_bar(ui, model);
        });

    // ── 主区:头部 + 会话流 ──
    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(t::WHITE)
                .inner_margin(egui::Margin::same(0.0)),
        )
        .show(ctx, |ui| {
            header(ui, model);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::none()
                        .inner_margin(egui::Margin {
                            left: 18.0,
                            right: 18.0,
                            top: 8.0,
                            bottom: 10.0,
                        })
                        .show(ui, |ui| {
                            let mut clicks: Vec<(String, bool)> = Vec::new();
                            if model.transcript().is_empty()
                                && model.input.trim().is_empty()
                                && model.pending.is_empty()
                            {
                                welcome(ui, model);
                            }
                            let items = model.transcript();
                            let n = items.len();
                            for (i, item) in items.iter().enumerate() {
                                render_item(ui, item, i == 0, i + 1 == n, &mut clicks);
                            }
                            if model.active_busy() {
                                busy_line(ui, model.busy_secs());
                            } else if let Some(Item::Ai(s)) = items.last() {
                                if s.done && s.typed >= s.text.chars().count() {
                                    if let Some(secs) = model.last_elapsed {
                                        ui.add_space(2.0);
                                        ui.label(
                                            RichText::new(format!("已完成 · 用时 {} 秒", secs))
                                                .size(12.0)
                                                .color(t::MUTED),
                                        );
                                    }
                                }
                            }
                            ui.add_space(10.0);
                            for (rid, ok) in clicks {
                                model.respond_approval(&rid, ok);
                            }
                        });
                });
        });

    // ── 弹窗 ──
    super::modal::show(ctx, model);
}

// ───────────────────────── 首页 ─────────────────────────

/// 空会话首页：品牌标记 + 账户状态 + 三个快捷入口，整体在可视区垂直居中
fn welcome(ui: &mut egui::Ui, model: &mut Model) {
    let avail = ui.available_height();
    let block = 156.0;
    let top = ((avail - block) * 0.40).clamp(16.0, 400.0);
    ui.add_space(top);
    ui.vertical_centered(|ui| {
        // 极淡的双层光晕，避免纯白背景上的品牌标记显得单薄
        let (halo, _) = ui.allocate_exact_size(Vec2::splat(76.0), Sense::hover());
        {
            let p = ui.painter();
            let c = halo.center();
            p.circle_filled(c, 36.0, Color32::from_rgba_unmultiplied(49, 95, 233, 7));
            p.circle_filled(c, 29.0, Color32::from_rgba_unmultiplied(49, 95, 233, 10));
            crate::logo::paint_tile(p, Rect::from_center_size(c, Vec2::splat(52.0)));
        }
        ui.add_space(4.0);
        ui.label(RichText::new("01 编码").size(24.0).color(t::TEXT).strong());
        ui.add_space(20.0);

        // 快捷入口：手动排版保证整行居中
        let labels: [(&str, &str); 3] = [
            ("解释这段代码", "解释这段代码的功能、输入输出和关键实现。\n"),
            (
                "定位并修复问题",
                "定位并修复这段代码中的问题，先说明原因，再给出修改。\n",
            ),
            (
                "补一组测试",
                "为这段代码补一组测试，覆盖主要分支和边界情况。\n",
            ),
        ];
        let font = FontId::proportional(12.5);
        let widths: Vec<f32> = labels
            .iter()
            .map(|(l, _)| {
                ui.painter()
                    .layout_no_wrap((*l).to_string(), font.clone(), t::TEXT2)
                    .size()
                    .x
                    + 28.0
            })
            .collect();
        let gap = 8.0;
        let total = widths.iter().sum::<f32>() + gap * (labels.len() - 1) as f32;
        let (row, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
        // 垂直居中布局会把光标推到上一个子控件的左侧，用 max_rect 保证整行相对页面居中
        let mut x = ui.max_rect().center().x - total * 0.5;
        let mut picked: Option<usize> = None;
        for (i, ((label, _), w)) in labels.iter().zip(widths.iter()).enumerate() {
            let r = Rect::from_min_size(Pos2::new(x, row.center().y - 15.0), Vec2::new(*w, 30.0));
            let resp = ui.interact(r, ui.id().with(("welcome-pill", i)), Sense::click());
            let hover = resp.hovered();
            let p = ui.painter();
            p.add(Shape::rect_filled(
                r,
                Rounding::same(15.0),
                if hover { t::SKY_CHIP_BG } else { t::WHITE },
            ));
            p.add(Shape::rect_stroke(
                r,
                Rounding::same(15.0),
                Stroke::new(1.0_f32, if hover { t::SKY_CHIP_BR } else { t::BORDER }),
            ));
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                *label,
                font.clone(),
                if hover { t::SKY_CHIP_TX } else { t::TEXT2 },
            );
            if resp.clicked() {
                picked = Some(i);
            }
            x += w + gap;
        }
        if let Some(i) = picked {
            model.input = labels[i].1.to_string();
            model.focus_input = true;
        }
    });
}

// ───────────────────────── 头部 ─────────────────────────

fn header(ui: &mut egui::Ui, model: &Model) {
    egui::Frame::none()
        .fill(t::WHITE)
        .inner_margin(egui::Margin {
            left: 18.0,
            right: 18.0,
            top: 12.0,
            bottom: 8.0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let title = model
                    .active_session()
                    .map(|s| s.title.clone())
                    .unwrap_or_else(|| "01 编码".to_string());
                ui.label(RichText::new(&title).size(15.0).color(t::TEXT).strong());
                if let Some(s) = model.active_session() {
                    widgets::chip(ui, &s.workspace, Chip::Plain);
                }
                let done = model
                    .todo
                    .iter()
                    .filter(|x| x.state == TodoState::Done)
                    .count();
                if !model.todo.is_empty() && !model.transcript().is_empty() {
                    widgets::chip(
                        ui,
                        &format!("{} / {} 步", done, model.todo.len()),
                        Chip::Sky,
                    );
                }
            });
        });
}

// ───────────────────────── 会话流 ─────────────────────────

fn render_item(
    ui: &mut egui::Ui,
    item: &Item,
    is_first: bool,
    is_last: bool,
    clicks: &mut Vec<(String, bool)>,
) {
    match item {
        Item::User(u) => {
            // 附件缩略图(右对齐,位于气泡上方)
            if !u.images.is_empty() {
                let row_w = ui.available_width();
                let max_h = 140.0f32;
                let mut items: Vec<(eframe::egui::TextureId, Vec2)> = Vec::new();
                let mut row_h = 0.0f32;
                for img in &u.images {
                    let [tw, th] = img.texture.size();
                    let scale = (220.0 / tw.max(1) as f32).min(max_h / th.max(1) as f32);
                    let sz = Vec2::new(tw as f32 * scale, th as f32 * scale);
                    row_h = row_h.max(sz.y);
                    items.push((img.texture.id(), sz));
                }
                let (row, _) =
                    ui.allocate_exact_size(Vec2::new(row_w, row_h + 6.0), Sense::hover());
                let mut x = row.right();
                let p = ui.painter_at(row);
                for (tid, sz) in items.iter().rev() {
                    let r = egui::Rect::from_min_size(Pos2::new(x - sz.x, row.top()), *sz);
                    p.image(
                        *tid,
                        r,
                        egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    p.add(Shape::rect_stroke(
                        r,
                        Rounding::same(10.0),
                        Stroke::new(1.0_f32, t::BORDER),
                    ));
                    x = r.left() - 8.0;
                }
                ui.add_space(4.0);
            }
            // 手动绘制气泡(圆角底 + 自动换行排版)
            let max_text_w = ((ui.available_width() * 0.66).clamp(220.0, 620.0) - 28.0).max(160.0);
            let gal = ui
                .painter()
                .layout(u.text.clone(), t::serif(14.5), t::TEXT, max_text_w);
            let bw = gal.size().x + 28.0;
            let bh = gal.size().y + 20.0;
            let row_w = ui.available_width();
            let (row_rect, _) = ui.allocate_exact_size(Vec2::new(row_w, bh), Sense::hover());
            let brect = egui::Rect::from_min_size(
                Pos2::new(row_rect.right() - bw, row_rect.top()),
                Vec2::new(bw, bh),
            );
            let p = ui.painter();
            p.add(Shape::rect_filled(
                brect,
                Rounding::same(14.0),
                t::BUBBLE_BG,
            ));
            p.galley(brect.min + Vec2::new(14.0, 10.0), gal, t::TEXT);
            // 时间 + 复制(手动行,避免 with_layout 吃掉剩余高度)
            let (trow, _) = ui.allocate_exact_size(Vec2::new(row_w, 20.0), Sense::hover());
            let copy_rect = egui::Rect::from_min_size(
                Pos2::new(trow.right() - 16.0, trow.top() + 2.0),
                Vec2::splat(16.0),
            );
            let cresp = ui.interact(
                copy_rect,
                egui::Id::new((u.time.as_str(), u.text.as_ptr())),
                Sense::click(),
            );
            let p = ui.painter_at(trow);
            let cy = trow.center().y;
            p.text(
                Pos2::new(copy_rect.left() - 8.0, cy),
                egui::Align2::RIGHT_CENTER,
                &u.time,
                FontId::proportional(11.0),
                t::FAINT,
            );
            icons::copy(
                &p,
                copy_rect.center(),
                13.0,
                if cresp.hovered() { t::TEXT2 } else { t::FAINT },
            );
            if cresp.clicked() {
                ui.ctx().output_mut(|o| o.copied_text = u.text.clone());
            }
            ui.add_space(6.0);
        }
        Item::Ai(stream) => {
            render_ai(ui, stream);
            ui.add_space(8.0);
        }
        Item::Tool(node) => render_tool(ui, node, is_first, is_last),
        Item::Diff(d) => render_diff(ui, d),
        Item::Approval(a) => render_approval(ui, a, clicks),
        Item::Sys(text) => {
            let gal = ui.painter().layout(
                text.clone(),
                FontId::proportional(11.5),
                t::FAINT,
                ui.available_width(),
            );
            let (row, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), gal.size().y + 8.0),
                Sense::hover(),
            );
            let x = row.center().x - gal.size().x * 0.5;
            ui.painter()
                .galley(Pos2::new(x, row.top() + 4.0), gal, t::FAINT);
        }
    }
}

/// 助手消息:流式打字 or Markdown 排版
fn render_ai(ui: &mut egui::Ui, stream: &Stream) {
    let len = stream.text.chars().count();
    if stream.text.is_empty() && !stream.done {
        return;
    }
    if stream.done && stream.typed < len {
        // 打字机:衬线正文 + 天蓝光标
        let partial: String = stream.text.chars().take(stream.typed).collect();
        let mut job = LayoutJob::default();
        job.append(
            &partial,
            0.0,
            TextFormat {
                font_id: t::serif(14.5),
                color: t::TEXT,
                ..Default::default()
            },
        );
        job.append(
            "▌",
            0.0,
            TextFormat {
                font_id: t::serif(14.5),
                color: t::SKY,
                ..Default::default()
            },
        );
        ui.add(egui::Label::new(job).wrap(true));
        return;
    }
    render_md(ui, &stream.text);
    // 复制按钮(界面禁止拖选,统一用按钮复制)
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
    let color = if resp.hovered() { t::TEXT2 } else { t::FAINT };
    icons::copy(&ui.painter_at(r), r.center(), 13.0, color);
    if resp.clicked() {
        ui.ctx().output_mut(|o| o.copied_text = stream.text.clone());
    }
}

/// 轻量 Markdown:标题 / 列表 / 代码块 / 行内代码 / 粗体(学 ZCode 对话排版)
fn render_md(ui: &mut egui::Ui, text: &str) {
    let mut in_code = false;
    let mut code_lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_code {
                flush_code(ui, &code_lines);
                code_lines.clear();
                in_code = false;
            } else {
                in_code = true;
            }
            continue;
        }
        if in_code {
            code_lines.push(line.to_string());
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(5.0);
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed.chars().nth(hashes) == Some(' ') {
            let content = trimmed[hashes..].trim_start();
            ui.add_space(6.0);
            ui.label(
                RichText::new(plain_text(content))
                    .font(t::bold(15.5))
                    .color(t::TEXT),
            );
            ui.add_space(2.0);
            continue;
        }
        let indent = raw.len() - trimmed.len();
        let is_bullet = trimmed.starts_with("- ") || trimmed.starts_with("* ");
        let dash_bullet = trimmed.starts_with("— ") || trimmed.starts_with("– ");
        let is_num = trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false);
        if is_bullet || dash_bullet || is_num {
            let content = if is_bullet {
                &trimmed[2..]
            } else if dash_bullet {
                &trimmed["— ".len()..]
            } else {
                match trimmed.find(". ") {
                    Some(i) => &trimmed[i + 2..],
                    None => trimmed,
                }
            };
            let marker = if is_num {
                if let Some(i) = trimmed.find(". ") {
                    trimmed[..=i].to_string()
                } else {
                    "•".to_string()
                }
            } else {
                "•".to_string()
            };
            ui.horizontal(|ui| {
                ui.add_space(indent as f32 * 7.0 + 4.0);
                ui.label(RichText::new(marker).font(t::serif(14.0)).color(t::TEXT2));
                ui.add(egui::Label::new(inline_job(content, 14.5)).wrap(true));
            });
            continue;
        }
        ui.add(egui::Label::new(inline_job(trimmed, 14.5)).wrap(true));
    }
    if in_code && !code_lines.is_empty() {
        flush_code(ui, &code_lines);
    }
}

fn plain_text(s: &str) -> String {
    s.replace("**", "").replace('`', "")
}

/// 行内代码 / 粗体 → LayoutJob
fn inline_job(text: &str, size: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut rest = text;
    let mut bold_on = false;
    loop {
        if rest.is_empty() {
            break;
        }
        let code_idx = rest.find('`');
        let bold_idx = rest.find("**");
        let next = match (code_idx, bold_idx) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        match next {
            None => {
                append_plain(&mut job, rest, size, bold_on);
                break;
            }
            Some(i) => {
                if i > 0 {
                    append_plain(&mut job, &rest[..i], size, bold_on);
                }
                if Some(i) == code_idx {
                    if let Some(end) = rest[i + 1..].find('`') {
                        let code = &rest[i + 1..i + 1 + end];
                        job.append(
                            code,
                            0.0,
                            TextFormat {
                                font_id: FontId::monospace(12.5),
                                color: t::TEXT,
                                background: t::CODE_BG,
                                ..Default::default()
                            },
                        );
                        rest = &rest[i + 1 + end + 1..];
                    } else {
                        append_plain(&mut job, &rest[i..], size, bold_on);
                        break;
                    }
                } else {
                    bold_on = !bold_on;
                    rest = &rest[i + 2..];
                }
            }
        }
    }
    job
}

fn append_plain(job: &mut LayoutJob, s: &str, size: f32, bold: bool) {
    if s.is_empty() {
        return;
    }
    let font_id = if bold { t::bold(size) } else { t::serif(size) };
    job.append(
        s,
        0.0,
        TextFormat {
            font_id,
            color: t::TEXT,
            ..Default::default()
        },
    );
}

/// 代码块:浅底圆角 + 等宽文本
fn flush_code(ui: &mut egui::Ui, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let font = FontId::monospace(12.0);
    let max_w = (ui.available_width() - 32.0).max(200.0);
    let galleys: Vec<_> = lines
        .iter()
        .map(|l| {
            ui.painter()
                .layout(l.clone(), font.clone(), t::TEXT2, max_w - 20.0)
        })
        .collect();
    let line_h = 17.0;
    let pad = 10.0;
    let inner_w = galleys.iter().map(|g| g.size().x).fold(0.0f32, f32::max);
    let w = (inner_w + pad * 2.0 + 8.0).min(ui.available_width() - 8.0);
    let h = galleys.len() as f32 * line_h + pad * 2.0;
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h + 4.0), Sense::hover());
    let rect = egui::Rect::from_min_size(
        Pos2::new(row.left() + 4.0, row.top() + 2.0),
        Vec2::new(w, h),
    );
    let p = ui.painter();
    p.add(Shape::rect_filled(rect, Rounding::same(9.0), t::PANEL));
    p.add(Shape::rect_stroke(
        rect,
        Rounding::same(9.0),
        Stroke::new(1.0_f32, t::BORDER),
    ));
    for (i, g) in galleys.iter().enumerate() {
        p.galley(
            Pos2::new(rect.left() + pad, rect.top() + pad + i as f32 * line_h),
            g.clone(),
            t::TEXT2,
        );
    }
    ui.add_space(6.0);
}

fn render_tool(ui: &mut egui::Ui, node: &crate::model::ToolNode, _is_first: bool, _is_last: bool) {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.add(Shape::rect_filled(
        rect,
        Rounding::same(8.0),
        if resp.hovered() { t::HOVER } else { t::PANEL },
    ));
    p.add(Shape::rect_stroke(
        rect,
        Rounding::same(8.0),
        Stroke::new(1.0_f32, t::BORDER),
    ));
    let c = Pos2::new(rect.left() + 16.0, rect.center().y);
    let icon_color = match node.state {
        ToolState::Ok => t::GRN,
        ToolState::Err => t::DEL_TX,
        ToolState::Running => t::SKY,
        ToolState::Pending => t::FAINT,
    };
    match node.state {
        ToolState::Ok => icons::check(&p, c, 12.0, icon_color),
        ToolState::Err => icons::sparkle(&p, c, 11.0, icon_color),
        ToolState::Running => {
            ui.put(
                Rect::from_center_size(c, Vec2::splat(14.0)),
                egui::Spinner::new().size(12.0).color(t::SKY),
            );
        }
        ToolState::Pending => icons::sparkle(&p, c, 11.0, icon_color),
    }
    let tool_color = if node.state == ToolState::Err {
        t::DEL_TX
    } else {
        t::TEXT2
    };
    let font = FontId::monospace(12.0);
    let tool_gal = p.layout_no_wrap(node.tool.clone(), font.clone(), tool_color);
    p.galley(
        Pos2::new(
            rect.left() + 30.0,
            rect.center().y - tool_gal.size().y * 0.5,
        ),
        tool_gal.clone(),
        tool_color,
    );
    if !node.meta.is_empty() {
        let meta_x = rect.left() + 30.0 + tool_gal.size().x + 8.0;
        let clip = Rect::from_min_max(
            Pos2::new(meta_x, rect.top()),
            Pos2::new(rect.right() - 54.0, rect.bottom()),
        );
        if clip.width() > 20.0 {
            ui.painter_at(clip).text(
                Pos2::new(meta_x, rect.center().y),
                Align2::LEFT_CENTER,
                &node.meta,
                FontId::proportional(11.0),
                t::FAINT,
            );
        }
    }
    let (status, status_color) = match node.state {
        ToolState::Ok => ("完成", t::GRN_CHIP_TX),
        ToolState::Err => ("失败", t::DEL_TX),
        ToolState::Running => ("执行中", t::SKY_DEEP),
        ToolState::Pending => ("等待", t::FAINT),
    };
    p.text(
        Pos2::new(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        status,
        FontId::proportional(10.5),
        status_color,
    );
    ui.add_space(5.0);
}

fn render_diff(ui: &mut egui::Ui, d: &crate::events::Diff) {
    let font = egui::FontId::monospace(12.0);
    let max_w = (ui.available_width() - 90.0).max(200.0);
    let mut rows: Vec<(String, Color32, Color32)> = Vec::new();
    for line in &d.removed {
        rows.push((format!("- {}", line), t::DEL_TX, t::DEL_BG));
    }
    for line in &d.added {
        rows.push((format!("+ {}", line), t::GRN_CHIP_TX, t::GRN_CHIP_BG));
    }
    if rows.is_empty() {
        return;
    }
    let galleys: Vec<_> = rows
        .iter()
        .map(|(txt, color, _)| {
            ui.painter()
                .layout(txt.clone(), font.clone(), *color, max_w)
        })
        .collect();
    let line_h = 18.0;
    let pad = 10.0;
    let inner_w = galleys.iter().map(|g| g.size().x).fold(0.0f32, f32::max);
    let card_w = (inner_w + pad * 2.0 + 16.0).min(ui.available_width() - 24.0);
    let card_h = galleys.len() as f32 * line_h + pad * 2.0;
    let (row_rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), card_h), Sense::hover());
    let card = egui::Rect::from_min_size(
        Pos2::new(row_rect.left() + 22.0, row_rect.top()),
        Vec2::new(card_w, card_h),
    );
    let p = ui.painter();
    p.add(Shape::rect_filled(card, Rounding::same(10.0), t::PANEL));
    p.add(Shape::rect_stroke(
        card,
        Rounding::same(10.0),
        Stroke::new(1.0_f32, t::BORDER),
    ));
    for (i, ((_, _, bg), g)) in rows.iter().zip(galleys.iter()).enumerate() {
        let ly = card.top() + pad + i as f32 * line_h;
        let line_rect = egui::Rect::from_min_size(
            Pos2::new(card.left() + 6.0, ly),
            Vec2::new(card.width() - 12.0, line_h),
        );
        p.add(Shape::rect_filled(line_rect, Rounding::same(3.0), *bg));
        p.galley(Pos2::new(card.left() + pad, ly), g.clone(), t::TEXT2);
    }
    ui.add_space(6.0);
}

fn render_approval(ui: &mut egui::Ui, a: &Approval, clicks: &mut Vec<(String, bool)>) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
        let p = ui.painter_at(r);
        p.add(Shape::convex_polygon(
            vec![
                Pos2::new(r.center().x, r.top() + 2.0),
                Pos2::new(r.right() - 1.0, r.bottom() - 3.0),
                Pos2::new(r.left() + 1.0, r.bottom() - 3.0),
            ],
            Color32::from_rgb(0xFD, 0xE6, 0x8A),
            Stroke::new(1.0_f32, Color32::from_rgb(0xD9, 0x77, 0x06)),
        ));
        ui.label(
            RichText::new(format!("需要批准:{}", a.action))
                .size(13.0)
                .color(t::TEXT),
        );
        if !a.reason.is_empty() {
            ui.label(RichText::new(&a.reason).size(12.0).color(t::MUTED));
        }
        match a.responded {
            Some(true) => {
                ui.label(RichText::new("已批准").size(12.0).color(t::GRN_CHIP_TX));
            }
            Some(false) => {
                ui.label(RichText::new("已拒绝").size(12.0).color(t::DEL_TX));
            }
            None => {
                if ui
                    .add_sized(
                        [52.0, 22.0],
                        Button::new(RichText::new("拒绝").size(12.0).color(t::TEXT2))
                            .fill(t::WHITE)
                            .stroke(Stroke::new(1.0_f32, t::BORDER))
                            .rounding(Rounding::same(7.0)),
                    )
                    .clicked()
                {
                    clicks.push((a.request_id.clone(), false));
                }
                if ui
                    .add_sized(
                        [52.0, 22.0],
                        Button::new(RichText::new("批准").size(12.0).color(Color32::WHITE))
                            .fill(t::SKY)
                            .rounding(Rounding::same(7.0)),
                    )
                    .clicked()
                {
                    clicks.push((a.request_id.clone(), true));
                }
            }
        }
    });
    ui.add_space(6.0);
}

fn busy_line(ui: &mut egui::Ui, secs: u64) {
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.add(egui::Spinner::new().size(13.0).color(t::SKY));
        ui.label(
            RichText::new(format!("执行中 · 用时 {} 秒 …", secs))
                .size(12.5)
                .color(t::SKY_DEEP),
        );
    });
    ui.add_space(4.0);
}

// ───────────────────────── 输入区 + 状态栏 ─────────────────────────

fn composer(ui: &mut egui::Ui, model: &mut Model) {
    egui::Frame::none()
        .fill(t::WHITE)
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0xE6, 0xEB, 0xF2)))
        .rounding(Rounding::same(18.0))
        .shadow(egui::epaint::Shadow {
            offset: Vec2::new(0.0, 4.0),
            blur: 14.0,
            spread: 0.0,
            color: Color32::from_rgba_unmultiplied(15, 23, 42, 20),
        })
        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
        .show(ui, |ui| {
            let modal_open = model.modal.is_some();
            // 待发送附件预览(单行横向滚动,图片缩略图 / 文件芯片,可移除)
            if !model.pending.is_empty() {
                let mut remove: Option<usize> = None;
                egui::ScrollArea::horizontal()
                    .id_source("pending-attachments")
                    .max_height(56.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (i, att) in model.pending.iter().enumerate() {
                                let (r, _) =
                                    ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
                                let p = ui.painter_at(r);
                                if let Some(tex) = &att.texture {
                                    p.image(
                                        tex.id(),
                                        r,
                                        egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                        Color32::WHITE,
                                    );
                                    p.add(Shape::rect_stroke(
                                        r,
                                        Rounding::same(8.0),
                                        Stroke::new(1.0_f32, t::BORDER),
                                    ));
                                } else {
                                    p.add(Shape::rect_filled(r, Rounding::same(8.0), t::PANEL));
                                    p.add(Shape::rect_stroke(
                                        r,
                                        Rounding::same(8.0),
                                        Stroke::new(1.0_f32, t::BORDER),
                                    ));
                                    let name: String = att.name.chars().take(7).collect();
                                    p.text(
                                        r.center(),
                                        egui::Align2::CENTER_CENTER,
                                        name,
                                        FontId::proportional(9.5),
                                        t::MUTED,
                                    );
                                }
                                // 移除 ✕
                                let xr = egui::Rect::from_min_size(
                                    Pos2::new(r.right() - 15.0, r.top() - 1.0),
                                    Vec2::splat(16.0),
                                );
                                let xresp =
                                    ui.interact(xr, ui.id().with(("rm_att", i)), Sense::click());
                                let p = ui.painter_at(xr);
                                p.add(Shape::circle_filled(
                                    xr.center(),
                                    7.0,
                                    Color32::from_rgba_unmultiplied(15, 23, 42, 190),
                                ));
                                p.add(Shape::line_segment(
                                    [
                                        Pos2::new(xr.center().x - 3.0, xr.center().y - 3.0),
                                        Pos2::new(xr.center().x + 3.0, xr.center().y + 3.0),
                                    ],
                                    Stroke::new(1.4_f32, Color32::WHITE),
                                ));
                                p.add(Shape::line_segment(
                                    [
                                        Pos2::new(xr.center().x + 3.0, xr.center().y - 3.0),
                                        Pos2::new(xr.center().x - 3.0, xr.center().y + 3.0),
                                    ],
                                    Stroke::new(1.4_f32, Color32::WHITE),
                                ));
                                if xresp.clicked() {
                                    remove = Some(i);
                                }
                            }
                        });
                    });
                if let Some(i) = remove {
                    model.remove_attachment(i);
                }
                ui.add_space(4.0);
            }
            // 在 TextEdit 消费按键前拦截 Enter:提交消息,Shift+Enter 保留换行
            let enter_now = if modal_open {
                false
            } else {
                ui.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, Key::Enter) && !i.modifiers.shift
                })
            };
            let hint = composer_hint(model);
            let resp = ui.add(
                egui::TextEdit::multiline(&mut model.input)
                    .id_source("composer-input")
                    .hint_text(hint)
                    .desired_rows(1)
                    .frame(false)
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut model.focus_input) {
                resp.request_focus();
            }
            // 无其它焦点控件时,输入框自动聚焦(主输入位;弹窗打开时不抢焦点)
            if !modal_open && ui.memory(|m| m.focused().is_none()) {
                resp.request_focus();
            }
            let can_send = composer_can_send(model);
            if enter_now {
                if can_send {
                    model.user_submit();
                } else {
                    open_settings_for_input(model);
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                // 添加文件:自绘圆形按钮(加号垂直水平居中)+ 文件选择
                let (r, cresp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
                let hover = cresp.hovered();
                let p = ui.painter_at(r);
                p.add(Shape::circle_filled(
                    r.center(),
                    15.0,
                    if hover { t::HOVER } else { t::WHITE },
                ));
                p.add(Shape::circle_stroke(
                    r.center(),
                    15.0,
                    Stroke::new(1.0_f32, t::BORDER),
                ));
                icons::plus(&p, r.center(), 11.0, t::TEXT2);
                if cresp.clicked() {
                    let files = rfd::FileDialog::new()
                        .set_title("选择文件或图片")
                        .pick_files();
                    if let Some(paths) = files {
                        for path in paths {
                            model.add_attachment(ui.ctx(), path);
                        }
                    }
                }
                access_mode_picker(ui, model);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let busy = model.active_busy();
                    let enabled = can_send && !busy;
                    let send = ui.add_sized(
                        [34.0, 34.0],
                        Button::new(
                            RichText::new(if busy { "■" } else { "↑" })
                                .size(14.0)
                                .color(Color32::WHITE),
                        )
                        .fill(if enabled { t::SKY } else { t::FAINT })
                        .rounding(Rounding::same(999.0)),
                    );
                    if send.clicked() && !busy {
                        if can_send {
                            model.user_submit();
                        } else {
                            open_settings_for_input(model);
                        }
                    }
                    ui.add_space(8.0);
                    if ui.available_width() > 110.0 {
                        model_picker(ui, model);
                    }
                });
            });
        });
}

fn composer_can_send(model: &Model) -> bool {
    if !model.gateway_mode {
        return true;
    }
    let s = &model.settings;
    s.key_configured && s.runtime_running && !s.busy && !s.logging_in
}

fn composer_hint(model: &Model) -> &'static str {
    const NORMAL: &str = "发送信息和创建任务";
    if !model.gateway_mode {
        return NORMAL;
    }
    let s = &model.settings;
    if !s.key_configured {
        return "登录言灵中转后开始对话";
    }
    if s.logging_in {
        return "等待浏览器完成登录…";
    }
    if !s.runtime_running {
        return if s.busy {
            "正在启动网关…"
        } else {
            "网关已停止"
        };
    }
    if s.busy {
        return "正在应用设置…";
    }
    NORMAL
}

fn open_settings_for_input(model: &mut Model) {
    model.modal = Some(crate::model::Modal::Settings);
    if !model.settings.key_configured || model.settings.logging_in {
        model.settings.tab = 0;
        return;
    }
    model.settings.tab = 1;
    if !model.settings.runtime_running {
        model.settings.status = "网关已停止，请先启动".into();
    }
}

/// 输入框右下角的模型选择器（完整列表，向上弹出）
fn access_mode_picker(ui: &mut egui::Ui, model: &mut Model) {
    let mode = model.settings.access_mode.min(2);
    let label = model.settings.access_label();
    let warn = mode > 0;
    let color = if warn {
        Color32::from_rgb(0xB4, 0x53, 0x09)
    } else {
        t::TEXT2
    };
    let font = FontId::proportional(12.0);
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), color)
        .size()
        .x;
    let width = (text_w + 48.0).clamp(90.0, 150.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click());
    let popup_id = egui::Id::new("composer-access-mode");
    let open = ui.memory(|m| m.is_popup_open(popup_id));
    let hover = resp.hovered() || open;
    let p = ui.painter_at(rect);
    p.add(Shape::rect_filled(
        rect,
        Rounding::same(15.0),
        if hover { t::HOVER } else { t::WHITE },
    ));
    p.add(Shape::rect_stroke(
        rect,
        Rounding::same(15.0),
        Stroke::new(1.0_f32, t::BORDER),
    ));
    let icon_c = Pos2::new(rect.left() + 14.0, rect.center().y);
    if mode == 0 {
        icons::shield(&p, icon_c, 13.0, t::MUTED);
    } else {
        icons::warning(&p, icon_c, 13.0, color);
    }
    p.text(
        Pos2::new(rect.left() + 27.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        color,
    );
    icons::chevron_down(
        &p,
        Pos2::new(rect.right() - 11.0, rect.center().y),
        8.0,
        t::MUTED,
    );
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    let screen = ui.ctx().screen_rect();
    let left = resp
        .rect
        .left()
        .min(screen.right() - 308.0)
        .max(screen.left() + 8.0);
    let mut anchor = resp.clone();
    anchor.rect = Rect::from_min_max(
        Pos2::new(left, resp.rect.top()),
        Pos2::new(left + 300.0, resp.rect.bottom()),
    );
    anchor.interact_rect = anchor.rect;
    let _ = egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        &anchor,
        egui::AboveOrBelow::Above,
        |ui| {
            ui.set_min_width(300.0);
            ui.set_max_width(300.0);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new("访问权限").size(11.0).color(t::FAINT));
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            for (i, (name, desc)) in [
                ("普通模式", "有变更时询问是否同意"),
                ("自主模式", "有风险时询问"),
                ("完全访问", "不再询问，风险自负"),
            ]
            .iter()
            .enumerate()
            {
                let selected = i == mode;
                let (r, row) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                if selected || row.hovered() {
                    ui.painter().add(Shape::rect_filled(
                        r,
                        Rounding::same(8.0),
                        if selected { t::SKY_CHIP_BG } else { t::HOVER },
                    ));
                }
                let pr = ui.painter_at(r);
                let warn = i > 0;
                let icon_color = if warn {
                    Color32::from_rgb(0xB4, 0x53, 0x09)
                } else {
                    t::MUTED
                };
                let ic = Pos2::new(r.left() + 16.0, r.center().y);
                if warn {
                    icons::warning(&pr, ic, 13.0, icon_color);
                } else {
                    icons::shield(&pr, ic, 13.0, icon_color);
                }
                pr.text(
                    Pos2::new(r.left() + 32.0, r.center().y - 8.0),
                    Align2::LEFT_CENTER,
                    *name,
                    FontId::proportional(12.5),
                    if selected { t::SKY_CHIP_TX } else { t::TEXT },
                );
                pr.text(
                    Pos2::new(r.left() + 32.0, r.center().y + 9.0),
                    Align2::LEFT_CENTER,
                    *desc,
                    FontId::proportional(10.5),
                    t::FAINT,
                );
                if selected {
                    pr.add(Shape::circle_filled(
                        Pos2::new(r.right() - 14.0, r.center().y),
                        3.0,
                        t::SKY,
                    ));
                }
                if row.clicked() {
                    model.settings.set_access_mode(i);
                    ui.memory_mut(|m| m.close_popup());
                }
                ui.add_space(2.0);
            }
            ui.add_space(4.0);
        },
    );
}

fn model_picker(ui: &mut egui::Ui, model: &mut Model) {
    let s = &model.settings;
    let current = if s.model.is_empty() {
        "选择模型".to_string()
    } else {
        s.model.clone()
    };
    let font = FontId::proportional(12.5);
    let think_font = FontId::proportional(10.5);
    let thinking = s.thinking_label().to_string();
    let text_w = ui
        .painter()
        .layout_no_wrap(current.clone(), font.clone(), t::TEXT2)
        .size()
        .x;
    let think_w = ui
        .painter()
        .layout_no_wrap(thinking.clone(), think_font.clone(), t::MUTED)
        .size()
        .x;
    let width = (text_w + think_w + 54.0).clamp(120.0, 250.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click());
    let popup_id = egui::Id::new("composer-model-picker");
    if std::mem::take(&mut model.open_model_picker) {
        ui.memory_mut(|m| m.open_popup(popup_id));
    }
    let open = ui.memory(|m| m.is_popup_open(popup_id));
    let hover = resp.hovered() || open;
    let p = ui.painter_at(rect);
    p.add(Shape::rect_filled(
        rect,
        Rounding::same(15.0),
        if hover { t::HOVER } else { t::WHITE },
    ));
    p.add(Shape::rect_stroke(
        rect,
        Rounding::same(15.0),
        Stroke::new(1.0_f32, t::BORDER),
    ));
    let name_area = (width - 12.0 - 8.0 - think_w - 26.0).max(20.0);
    let think_x = rect.left() + 12.0 + name_area + 8.0;
    let clip = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, rect.top()),
        Pos2::new(think_x - 6.0, rect.bottom()),
    );
    ui.painter_at(clip).text(
        Pos2::new(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        &current,
        font,
        if s.key_configured { t::TEXT2 } else { t::FAINT },
    );
    p.text(
        Pos2::new(think_x, rect.center().y),
        Align2::LEFT_CENTER,
        &thinking,
        think_font,
        t::MUTED,
    );
    icons::chevron_down(
        &p,
        Pos2::new(rect.right() - 13.0, rect.center().y),
        9.0,
        t::MUTED,
    );
    if resp.clicked() {
        if s.models.is_empty() && !s.syncing_models && s.key_configured {
            model.settings.sync_models();
        }
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    // 弹层右对齐到按钮，避免在窗口右下角被裁掉
    let screen = ui.ctx().screen_rect();
    let right = resp.rect.right().min(screen.right() - 8.0);
    let left = (right - 340.0).max(screen.left() + 8.0);
    let mut anchor = resp.clone();
    anchor.rect = Rect::from_min_max(
        Pos2::new(left, resp.rect.top()),
        Pos2::new(right, resp.rect.bottom()),
    );
    anchor.interact_rect = anchor.rect;
    let _ = egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        &anchor,
        egui::AboveOrBelow::Above,
        |ui| {
            ui.set_min_width(320.0);
            ui.set_max_width(360.0);
            ui.add_space(4.0);
            let (row, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
            let p = ui.painter_at(row);
            let title = p.layout_no_wrap("言灵中转".into(), FontId::proportional(13.5), t::TEXT);
            let tx = row.left() + 8.0;
            p.galley(
                Pos2::new(tx, row.center().y - title.size().y * 0.5),
                title.clone(),
                t::TEXT,
            );
            let chip = Rect::from_center_size(
                Pos2::new(tx + title.size().x + 8.0 + 15.0, row.center().y),
                Vec2::new(30.0, 16.0),
            );
            p.add(Shape::rect_filled(chip, Rounding::same(8.0), t::SELECTED));
            p.text(
                chip.center(),
                Align2::CENTER_CENTER,
                "API",
                FontId::proportional(9.5),
                t::MUTED,
            );
            if !model.settings.models.is_empty() {
                p.text(
                    Pos2::new(row.right() - 8.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    format!("{} 个", model.settings.models.len()),
                    FontId::proportional(10.5),
                    t::FAINT,
                );
            }
            ui.add_space(3.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("模型").size(11.0).color(t::FAINT));
            });
            ui.add_space(3.0);
            if !model.settings.key_configured {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("登录言灵中转后可选择模型")
                        .size(11.5)
                        .color(t::MUTED),
                );
                ui.add_space(4.0);
                if ui
                    .add(
                        egui::Button::new(RichText::new("去登录").size(11.5).color(t::TEXT2))
                            .fill(t::WHITE)
                            .stroke(Stroke::new(1.0_f32, t::BORDER))
                            .rounding(7.0),
                    )
                    .clicked()
                {
                    model.settings.tab = 0;
                    model.modal = Some(crate::model::Modal::Settings);
                    ui.memory_mut(|m| m.close_popup());
                }
            } else if model.settings.models.is_empty() {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if model.settings.syncing_models {
                        ui.add(egui::Spinner::new().size(14.0).color(t::MUTED));
                        ui.label(
                            RichText::new("正在同步模型列表…")
                                .size(11.5)
                                .color(t::MUTED),
                        );
                    } else {
                        ui.label(RichText::new("暂无模型列表").size(11.5).color(t::MUTED));
                        if ui
                            .add(
                                egui::Button::new(RichText::new("同步").size(11.0).color(t::TEXT2))
                                    .fill(t::WHITE)
                                    .stroke(Stroke::new(1.0_f32, t::BORDER))
                                    .rounding(7.0),
                            )
                            .clicked()
                        {
                            model.settings.sync_models();
                        }
                    }
                });
            } else {
                let ids = model.settings.models.clone();
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for id in ids {
                            let selected = id == model.settings.model;
                            let meta = model.settings.model_meta.get(&id).cloned();
                            let (r, row) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 32.0),
                                Sense::click(),
                            );
                            if selected || row.hovered() {
                                ui.painter().add(Shape::rect_filled(
                                    r,
                                    Rounding::same(8.0),
                                    if selected { t::SKY_CHIP_BG } else { t::HOVER },
                                ));
                            }
                            let pr = ui.painter_at(r);
                            if selected {
                                pr.add(Shape::circle_filled(
                                    Pos2::new(r.left() + 4.0, r.center().y),
                                    2.2,
                                    t::SKY,
                                ));
                            }
                            let text_clip = Rect::from_min_max(
                                Pos2::new(r.left() + 10.0, r.top()),
                                Pos2::new(r.right() - 70.0, r.bottom()),
                            );
                            ui.painter_at(text_clip).text(
                                Pos2::new(r.left() + 10.0, r.center().y),
                                Align2::LEFT_CENTER,
                                &id,
                                FontId::proportional(12.5),
                                if selected { t::SKY_CHIP_TX } else { t::TEXT2 },
                            );
                            let mut right = r.right() - 10.0;
                            if let Some(meta) = meta {
                                if meta.paid {
                                    pr.text(
                                        Pos2::new(right, r.center().y),
                                        Align2::RIGHT_CENTER,
                                        "付费",
                                        FontId::proportional(9.5),
                                        t::FAINT,
                                    );
                                    right -= 30.0;
                                } else if meta.free {
                                    pr.text(
                                        Pos2::new(right, r.center().y),
                                        Align2::RIGHT_CENTER,
                                        "免费",
                                        FontId::proportional(9.5),
                                        t::GRN_CHIP_TX,
                                    );
                                    right -= 30.0;
                                }
                                if meta.ctx > 0 {
                                    pr.text(
                                        Pos2::new(right, r.center().y),
                                        Align2::RIGHT_CENTER,
                                        format!("{}k", meta.ctx / 1000),
                                        FontId::proportional(9.5),
                                        t::FAINT,
                                    );
                                }
                            }
                            if row.clicked() {
                                let old = model.settings.model.clone();
                                model.settings.model = id.clone();
                                model.model_name = id.clone();
                                if model.settings.save() {
                                    model.settings.status = "正在切换模型…".into();
                                    ui.memory_mut(|m| m.close_popup());
                                } else {
                                    model.settings.model = old.clone();
                                    model.model_name = old;
                                }
                            }
                        }
                    });
            }
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), 20.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new("思考强度").size(11.0).color(t::FAINT));
                    ui.label(
                        RichText::new(model.settings.thinking_label())
                            .size(12.5)
                            .color(t::TEXT)
                            .strong(),
                    );
                },
            );
            ui.add_space(4.0);
            thinking_slider(ui, model);
            ui.add_space(2.0);
        },
    );
}

fn thinking_slider(ui: &mut egui::Ui, model: &mut Model) {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 24.0),
        Sense::click_and_drag(),
    );
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + 16.0, rect.center().y - 4.0),
        Pos2::new(rect.right() - 16.0, rect.center().y + 4.0),
    );
    let green = Color32::from_rgb(0x10, 0xB9, 0x81);
    let blue = Color32::from_rgb(0x3B, 0x82, 0xF6);
    let p = ui.painter_at(rect);
    // 连续渐变圆角条：按像素切片绘制，两端自然收圆，不再额外画两个圆点
    let radius = track.height() * 0.5;
    let half_height = |x: f32| -> f32 {
        if x < track.left() + radius {
            let d = track.left() + radius - x;
            (radius * radius - d * d).max(0.0).sqrt()
        } else if x > track.right() - radius {
            let d = x - (track.right() - radius);
            (radius * radius - d * d).max(0.0).sqrt()
        } else {
            radius
        }
    };
    let mix = |a: u8, b: u8, t: f32| -> u8 { (a as f32 + (b as f32 - a as f32) * t).round() as u8 };
    let steps = 180usize;
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let x0 = track.left() + track.width() * t0;
        let x1 = track.left() + track.width() * t1;
        let h0 = half_height(x0);
        let h1 = half_height(x1);
        if h0 <= 0.0 && h1 <= 0.0 {
            continue;
        }
        let mid = (t0 + t1) * 0.5;
        let color = Color32::from_rgb(
            mix(green.r(), blue.r(), mid),
            mix(green.g(), blue.g(), mid),
            mix(green.b(), blue.b(), mid),
        );
        p.add(Shape::convex_polygon(
            vec![
                Pos2::new(x0, track.center().y - h0),
                Pos2::new(x1, track.center().y - h1),
                Pos2::new(x1, track.center().y + h1),
                Pos2::new(x0, track.center().y + h0),
            ],
            color,
            Stroke::NONE,
        ));
    }
    let level = model.settings.thinking_level.min(4);
    let t = level as f32 / 4.0;
    let cx = track.left() + (track.right() - track.left()) * t;
    p.circle_filled(
        Pos2::new(cx, rect.center().y + 1.0),
        9.0,
        Color32::from_black_alpha(25),
    );
    p.circle_filled(Pos2::new(cx, rect.center().y), 8.0, t::WHITE);
    p.circle_stroke(
        Pos2::new(cx, rect.center().y),
        8.0,
        Stroke::new(1.0_f32, t::BORDER),
    );
    if let Some(pos) = resp.interact_pointer_pos() {
        let frac = ((pos.x - track.left()) / (track.right() - track.left())).clamp(0.0, 1.0);
        let next = (frac * 4.0).round() as usize;
        if next != model.settings.thinking_level {
            model.settings.thinking_level = next;
        }
    }
    if resp.drag_stopped() || resp.clicked() {
        let level = model.settings.thinking_level;
        model.settings.set_thinking_level(level);
        // 点击/拖动后保持模型弹层打开，避免被误判为外部点击
        ui.memory_mut(|m| m.open_popup(egui::Id::new("composer-model-picker")));
    }
}

fn status_bar(ui: &mut egui::Ui, model: &Model) {
    let steps = model.steps();
    if steps == 0 {
        return;
    }
    // 网关同步的上下文上限优先；运行时上报的 CtxUsage 会覆盖它
    let synced = model.settings.context_limit(&model.model_name);
    let limit = if model.ctx_limit > 0.0 {
        model.ctx_limit
    } else if synced > 0.0 {
        synced
    } else {
        200_000.0
    };
    let pct = (model.ctx_used / limit.max(1.0) * 100.0) as u32;
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.label(
            RichText::new(format!(
                "{} 步 · 上下文 {}k / {}k({}%)",
                steps,
                (model.ctx_used / 1000.0) as u32,
                (limit / 1000.0) as u32,
                pct
            ))
            .size(11.5)
            .color(t::FAINT),
        );
    });
    ui.add_space(8.0);
}
