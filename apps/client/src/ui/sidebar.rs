use eframe::egui::{
    self, Button, Color32, FontId, Pos2, RichText, Rounding, Sense, Shape, Stroke, Vec2,
};

use crate::icons;
use crate::logo;
use crate::model::{Modal, Model, Session};

use crate::theme as t;
use crate::util::rel_time;

/// 会话行:状态点 + 标题(裁切) + 相对时间
fn session_row(
    ui: &mut egui::Ui,
    s: &Session,
    selected: bool,
    clicks: &mut Vec<String>,
    deletes: &mut Vec<String>,
) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::click());
    let bg = if selected {
        t::SELECTED
    } else if resp.hovered() {
        t::HOVER
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        ui.painter()
            .add(Shape::rect_filled(rect, Rounding::same(10.0), bg));
    }
    let cy = rect.center().y;
    icons::status_dot(
        &ui.painter_at(rect),
        Pos2::new(rect.left() + 12.0, cy),
        if s.done { t::FAINT } else { t::GRN },
    );
    // 标题区域裁切到时间标签之前,避免长标题压住时间
    let title_clip = egui::Rect::from_min_max(
        Pos2::new(rect.left() + 24.0, rect.top()),
        Pos2::new(rect.right() - 56.0, rect.bottom()),
    );
    let p_title = ui.painter_at(title_clip);
    p_title.text(
        Pos2::new(rect.left() + 24.0, cy),
        egui::Align2::LEFT_CENTER,
        &s.title,
        egui::FontId::proportional(13.0),
        if selected { t::TEXT } else { t::TEXT2 },
    );
    let p_time = ui.painter_at(rect);
    p_time.text(
        Pos2::new(rect.right() - 10.0, cy),
        egui::Align2::RIGHT_CENTER,
        rel_time(s.created_at),
        egui::FontId::proportional(11.0),
        t::FAINT,
    );
    if resp.clicked() {
        clicks.push(s.sid.clone());
    }
    resp.context_menu(|ui| {
        if ui
            .button(RichText::new("删除会话").color(t::DEL_TX))
            .clicked()
        {
            deletes.push(s.sid.clone());
            ui.close_menu();
        }
    });
}

fn menu_row(ui: &mut egui::Ui, label: &str, icon: fn(&egui::Painter, Pos2, f32, Color32)) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 40.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .add(Shape::rect_filled(rect, Rounding::same(10.0), t::HOVER));
    }
    let p = ui.painter_at(rect);
    let cy = rect.center().y;
    icon(&p, Pos2::new(rect.left() + 12.0, cy), 14.0, t::MUTED);
    p.text(
        Pos2::new(rect.left() + 28.0, cy),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        t::TEXT2,
    );
    resp.clicked()
}

/// 小图标按钮(工作区头部)
fn icon_button(ui: &mut egui::Ui, icon: fn(&egui::Painter, Pos2, f32, Color32)) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .add(Shape::rect_filled(rect, Rounding::same(6.0), t::HOVER));
    }
    icon(&ui.painter_at(rect), rect.center(), 13.0, t::MUTED);
    resp.clicked()
}

