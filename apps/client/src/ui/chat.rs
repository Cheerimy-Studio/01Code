use eframe::egui::{
    self, text::LayoutJob, Button, Color32, FontId, Key, Pos2, RichText, Rounding, Sense, Shape,
    Stroke, TextFormat, Vec2,
};

use crate::events::TodoState;
use crate::icons;
use crate::model::{Approval, Item, Model, Stream, ToolState};
use crate::theme as t;
use crate::widgets::{self, Chip};

pub fn show(ctx: &egui::Context, model: &mut Model) {
    // ── 底部:输入卡 + 状态栏(面板停靠,永不截断;无分隔线) ──
    let bottom_h = if !model.pending.is_empty() {
        212.0
    } else {
        150.0
    };
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
            let width = ui.available_width();
            let gutter = ((width - 1040.0) * 0.5).max(0.0);
            ui.horizontal(|ui| {
                ui.add_space(gutter);
                ui.vertical(|ui| {
                    ui.set_width(width - 2.0 * gutter);
                    composer(ui, model);
                    status_bar(ui, model);
                });
            });
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
                .stick_to_bottom(true)
                .id_source(("chat_scroll", &model.active_sid, model.trace_view))
                .show(ui, |ui| {
                    let gutter = ((ui.available_width() - 1040.0) * 0.5).max(18.0);
                    egui::Frame::none()
                        .inner_margin(egui::Margin {
                            left: gutter,
                            right: gutter,
                            top: 8.0,
                            bottom: 10.0,
                        })
                        .show(ui, |ui| {
                            let content_width = ui.available_width();
                            ui.set_max_width(content_width);
                            let mut clicks: Vec<(String, bool)> = Vec::new();
                            let mut start_new = false;
                            let items = model.transcript();
                            let n = items.len();
                            if n == 0 {
                                ui.add_space(70.0);
                                ui.vertical_centered(|ui| {
                                    let (logo_rect, _) = ui
                                        .allocate_exact_size(Vec2::splat(46.0), Sense::hover());
                                    crate::logo::paint_tile(ui.painter(), logo_rect);
                                    ui.add_space(12.0);
                                    ui.label(
                                        RichText::new("01 编码")
                                            .size(21.0)
                                            .color(t::TEXT)
                                            .strong(),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new("经由 NullClaw 网关运行的编码助手")
                                            .size(12.5)
                                            .color(t::FAINT),
                                    );
                                    ui.add_space(18.0);
                                    if ui
                                        .add_sized(
                                            [132.0, 34.0],
                                            Button::new(
                                                RichText::new("＋ 新会话")
                                                    .size(13.0)
                                                    .color(t::TEXT),
                                            )
                                            .fill(t::WHITE)
                                            .stroke(Stroke::new(1.0_f32, t::BORDER))
                                            .rounding(Rounding::same(9.0)),
                                        )
                                        .clicked()
                                    {
                                        start_new = true;
                                    }
                                });
                            }
                            for (i, item) in items.iter().enumerate() {
                                if model.trace_view && matches!(item, Item::User(_) | Item::Ai(_)) {
                                    continue;
                                }
                                render_cached_item(
                                    ui,
                                    &model.active_sid,
                                    item,
                                    i,
                                    i + 1 == n,
                                    &mut clicks,
                                );
                            }
                            if model.is_busy() {
                                busy_line(ui, model.busy_secs(), model.waiting_approval());
                            } else if let Some(Item::Ai(s)) = items.last() {
                                if s.done {
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
                            if start_new {
                                model.new_task();
                            }
                        });
                });
        });

    // ── 弹窗 ──
    super::modal::show(ctx, model);
}

// ───────────────────────── 头部 ─────────────────────────

