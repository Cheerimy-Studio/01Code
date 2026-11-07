//! 设置与信息弹窗(居中固定 + 半透明遮罩;设置页为「左导航 + 右卡片」布局)

use eframe::egui::{self, Align2, Color32, RichText, Rounding, Sense, Stroke, Vec2};

use crate::model::{Modal, Model};
use crate::settings;
use crate::theme as t;

pub fn show(ctx: &egui::Context, model: &mut Model) {
    if model.modal.is_none() {
        return;
    }
    // 半透明遮罩层(吞掉背后点击)
    egui::Area::new(egui::Id::new("o1_overlay"))
        .order(egui::Order::Middle)
        .fixed_pos(egui::Pos2::ZERO)
        .interactable(true)
        .show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let (rect, _) =
                ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            ui.painter().rect_filled(
                rect,
                0.0,
                Color32::from_rgba_unmultiplied(15, 23, 42, 96),
            );
        });
    match model.modal.clone() {
        Some(Modal::Settings) => settings_window(ctx, model),
        Some(Modal::Info) => info_window(ctx, model),
        None => {}
    }
}

fn nav_item(ui: &mut egui::Ui, label: &str, idx: usize, tab: &mut usize) {
    let selected = *tab == idx;
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 36.0),
        Sense::click(),
    );
    if selected || resp.hovered() {
        ui.painter().add(egui::Shape::rect_filled(
            rect,
            Rounding::same(8.0),
            if selected { t::HOVER } else { Color32::from_rgb(0xF8, 0xFA, 0xFC) },
        ));
    }
    let p = ui.painter_at(rect);
    p.text(
        egui::Pos2::new(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        if selected { t::TEXT } else { t::TEXT2 },
    );
    if resp.clicked() {
        *tab = idx;
    }
}

fn field_row(
    ui: &mut egui::Ui,
    label: &str,
    text: &mut String,
    password: bool,
    hint: &str,
    width: f32,
) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(96.0, 28.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(RichText::new(label).size(12.5).color(t::TEXT2));
            },
        );
        let mut te = egui::TextEdit::singleline(text).desired_width(width);
        if password {
            te = te.password(true);
        }
        if !hint.is_empty() {
            te = te.hint_text(hint);
        }
        ui.add(te);
    });
    ui.add_space(6.0);
}

fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(t::WHITE)
        .stroke(Stroke::new(1.0, t::BORDER))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::symmetric(16.0, 14.0))
}

fn settings_window(ctx: &egui::Context, model: &mut Model) {
    let mut open = true;
    egui::Window::new(RichText::new("设置").size(15.0).color(t::TEXT))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([780.0, 500.0])
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // 左侧导航
                ui.allocate_ui_with_layout(
                    Vec2::new(150.0, 360.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        nav_item(ui, "模型 API", 0, &mut model.settings.tab);
                        nav_item(ui, "Web 渠道", 1, &mut model.settings.tab);
                    },
                );
                // 垂直分隔线
                let top = ui.cursor().top();
                let x = ui.cursor().left() + 8.0;
                ui.painter().line_segment(
                    [egui::Pos2::new(x, top), egui::Pos2::new(x, top + 350.0)],
                    Stroke::new(1.0, t::BORDER),
                );
                ui.add_space(24.0);

                // 右侧内容
                ui.vertical(|ui| {
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("打开配置文件").size(12.0).color(t::TEXT2),
                                    )
                                    .fill(t::WHITE)
                                    .stroke(Stroke::new(1.0, t::BORDER))
                                    .rounding(Rounding::same(7.0)),
                                )
                                .clicked()
                            {
                                model.settings.status = settings::open_config();
                            }
                        },
                    );
                    ui.add_space(2.0);
                    let w = ui.available_width();
                    card_frame().show(ui, |ui| {
                        ui.set_width(w - 34.0);
                        if model.settings.tab == 0 {
                            ui.label(
                                RichText::new("模型 API").size(13.0).color(t::TEXT),
                            );
                            ui.add_space(8.0);
                            field_row(
                                ui,
                                "API 地址",
                                &mut model.settings.base_url,
                                false,
                                "",
                                400.0,
                            );
                            let key_hint = if model.settings.key_configured {
                                "已配置,留空不修改"
                            } else {
                                "sk-..."
                            };
                            field_row(
                                ui,
                                "API Key",
                                &mut model.settings.key_input,
                                true,
                                key_hint,
                                400.0,
                            );
                            field_row(
                                ui,
                                "模型",
                                &mut model.settings.model,
                                false,
                                "",
                                400.0,
                            );
                        } else {
                            ui.label(
                                RichText::new("Web 渠道").size(13.0).color(t::TEXT),
                            );
                            ui.add_space(8.0);
                            field_row(
                                ui,
                                "端口",
                                &mut model.settings.web_port,
                                false,
                                "",
                                120.0,
                            );
                            let token_hint = if model.settings.token_configured {
                                "已配置,留空不修改"
                            } else {
                                "随机长令牌"
                            };
                            field_row(
                                ui,
                                "通道令牌",
                                &mut model.settings.web_token_input,
                                true,
                                token_hint,
                                400.0,
                            );
                            ui.label(
                                RichText::new("保存后需重启网关生效").size(11.0).color(t::FAINT),
                            );
                        }
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("保存设置").size(12.5).color(t::TEXT2),
                                )
                                .fill(t::WHITE)
                                .stroke(Stroke::new(1.0, t::BORDER))
                                .rounding(Rounding::same(8.0)),
                            )
                            .clicked()
                        {
                            let ok = model.settings.save();
                            if ok {
                                model.model_name = model.settings.model.clone();
                            }
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("保存并重启网关")
                                        .size(12.5)
                                        .color(Color32::WHITE),
                                )
                                .fill(t::SKY)
                                .rounding(Rounding::same(8.0)),
                            )
                            .clicked()
                        {
                            if model.settings.save() {
                                model.model_name = model.settings.model.clone();
                                if let Some(bin) = model.settings.nullclaw.clone() {
                                    let port = crate::config::ClientCfg::load().gateway_port;
                                    model.settings.status =
                                        settings::restart_gateway(&bin, port);
                                    model.request_reconnect = true;
                                } else {
                                    model.settings.status =
                                        "未找到 nullclaw 可执行文件".into();
                                }
                            }
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("关闭").size(12.5).color(t::TEXT2),
                                )
                                .fill(t::WHITE)
                                .stroke(Stroke::new(1.0, t::BORDER))
                                .rounding(Rounding::same(8.0)),
                            )
                            .clicked()
                        {
                            model.modal = None;
                        }
                    });
                    ui.add_space(4.0);
                    if !model.settings.status.is_empty() {
                        ui.label(
                            RichText::new(&model.settings.status)
                                .size(11.5)
                                .color(t::MUTED),
                        );
                    }
                });
            });
        });
    if !open {
        model.modal = None;
    }
}

fn info_window(ctx: &egui::Context, model: &mut Model) {
    let mut open = true;
    egui::Window::new(RichText::new(&model.info_title).size(15.0).color(t::TEXT))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(600.0)
        .default_height(440.0)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&model.info_text)
                                .monospace()
                                .size(11.5)
                                .color(t::TEXT2),
                        )
                        .wrap(true),
                    );
                });
        });
    if !open {
        model.modal = None;
    }
}