pub fn show(ctx: &egui::Context, model: &mut Model) {
    let mut clicks: Vec<String> = Vec::new();
    let mut deletes: Vec<String> = Vec::new();
    let mut open_account = false;
    egui::SidePanel::left("left")
        .exact_width(248.0)
        .resizable(false)
        .frame(
            egui::Frame::none()
                .fill(t::PANEL)
                .stroke(Stroke::new(1.0_f32, t::BORDER))
                .inner_margin(egui::Margin {
                    left: 14.0,
                    right: 14.0,
                    top: 14.0,
                    bottom: 0.0,
                }),
        )
        .show(ctx, |ui| {
            // 品牌行:手动绘制,logo / 名称 / 徽标全部垂直居中
            let (brand_rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
            {
                let p = ui.painter_at(brand_rect);
                let cy = brand_rect.center().y;
                logo::paint_tile(
                    &p,
                    egui::Rect::from_min_size(
                        Pos2::new(brand_rect.left(), cy - 13.0),
                        Vec2::splat(26.0),
                    ),
                );
                let name = p.layout_no_wrap("01 编码".into(), t::bold(15.5), t::TEXT);
                let tx = brand_rect.left() + 34.0;
                let ty = cy - name.size().y * 0.5;
                p.galley(Pos2::new(tx, ty), name.clone(), t::TEXT);
                // Code 徽标(整体下移 1px,与文字视觉对齐)
                let bg = p.layout_no_wrap("Code".into(), FontId::proportional(8.5), Color32::WHITE);
                let bw = bg.size().x + 10.0;
                let bh = 16.0;
                let bx = tx + name.size().x + 8.0;
                let brect = egui::Rect::from_min_size(
                    Pos2::new(bx, cy - bh * 0.5 + 1.0),
                    Vec2::new(bw, bh),
                );
                p.add(Shape::rect_filled(
                    brect,
                    Rounding::same(3.5),
                    Color32::from_rgb(0x0F, 0x17, 0x2A),
                ));
                p.galley(
                    Pos2::new(bx + 5.0, cy - bg.size().y * 0.5 + 1.0),
                    bg,
                    Color32::WHITE,
                );
            }
            ui.add_space(12.0);

            // 新会话
            let w = ui.available_width();
            if ui
                .add_sized(
                    [w, 42.0],
                    Button::new(RichText::new("＋  新会话").size(13.5).color(t::TEXT))
                        .fill(t::WHITE)
                        .stroke(Stroke::new(1.0_f32, t::BORDER))
                        .rounding(Rounding::same(10.0)),
                )
                .clicked()
            {
                model.new_task();
            }
            ui.add_space(8.0);

            if menu_row(ui, "扩展", icons::grid4) {
                model.settings.cli("扩展", vec!["capabilities".into()]);
            }
            if menu_row(ui, "定时任务", icons::clock) {
                model.settings.tab = 1;
                model.settings.clear_status();
                model.modal = Some(Modal::Settings);
            }
            if menu_row(ui, "设置", icons::gear) {
                model.settings.clear_status();
                model.modal = Some(Modal::Settings);
            }
            ui.add_space(6.0);

            // 工作区头部:标题 + 搜索图标
            ui.horizontal(|ui| {
                ui.label(RichText::new("工作区").size(11.0).color(t::FAINT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, icons::search) {
                        model.show_search = !model.show_search;
                    }
                });
            });

            if model.show_search {
                let w = ui.available_width();
                egui::Frame::none()
                    .fill(t::WHITE)
                    .stroke(Stroke::new(1.0_f32, t::BORDER))
                    .rounding(Rounding::same(9.0))
                    .inner_margin(egui::Margin::symmetric(9.0, 5.0))
                    .show(ui, |ui| {
                        ui.set_width(w - 20.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut model.search)
                                .hint_text("搜索会话")
                                .frame(false)
                                .desired_width(f32::INFINITY),
                        );
                    });
                ui.add_space(2.0);
            }

            // 会话列表(为底部用户行预留高度,避免被挤压截断)
            let user_h = 66.0;
            let scroll_h = (ui.available_height() - user_h).max(60.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(scroll_h)
                .show(ui, |ui| {
                    let groups = model.groups();
                    let sessions = model.sessions.clone();
                    for (ws_name, idxs) in groups {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(2.0);
                            let (r, _) =
                                ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
                            icons::folder(ui.painter(), r.center(), 12.0, t::MUTED);
                            ui.label(RichText::new(&ws_name).size(12.0).color(t::TEXT2));
                        });
                        for idx in idxs {
                            let s = &sessions[idx];
                            let selected = s.sid == model.active_sid;
                            session_row(ui, s, selected, &mut clicks, &mut deletes);
                        }
                    }
                    if sessions.is_empty() {
                        ui.add_space(10.0);
                        ui.label(RichText::new("暂无会话").size(12.0).color(t::FAINT));
                    }
                });

            // 底部用户行：整行可点击，头像与两行文字在区域内垂直居中
            let (rect, resp) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), user_h), Sense::click());
            let p = ui.painter_at(rect);
            let sep_y = rect.top() + 0.5;
            if resp.hovered() {
                p.add(Shape::rect_filled(
                    rect.shrink2(Vec2::new(0.0, 5.0)),
                    Rounding::same(8.0),
                    t::HOVER,
                ));
            }
            let cy = (sep_y + rect.bottom() - 8.0) * 0.5;
            let connected = model.settings.key_configured;
            let ax = rect.left() + 16.0;
            p.add(Shape::circle_filled(
                Pos2::new(ax, cy),
                15.0,
                if connected { t::SKY } else { t::SELECTED },
            ));
            icons::user(
                &p,
                Pos2::new(ax, cy),
                15.0,
                if connected { Color32::WHITE } else { t::MUTED },
            );
            // 连接状态点：仅在线时显示，白描边压住头像边缘
            if connected {
                let dot = Pos2::new(ax + 10.5, cy + 10.5);
                p.add(Shape::circle_filled(dot, 4.8, t::PANEL));
                p.add(Shape::circle_filled(dot, 3.1, t::GRN));
            }
            let account_name = model.settings.account_name();
            let name = if connected {
                if account_name.is_empty() {
                    "已登录账户".to_string()
                } else {
                    account_name
                }
            } else {
                "未登录".to_string()
            };
            let sub = if connected {
                if model.settings.account.is_null() {
                    "正在同步账户…"
                } else {
                    "已连接言灵中转"
                }
            } else {
                "点击登录言灵中转"
            };
            let clip = egui::Rect::from_min_max(
                Pos2::new(rect.left() + 40.0, rect.top()),
                Pos2::new(rect.right() - 34.0, rect.bottom()),
            );
            let pt = ui.painter_at(clip);
            // 按实际字形高度居中两行文字，避免固定偏移在不同字号/缩放下偏低
            let name_gal = pt.layout_no_wrap(name, egui::FontId::proportional(13.5), t::TEXT);
            let sub_gal =
                pt.layout_no_wrap(sub.to_string(), egui::FontId::proportional(11.0), t::FAINT);
            let gap = 3.0;
            let total = name_gal.size().y + gap + sub_gal.size().y;
            let top = cy - total * 0.5;
            pt.galley(
                Pos2::new(rect.left() + 40.0, top),
                name_gal.clone(),
                t::TEXT,
            );
            pt.galley(
                Pos2::new(rect.left() + 40.0, top + name_gal.size().y + gap),
                sub_gal,
                t::FAINT,
            );
            icons::gear(
                &p,
                Pos2::new(rect.right() - 15.0, cy),
                13.0,
                if resp.hovered() { t::MUTED } else { t::FAINT },
            );
            if resp.clicked() {
                open_account = true;
            }
        });

    if open_account {
        model.settings.tab = 0;
        model.settings.clear_status();
        model.modal = Some(Modal::Settings);
    }
    for sid in clicks {
        model.switch_session(&sid);
    }
    for sid in deletes {
        model.delete_session(&sid);
    }
}