fn header(ui: &mut egui::Ui, model: &mut Model) {
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
                if let Some(s) = model.active_session() {
                    widgets::chip(ui, &s.workspace, Chip::Plain);
                }
                ui.add(
                    egui::Label::new(RichText::new(&title).size(15.0).color(t::TEXT).strong())
                        .truncate(true),
                )
                .on_hover_text(&title);
                let done = model
                    .todo
                    .iter()
                    .filter(|x| x.state == TodoState::Done)
                    .count();
                if !model.todo.is_empty() {
                    widgets::chip(
                        ui,
                        &format!("{} / {} 步", done, model.todo.len()),
                        Chip::Sky,
                    );
                }
            });
            if !model.todo.is_empty() {
                egui::CollapsingHeader::new("任务计划")
                    .id_source(("plan", &model.active_sid))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(140.0)
                            .show(ui, |ui| {
                                for step in &model.todo {
                                    let (mark, color) = match step.state {
                                        TodoState::Pending => ("○", t::MUTED),
                                        TodoState::Running => ("●", t::SKY),
                                        TodoState::Done => ("✓", t::GRN_CHIP_TX),
                                    };
                                    ui.label(
                                        RichText::new(format!("{mark} {}", step.title))
                                            .color(color),
                                    );
                                }
                            });
                    });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let steps = model.steps();
                for (value, label) in [
                    (false, "对话".to_string()),
                    (
                        true,
                        if steps > 0 {
                            format!("轨迹 · {}", steps)
                        } else {
                            "轨迹".to_string()
                        },
                    ),
                ] {
                    let selected = model.trace_view == value;
                    let response = ui.add(
                        Button::new(RichText::new(label).size(14.0).color(if selected {
                            t::SKY
                        } else {
                            t::MUTED
                        }))
                        .frame(false),
                    );
                    if response.clicked() {
                        model.trace_view = value;
                    }
                    if selected {
                        ui.painter().line_segment(
                            [response.rect.left_bottom(), response.rect.right_bottom()],
                            Stroke::new(2.0_f32, t::SKY),
                        );
                    }
                }
            });
            ui.separator();
        });
}

// ───────────────────────── 会话流 ─────────────────────────

#[derive(Clone, Copy)]
struct ItemLayout {
    width: u32,
    stamp: u64,
    height: f32,
}
fn render_cached_item(
    ui: &mut egui::Ui,
    sid: &str,
    item: &Item,
    index: usize,
    last: bool,
    clicks: &mut Vec<(String, bool)>,
) {
    let width = ui.available_width().round() as u32;
    let stamp = match item {
        Item::Ai(a) => a.text.len() as u64,
        Item::Reasoning(s) | Item::Sys(s) => s.len() as u64,
        Item::User(u) => u.text.len() as u64 + u.images.len() as u64,
        Item::Tool(n) => n.meta.len() as u64 + (n.state as u64) * 1_000_000,
        Item::Diff(d) => d.added.len() as u64 + d.removed.len() as u64,
        Item::Approval(a) => {
            a.action.len() as u64 + if a.responded.is_some() { 1_000_000 } else { 0 }
        }
    };
    let id = egui::Id::new(("message_layout", sid, index));
    let cache = ui.ctx().data(|d| d.get_temp::<ItemLayout>(id));
    if !last {
        if let Some(cache) = cache.filter(|c| c.width == width && c.stamp == stamp) {
            if cache.height <= 0.0 {
                return;
            }
            let rect = egui::Rect::from_min_size(
                ui.cursor().min,
                Vec2::new(ui.available_width(), cache.height),
            );
            if !ui.is_rect_visible(rect) {
                ui.allocate_exact_size(
                    Vec2::new(
                        ui.available_width(),
                        (cache.height - ui.spacing().item_spacing.y).max(0.0),
                    ),
                    Sense::hover(),
                );
                return;
            }
        }
    }
    let top = ui.cursor().top();
    ui.push_id((sid, index), |ui| {
        render_item(ui, item, index == 0, last, clicks)
    });
    let height = ui.cursor().top() - top;
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            id,
            ItemLayout {
                width,
                stamp,
                height,
            },
        )
    });
}

