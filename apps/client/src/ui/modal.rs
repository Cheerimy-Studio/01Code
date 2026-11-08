use crate::{
    model::{Modal, Model},
    theme as t,
};
use eframe::egui::{self, Align2, RichText, Stroke, Vec2};

pub fn show(ctx: &egui::Context, model: &mut Model) {
    if model.modal.is_some() {
        egui::Area::new(egui::Id::new("modal-mask"))
            .order(egui::Order::Middle)
            .fixed_pos(ctx.screen_rect().min)
            .show(ctx, |ui| {
                let (r, _) =
                    ui.allocate_exact_size(ctx.screen_rect().size(), egui::Sense::click_and_drag());
                ui.painter()
                    .rect_filled(r, 0.0, egui::Color32::from_black_alpha(60));
            });
    }
    match model.modal.clone() {
        Some(Modal::Settings) => settings_window(ctx, model),
        Some(Modal::Info) => info_window(ctx, model),
        None => {}
    }
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, password: bool, hint: &str) {
    ui.label(RichText::new(label).size(12.0).color(t::MUTED));
    let width = ui.available_width();
    ui.add_sized(
        [width, 34.0],
        egui::TextEdit::singleline(value)
            .password(password)
            .hint_text(hint),
    );
    ui.add_space(8.0);
}
fn settings_window(ctx: &egui::Context, model: &mut Model) {
    let screen = ctx.screen_rect().size();
    let width = (screen.x - 80.0).clamp(280.0, 720.0);
    let max_h = (screen.y - 140.0).clamp(240.0, 600.0);
    let mut open = true;
    let mut close = false;
    egui::Window::new("设置")
        .open(&mut open)
        .title_bar(false)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(t::WHITE)
                .rounding(12.0)
                .shadow(egui::epaint::Shadow {
                    offset: Vec2::new(0.0, 8.0),
                    blur: 26.0,
                    spread: 0.0,
                    color: egui::Color32::from_rgba_unmultiplied(15, 23, 42, 32),
                })
                .inner_margin(16.0),
        )
        .collapsible(false)
        .resizable(false)
        .default_width(width)
        .max_width(width)
        .max_height(max_h)
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 48.0))
        .constrain_to(ctx.screen_rect())
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("设置").size(20.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("关闭").size(12.5).color(t::TEXT2))
                                .fill(t::WHITE)
                                .stroke(Stroke::new(1.0_f32, t::BORDER))
                                .rounding(8.0)
                                .min_size(Vec2::new(72.0, 30.0)),
                        )
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            ui.add_space(14.0);
            let s = &mut model.settings;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                for (i, label) in ["账户", "模型", "运行环境"].iter().enumerate() {
                    if ui.selectable_label(s.tab == i, *label).clicked() {
                        s.tab = i;
                    }
                }
            });
            ui.add_space(8.0);
            ui.separator();
            egui::ScrollArea::vertical()
                .id_source("settings-content")
                .max_height((max_h - 150.0).max(120.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_min_width((width - 28.0).max(240.0));
                    ui.add_space(16.0);
                    match s.tab {
                        0 => {
                            ui.heading(if s.key_configured {
                                "言灵中转"
                            } else {
                                "连接言灵中转"
                            });
                            ui.add_space(12.0);
                            if let Some(name) = s.account["username"].as_str() {
                                ui.label(name);
                                ui.label(format!(
                                    "余额  ¥{:.4}",
                                    s.account["balance"].as_f64().unwrap_or(0.0)
                                ));
                            }
                            ui.add_enabled_ui(!s.busy, |ui| {
                                if ui
                                    .add_sized(
                                        [180.0, 40.0],
                                        egui::Button::new(
                                            RichText::new(if s.key_configured {
                                                "重新登录"
                                            } else {
                                                "在浏览器中登录"
                                            })
                                            .color(egui::Color32::WHITE),
                                        )
                                        .fill(t::SKY)
                                        .rounding(8.0),
                                    )
                                    .clicked()
                                {
                                    s.login();
                                }
                            });
                            if !s.device_code.is_empty() {
                                ui.add_space(12.0);
                                ui.label("确认浏览器中的验证码");
                                ui.label(RichText::new(&s.device_code).size(24.0).monospace());
                                ui.horizontal(|ui| {
                                    if ui.button("打开授权页").clicked() {
                                        let _ = open::that(&s.verification_uri);
                                    }
                                    if ui.button("取消").clicked() {
                                        s.cancel_login();
                                    }
                                });
                            }
                            ui.add_space(16.0);
                            ui.collapsing("服务地址", |ui| {
                                ui.add_enabled_ui(!s.busy, |ui| {
                                    field(
                                        ui,
                                        "API 地址",
                                        &mut s.base_url,
                                        false,
                                        "https://ai.gzmath.cn/v1",
                                    );
                                });
                            });
                            if s.key_configured {
                                ui.add_space(12.0);
                                ui.add_enabled_ui(!s.busy, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 12.0;
                                        if ui.button("刷新账户").clicked() {
                                            s.sync();
                                        }
                                        if ui.button("撤销设备登录").clicked() {
                                            s.logout();
                                        }
                                    });
                                });
                            }
                            if let Some(rows) = s.usage.as_array() {
                                ui.add_space(20.0);
                                ui.label(RichText::new("近 7 天用量").strong());
                                egui::Grid::new("usage")
                                    .num_columns(3)
                                    .spacing([24.0, 8.0])
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("日期").size(11.5).color(t::FAINT));
                                        ui.label(RichText::new("请求").size(11.5).color(t::FAINT));
                                        ui.label(RichText::new("费用").size(11.5).color(t::FAINT));
                                        ui.end_row();
                                        for r in rows {
                                            ui.label(r["day"].as_str().unwrap_or(""));
                                            ui.label(r["requests"].to_string());
                                            ui.label(format!(
                                                "¥{:.4}",
                                                r["cost"].as_f64().unwrap_or(0.0)
                                            ));
                                            ui.end_row();
                                        }
                                    });
                            }
                        }
                        1 => {
                            ui.heading("模型");
                            ui.add_space(14.0);
                            ui.add_enabled_ui(!s.busy, |ui| {
                                ui.label(RichText::new("当前模型").size(11.5).color(t::FAINT));
                                ui.add_space(6.0);
                                egui::Frame::none()
                                    .fill(t::WHITE)
                                    .stroke(Stroke::new(1.0_f32, t::BORDER))
                                    .rounding(8.0)
                                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width() - 8.0);
                                        if s.models.is_empty() {
                                            ui.add(
                                                egui::TextEdit::singleline(&mut s.model)
                                                    .frame(false)
                                                    .hint_text("输入模型名称")
                                                    .desired_width(f32::INFINITY),
                                            );
                                        } else {
                                            egui::ComboBox::from_id_source("gateway-model")
                                                .selected_text(&s.model)
                                                .width(ui.available_width() - 12.0)
                                                .show_ui(ui, |ui| {
                                                    for m in &s.models {
                                                        ui.selectable_value(
                                                            &mut s.model,
                                                            m.clone(),
                                                            m,
                                                        );
                                                    }
                                                });
                                        }
                                    });
                                ui.add_space(14.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 12.0;
                                    if ui
                                        .add_sized(
                                            [108.0, 32.0],
                                            egui::Button::new(
                                                RichText::new("应用模型").color(t::WHITE),
                                            )
                                            .fill(t::TEXT)
                                            .rounding(9.0),
                                        )
                                        .clicked()
                                    {
                                        s.save();
                                    }
                                    if ui
                                        .add_sized(
                                            [120.0, 32.0],
                                            egui::Button::new(
                                                RichText::new("同步模型列表").color(t::TEXT2),
                                            )
                                            .fill(t::WHITE)
                                            .stroke(Stroke::new(1.0_f32, t::BORDER))
                                            .rounding(9.0),
                                        )
                                        .clicked()
                                    {
                                        s.sync();
                                    }
                                });
                            });
                        }
                        _ => {
                            ui.heading("NullClaw");
                            ui.add_space(12.0);
                            ui.add_enabled_ui(!s.busy, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 12.0;
                                    if ui
                                        .add_sized(
                                            [104.0, 32.0],
                                            egui::Button::new("检查并更新")
                                                .fill(t::WHITE)
                                                .stroke(Stroke::new(1.0_f32, t::BORDER))
                                                .rounding(9.0),
                                        )
                                        .clicked()
                                    {
                                        s.update_runtime();
                                    }
                                    if ui
                                        .add_sized(
                                            [92.0, 32.0],
                                            egui::Button::new("重新启动")
                                                .fill(t::WHITE)
                                                .stroke(Stroke::new(1.0_f32, t::BORDER))
                                                .rounding(9.0),
                                        )
                                        .clicked()
                                    {
                                        s.save();
                                    }
                                    if ui
                                        .add_sized(
                                            [72.0, 32.0],
                                            egui::Button::new("停止")
                                                .fill(t::WHITE)
                                                .stroke(Stroke::new(1.0_f32, t::BORDER))
                                                .rounding(9.0),
                                        )
                                        .clicked()
                                    {
                                        let _ = crate::runtime::stop();
                                        s.runtime_status = "已停止".into();
                                    }
                                });
                                ui.add_space(16.0);
                                ui.checkbox(&mut s.workspace_only, "仅允许访问工作区");
                                ui.add_space(12.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 12.0;
                                    for (label, args) in [
                                        ("诊断", vec!["doctor"]),
                                        ("能力", vec!["capabilities"]),
                                        ("扩展", vec!["skills", "list"]),
                                        ("定时任务", vec!["cron", "list"]),
                                        ("状态", vec!["status"]),
                                    ] {
                                        if ui
                                            .add_sized(
                                                [78.0, 30.0],
                                                egui::Button::new(label)
                                                    .fill(t::WHITE)
                                                    .stroke(Stroke::new(1.0_f32, t::BORDER))
                                                    .rounding(8.0),
                                            )
                                            .clicked()
                                        {
                                            s.cli(
                                                label,
                                                args.into_iter().map(String::from).collect(),
                                            );
                                        }
                                    }
                                });
                                ui.add_space(16.0);
                                ui.collapsing("添加定时任务", |ui| {
                                    field(
                                        ui,
                                        "Cron 表达式",
                                        &mut s.cron_schedule,
                                        false,
                                        "0 9 * * *",
                                    );
                                    field(ui, "任务内容", &mut s.cron_prompt, false, "输入任务");
                                    if ui.button("添加").clicked()
                                        && !s.cron_schedule.trim().is_empty()
                                        && !s.cron_prompt.trim().is_empty()
                                    {
                                        s.cli(
                                            "定时任务",
                                            vec![
                                                "cron".into(),
                                                "add-agent".into(),
                                                s.cron_schedule.clone(),
                                                s.cron_prompt.clone(),
                                            ],
                                        );
                                    }
                                    field(ui, "任务 ID", &mut s.cron_id, false, "");
                                    if ui.button("删除任务").clicked()
                                        && !s.cron_id.trim().is_empty()
                                    {
                                        s.cli(
                                            "定时任务",
                                            vec!["cron".into(), "remove".into(), s.cron_id.clone()],
                                        );
                                    }
                                });
                                ui.collapsing("高级", |ui| {
                                    if ui.button("打开运行配置").clicked() {
                                        let _ =
                                            open::that(crate::runtime::home().join("config.json"));
                                    }
                                    if ui.button("打开运行日志").clicked() {
                                        let _ =
                                            open::that(crate::runtime::home().join("runtime.log"));
                                    }
                                    if ui.button("打开安装日志").clicked() {
                                        let _ = open::that(crate::runtime::install_log_path());
                                    }
                                    if ui.button("打开工作区").clicked() {
                                        let dir = crate::runtime::home().join("workspace");
                                        let _ = std::fs::create_dir_all(&dir);
                                        let _ = open::that(dir);
                                    }
                                });
                            });
                        }
                    }
                });
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if s.busy {
                    ui.spinner();
                }
                ui.label(RichText::new(&s.status).size(12.0).color(t::MUTED));
            });
        });
    if close || !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        model.modal = None;
    }
}
fn info_window(ctx: &egui::Context, model: &mut Model) {
    let mut open = true;
    let size = ctx.screen_rect().size();
    egui::Window::new(&model.info_title)
        .open(&mut open)
        .default_width((size.x - 80.0).min(720.0))
        .max_height((size.y - 100.0).max(180.0))
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(&model.info_text).monospace()).selectable(true),
                );
            });
        });
    if !open {
        model.modal = None;
    }
}