fn render_item(
    ui: &mut egui::Ui,
    item: &Item,
    _is_first: bool,
    _is_last: bool,
    clicks: &mut Vec<(String, bool)>,
) {
    match item {
        Item::Reasoning(text) => {
            egui::CollapsingHeader::new("思考过程")
                .id_source("reasoning")
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(text).color(t::MUTED))
                            .wrap(true)
                            .selectable(true),
                    );
                });
        }
        Item::User(u) => {
            // 附件缩略图(右对齐,位于气泡上方)
            if !u.images.is_empty() {
                let row_w = ui.available_width();
                let max_h = 140.0f32;
                let mut items: Vec<(eframe::egui::TextureId, Vec2)> = Vec::new();
                let mut row_h = 0.0f32;
                for img in &u.images {
                    let [tw, th] = img.texture.size();
                    let max_w = ((row_w - 8.0 * (u.images.len().saturating_sub(1)) as f32)
                        / u.images.len().max(1) as f32)
                        .clamp(1.0, 220.0);
                    let scale = (max_w / tw.max(1) as f32).min(max_h / th.max(1) as f32);
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
        Item::Tool(node) => render_tool(ui, node),
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
    if stream.text.is_empty() {
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
        ui.add(
            egui::Label::new(inline_job(trimmed, 14.5))
                .wrap(true)
                .selectable(true),
        );
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
    let h = galleys.iter().map(|g| g.size().y.max(line_h)).sum::<f32>() + pad * 2.0;
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
    let mut y = rect.top() + pad;
    for g in &galleys {
        p.galley(Pos2::new(rect.left() + pad, y), g.clone(), t::TEXT2);
        y += g.size().y.max(line_h);
    }
    let copy = egui::Rect::from_min_size(
        Pos2::new(rect.right() - 24.0, rect.top() + 3.0),
        Vec2::splat(20.0),
    );
    icons::copy(&ui.painter_at(copy), copy.center(), 12.0, t::MUTED);
    if ui
        .interact(
            copy,
            ui.id().with(("code_copy", rect.top().to_bits())),
            Sense::click(),
        )
        .clicked()
    {
        ui.ctx().output_mut(|o| o.copied_text = lines.join("\n"));
    }
    ui.add_space(6.0);
}

fn render_tool(ui: &mut egui::Ui, node: &crate::model::ToolNode) {
    let (state, color) = match node.state {
        ToolState::Running => ("运行中", t::SKY_DEEP),
        ToolState::Ok => ("已完成", t::GRN_CHIP_TX),
        ToolState::Err => ("失败 / 中断", t::DEL_TX),
        _ => ("等待", t::MUTED),
    };
    let elapsed = node
        .started
        .filter(|_| node.state == ToolState::Running)
        .map(|s| s.elapsed().as_millis() as u64)
        .unwrap_or(node.elapsed_ms);
    egui::Frame::none()
        .fill(t::PANEL)
        .rounding(9.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            egui::CollapsingHeader::new(
                RichText::new(format!(
                    "{}  ·  {}  ·  {:.1}s",
                    node.tool,
                    state,
                    elapsed as f64 / 1000.0
                ))
                .color(color),
            )
            .id_source((&node.call, "tool"))
            .show(ui, |ui| {
                ui.label(RichText::new("参数").size(11.0).color(t::MUTED));
                ui.add(
                    egui::Label::new(RichText::new(&node.input).monospace())
                        .wrap(true)
                        .selectable(true),
                );
                if node.state != ToolState::Running {
                    ui.separator();
                    ui.label(RichText::new("结果").size(11.0).color(t::MUTED));
                    egui::ScrollArea::vertical()
                        .id_source((&node.call, "output"))
                        .max_height(240.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&node.meta).monospace())
                                    .wrap(true)
                                    .selectable(true),
                            );
                        });
                    if ui.small_button("复制结果").clicked() {
                        ui.ctx().output_mut(|o| o.copied_text = node.meta.clone());
                    }
                }
            });
        });
    ui.add_space(4.0);
}

fn render_diff(ui: &mut egui::Ui, d: &crate::events::Diff) {
    if d.removed.is_empty() && d.added.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!(
        "文件变更  +{}  −{}",
        d.added.len(),
        d.removed.len()
    ))
    .default_open(true)
    .show(ui, |ui| {
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for (lines, prefix, color, bg) in [
                    (&d.removed, "−", t::DEL_TX, t::DEL_BG),
                    (&d.added, "+", t::GRN_CHIP_TX, t::GRN_CHIP_BG),
                ] {
                    for line in lines {
                        egui::Frame::none()
                            .fill(bg)
                            .inner_margin(4.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("{prefix} {line}"))
                                            .monospace()
                                            .color(color),
                                    )
                                    .wrap(true)
                                    .selectable(true),
                                );
                            });
                    }
                }
            });
    });
    ui.add_space(6.0);
}

fn render_approval(ui: &mut egui::Ui, a: &Approval, clicks: &mut Vec<(String, bool)>) {
    egui::Frame::none()
        .fill(t::CODE_BG)
        .rounding(10.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
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
            ui.add_space(5.0);
            ui.horizontal(|ui| match a.responded {
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
            });
        });
    ui.add_space(6.0);
}

fn busy_line(ui: &mut egui::Ui, secs: u64, waiting: bool) {
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        if !waiting {
            ui.add(egui::Spinner::new().size(13.0).color(t::SKY));
        } else {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            icons::clock(&ui.painter_at(r), r.center(), 13.0, t::MUTED);
        }
        ui.label(
            RichText::new(format!(
                "{} · 用时 {} 秒",
                if waiting {
                    "等待你的审批"
                } else {
                    "生成中"
                },
                secs
            ))
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
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            let modal_open = model.modal.is_some();
            // 待发送附件预览(图片缩略图 / 文件芯片,可移除)
            if !model.pending.is_empty() {
                let mut remove: Option<usize> = None;
                ui.horizontal_wrapped(|ui| {
                    for (i, att) in model.pending.iter().enumerate() {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
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
                        let xresp = ui.interact(xr, ui.id().with(("rm_att", i)), Sense::click());
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
                if let Some(i) = remove {
                    model.remove_attachment(i);
                }
                ui.add_space(4.0);
            }
            // 在 TextEdit 消费按键前拦截 Enter:提交消息,Shift+Enter 保留换行
            let input_id = ui.make_persistent_id("message_input");
            let composition_frame = ui.input(|i| {
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::CompositionStart
                            | egui::Event::CompositionUpdate(_)
                            | egui::Event::CompositionEnd(_)
                    )
                })
            });
            let enter_now = if modal_open
                || model.composing
                || composition_frame
                || !ui.memory(|m| m.has_focus(input_id))
            {
                false
            } else {
                ui.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, Key::Enter) && !i.modifiers.shift
                })
            };
            let resp = ui.add(
                egui::TextEdit::multiline(&mut model.input)
                    .id(input_id)
                    .hint_text("发消息或创建任务 · Enter 发送，Shift+Enter 换行")
                    .desired_rows(2)
                    .frame(false)
                    .desired_width(f32::INFINITY),
            );
            // 无其它焦点控件时,输入框自动聚焦(主输入位;弹窗打开时不抢焦点)
            if !modal_open && ui.memory(|m| m.focused().is_none()) {
                resp.request_focus();
            }
            if enter_now {
                model.user_submit();
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
                widgets::chip(
                    ui,
                    if model.demo { "演示" } else { "NullClaw" },
                    Chip::Plain,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let busy = model.is_busy();
                    let send = ui.add_sized(
                        [34.0, 34.0],
                        Button::new(
                            RichText::new(if busy { "■" } else { "↑" })
                                .size(14.0)
                                .color(Color32::WHITE),
                        )
                        .fill(if busy { t::FAINT } else { t::SKY })
                        .rounding(Rounding::same(999.0)),
                    );
                    if send.clicked() && !modal_open {
                        if busy {
                            model.stop();
                        } else {
                            model.user_submit();
                        }
                    }
                });
            });
        });
}

fn status_bar(ui: &mut egui::Ui, model: &Model) {
    if model.history_blocked || model.settings.status.starts_with("会话保存失败") {
        ui.label(
            RichText::new("会话尚未保存，请在「更新与存储」中查看原因")
                .size(11.0)
                .color(t::DEL_TX),
        );
        return;
    }
    let steps = model.steps();
    let busy = model.is_busy();
    // 空闲且无统计信息时隐藏状态栏(保持界面干净)
    if !busy && steps == 0 && model.ctx_used <= 0.0 {
        return;
    }
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        let text = if busy {
            format!("生成中 · 用时 {} 秒", model.busy_secs())
        } else {
            let mut parts: Vec<String> = Vec::new();
            if steps > 0 {
                parts.push(format!("{} 次工具调用", steps));
            }
            if model.ctx_used > 0.0 && model.ctx_limit > 0.0 {
                let pct = (model.ctx_used / model.ctx_limit * 100.0) as u32;
                parts.push(format!(
                    "上下文 {}k ({}%)",
                    (model.ctx_used / 1000.0) as u32,
                    pct
                ));
            }
            parts.join(" · ")
        };
        if !text.is_empty() {
            ui.label(RichText::new(text).size(11.5).color(t::FAINT));
        }
    });
    ui.add_space(8.0);
}
