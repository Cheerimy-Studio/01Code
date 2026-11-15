use crate::{
    icons,
    model::{Modal, Model},
    theme as t,
};
use chrono::TimeZone;
use eframe::egui::{self, Align2, RichText, Sense, Stroke, Vec2};

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
fn status_color(status: &str) -> egui::Color32 {
    const BAD: [&str; 9] = [
        "失败",
        "错误",
        "过期",
        "拒绝",
        "不可用",
        "不稳定",
        "无效",
        "不一致",
        "未启用",
    ];
    if BAD.iter().any(|k| status.contains(k)) {
        t::DEL_TX
    } else {
        t::MUTED
    }
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
                for (i, label) in ["账户", "运行环境"].iter().enumerate() {
                    if ui.selectable_label(s.tab == i, *label).clicked() {
                        s.tab = i;
                        s.clear_status();
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
                            ui.horizontal(|ui| {
                                ui.heading(if s.key_configured {
                                    "言灵中转"
                                } else {
                                    "连接言灵中转"
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if s.key_configured {
                                            egui::Frame::none()
                                                .fill(t::GRN_CHIP_BG)
                                                .stroke(Stroke::new(1.0_f32, t::GRN_CHIP_BR))
                                                .rounding(999.0)
                                                .inner_margin(egui::Margin::symmetric(9.0, 3.0))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        RichText::new("已连接")
                                                            .size(11.0)
                                                            .color(t::GRN_CHIP_TX),
                                                    );
                                                });
                                        }
                                    },
                                );
                            });
                            if !s.key_configured {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("浏览器确认验证码即可登录，无需填写 API Key")
                                        .size(12.0)
                                        .color(t::MUTED),
                                );
                            }
                            if s.key_configured && !s.logging_in {
                                ui.add_space(14.0);
                                let name = s.account_name();
                                let name = if name.is_empty() {
                                    "已登录账户".to_string()
                                } else {
                                    name
                                };
                                let balance = s.account["balance"].as_f64().unwrap_or(0.0);
                                egui::Frame::none()
                                    .fill(t::PANEL)
                                    .stroke(Stroke::new(1.0_f32, t::BORDER))
                                    .rounding(12.0)
                                    .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.allocate_ui_with_layout(
                                            Vec2::new(ui.available_width(), 53.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                let (r, _) = ui.allocate_exact_size(
                                                    Vec2::splat(40.0),
                                                    Sense::hover(),
                                                );
                                                ui.painter().circle_filled(
                                                    r.center(),
                                                    20.0,
                                                    t::SKY_CHIP_BG,
                                                );
                                                icons::user(
                                                    &ui.painter_at(r),
                                                    r.center(),
                                                    20.0,
                                                    t::SKY_DEEP,
                                                );
                                                ui.add_space(12.0);
                                                ui.vertical(|ui| {
                                                    ui.add_space(5.0);
                                                    ui.label(
                                                        RichText::new(name)
                                                            .size(15.5)
                                                            .color(t::TEXT)
                                                            .strong(),
                                                    );
                                                    ui.add_space(3.0);
                                                    egui::Frame::none()
                                                        .fill(t::SELECTED)
                                                        .stroke(Stroke::new(1.0_f32, t::BORDER))
                                                        .rounding(6.0)
                                                        .inner_margin(egui::Margin::symmetric(
                                                            7.0, 2.0,
                                                        ))
                                                        .show(ui, |ui| {
                                                            ui.label(
                                                                RichText::new(
                                                                    if s.account.is_null() {
                                                                        "正在同步…".to_string()
                                                                    } else {
                                                                        format!(
                                                                            "余额 ¥{balance:.2}"
                                                                        )
                                                                    },
                                                                )
                                                                .size(10.5)
                                                                .color(t::MUTED),
                                                            );
                                                        });
                                                });
                                            },
                                        );
                                    });
                            }
                            ui.add_space(14.0);
                            egui::Frame::none().show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if s.key_configured && !s.logging_in {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 10.0;
                                        ui.add_enabled_ui(!s.busy && !s.syncing, |ui| {
                                            if ui
                                                .add_sized(
                                                    [104.0, 34.0],
                                                    egui::Button::new(
                                                        RichText::new("重新登录")
                                                            .size(12.5)
                                                            .color(t::TEXT2),
                                                    )
                                                    .fill(t::WHITE)
                                                    .stroke(Stroke::new(1.0_f32, t::BORDER))
                                                    .rounding(8.0),
                                                )
                                                .clicked()
                                            {
                                                s.login();
                                            }
                                            if ui
                                                .add_sized(
                                                    [92.0, 34.0],
                                                    egui::Button::new(
                                                        RichText::new("退出登录")
                                                            .size(12.5)
                                                            .color(t::DEL_TX),
                                                    )
                                                    .fill(t::WHITE)
                                                    .stroke(Stroke::new(1.0_f32, t::BORDER))
                                                    .rounding(8.0),
                                                )
                                                .clicked()
                                            {
                                                s.logout();
                                            }
                                            if ui
                                                .add_sized(
                                                    [92.0, 34.0],
                                                    egui::Button::new(
                                                        RichText::new("刷新账户")
                                                            .size(12.5)
                                                            .color(t::TEXT2),
                                                    )
                                                    .fill(t::WHITE)
                                                    .stroke(Stroke::new(1.0_f32, t::BORDER))
                                                    .rounding(8.0),
                                                )
                                                .clicked()
                                            {
                                                s.sync_account();
                                            }
                                        });
                                    });
                                } else if s.logging_in {
                                    let has_url = !s.verification_uri.is_empty();
                                    ui.add_enabled_ui(has_url, |ui| {
                                        if ui
                                            .add_sized(
                                                [200.0, 42.0],
                                                egui::Button::new(
                                                    RichText::new(if has_url {
                                                        "在浏览器中打开"
                                                    } else {
                                                        "正在获取验证码…"
                                                    })
                                                    .size(13.5)
                                                    .color(egui::Color32::WHITE),
                                                )
                                                .fill(if has_url { t::SKY } else { t::FAINT })
                                                .rounding(9.0),
                                            )
                                            .clicked()
                                        {
                                            let _ = open::that(&s.verification_uri);
                                        }
                                    });
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(if has_url {
                                            "点击上方按钮在浏览器中打开授权页"
                                        } else {
                                            "验证码生成后，可点击上方按钮打开授权页"
                                        })
                                        .size(11.0)
                                        .color(t::FAINT),
                                    );
                                } else {
                                    ui.add_enabled_ui(!s.busy && !s.syncing, |ui| {
                                        if ui
                                            .add_sized(
                                                [200.0, 42.0],
                                                egui::Button::new(
                                                    RichText::new("在浏览器中登录")
                                                        .size(13.5)
                                                        .color(egui::Color32::WHITE),
                                                )
                                                .fill(t::SKY)
                                                .rounding(9.0),
                                            )
                                            .clicked()
                                        {
                                            s.login();
                                        }
                                    });
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new("点击上方按钮在浏览器中打开授权页")
                                            .size(11.0)
                                            .color(t::FAINT),
                                    );
                                }
                            });
                            if !s.device_code.is_empty() {
                                ui.add_space(12.0);
                                egui::Frame::none()
                                    .fill(t::PANEL)
                                    .stroke(Stroke::new(1.0_f32, t::BORDER))
                                    .rounding(12.0)
                                    .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new("浏览器验证码")
                                                    .size(11.5)
                                                    .color(t::FAINT),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if let Some(rem) = s.device_remaining() {
                                                        let color = if rem < 60 {
                                                            t::DEL_TX
                                                        } else {
                                                            t::MUTED
                                                        };
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "剩余 {:02}:{:02}",
                                                                rem / 60,
                                                                rem % 60
                                                            ))
                                                            .size(11.5)
                                                            .color(color),
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(3.0);
                                        ui.label(
                                            RichText::new(&s.device_code)
                                                .size(30.0)
                                                .monospace()
                                                .strong()
                                                .color(t::TEXT),
                                        );
                                        ui.add_space(3.0);
                                        ui.horizontal(|ui| {
                                            if s.busy {
                                                ui.add(
                                                    egui::Spinner::new().size(14.0).color(t::MUTED),
                                                );
                                                ui.add_space(1.0);
                                                ui.label(
                                                    RichText::new("等待浏览器确认…")
                                                        .size(11.5)
                                                        .color(t::MUTED),
                                                );
                                            } else {
                                                ui.label(
                                                    RichText::new(
                                                        "在浏览器中确认后，此窗口会自动完成登录",
                                                    )
                                                    .size(11.0)
                                                    .color(t::FAINT),
                                                );
                                            }
                                        });
                                        ui.add_space(10.0);
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new("取消登录")
                                                                .size(12.0)
                                                                .color(t::MUTED),
                                                        )
                                                        .fill(t::WHITE)
                                                        .stroke(Stroke::new(1.0_f32, t::BORDER))
                                                        .rounding(7.0)
                                                        .min_size(Vec2::new(76.0, 30.0)),
                                                    )
                                                    .clicked()
                                                {
                                                    s.cancel_login();
                                                }
                                            },
                                        );
                                    });
                            }
                            if s.key_configured && !s.devices.is_empty() {
                                ui.add_space(14.0);
                                ui.label(
                                    RichText::new("已授权设备")
                                        .size(12.5)
                                        .strong()
                                        .color(t::TEXT),
                                );
                                for d in &s.devices {
                                    let created = d["created_at"].as_i64().unwrap_or(0);
                                    let when = chrono::Local
                                        .timestamp_opt(created, 0)
                                        .single()
                                        .map(|v| v.format("%Y-%m-%d %H:%M").to_string())
                                        .unwrap_or_default();
                                    let ip = d["device_ip"].as_str().unwrap_or("—");
                                    let name = d["name"].as_str().unwrap_or("01Code");
                                    let name = if name.starts_with("01Code-设备") {
                                        "01Code"
                                    } else {
                                        name
                                    };
                                    egui::Frame::none()
                                        .fill(t::PANEL)
                                        .stroke(Stroke::new(1.0_f32, t::BORDER))
                                        .rounding(9.0)
                                        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(name)
                                                        .size(12.0)
                                                        .strong()
                                                        .color(t::TEXT),
                                                );
                                                if d["current"].as_bool().unwrap_or(false) {
                                                    ui.label(
                                                        RichText::new("当前设备")
                                                            .size(10.0)
                                                            .color(t::GRN_CHIP_TX),
                                                    );
                                                }
                                                if !d["enabled"].as_bool().unwrap_or(true) {
                                                    ui.label(
                                                        RichText::new("已撤销")
                                                            .size(10.0)
                                                            .color(t::DEL_TX),
                                                    );
                                                }
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        ui.label(
                                                            RichText::new(when)
                                                                .size(10.5)
                                                                .color(t::FAINT),
                                                        );
                                                    },
                                                );
                                            });
                                            ui.label(
                                                RichText::new(format!("来源 IP {}", ip))
                                                    .size(10.5)
                                                    .color(t::FAINT),
                                            );
                                        });
                                    ui.add_space(5.0);
                                }
                            }
                        }
                        _ => {
                            ui.horizontal(|ui| {
                                ui.heading("NullClaw");
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let (text, bg, br, tx) = if s.runtime_running {
                                            (
                                                "运行中",
                                                t::GRN_CHIP_BG,
                                                t::GRN_CHIP_BR,
                                                t::GRN_CHIP_TX,
                                            )
                                        } else {
                                            ("已暂停", t::PANEL, t::BORDER, t::MUTED)
                                        };
                                        egui::Frame::none()
                                            .fill(bg)
                                            .stroke(Stroke::new(1.0_f32, br))
                                            .rounding(999.0)
                                            .inner_margin(egui::Margin::symmetric(9.0, 3.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(text).size(11.0).color(tx));
                                            });
                                    },
                                );
                            });
                            ui.add_space(10.0);
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
                                            egui::Button::new(
                                                RichText::new(if s.runtime_running {
                                                    "重新启动"
                                                } else {
                                                    "启动"
                                                })
                                                .color(if s.runtime_running {
                                                    t::TEXT2
                                                } else {
                                                    egui::Color32::WHITE
                                                }),
                                            )
                                            .fill(if s.runtime_running { t::WHITE } else { t::SKY })
                                            .stroke(Stroke::new(1.0_f32, t::BORDER))
                                            .rounding(9.0),
                                        )
                                        .clicked()
                                    {
                                        s.save();
                                    }
                                    ui.add_enabled_ui(s.runtime_running, |ui| {
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
                                            s.stop_runtime();
                                        }
                                    });
                                });
                                ui.add_space(16.0);
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
                if s.busy || s.syncing {
                    ui.spinner();
                }
                ui.label(
                    RichText::new(&s.status)
                        .size(12.0)
                        .color(status_color(&s.status)),
                );
            });
        });
    // 遮罩也是 Middle 层，点击它时会被置顶；每帧把窗口层拉回最上方
    ctx.move_to_top(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("设置"),
    ));
    if close || !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        model.modal = None;
    }
}
fn info_window(ctx: &egui::Context, model: &mut Model) {
    let mut close = false;
    let size = ctx.screen_rect().size();
    egui::Window::new("info-window")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
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
        .default_width((size.x - 80.0).min(720.0))
        .max_height((size.y - 120.0).max(200.0))
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&model.info_title)
                        .size(18.0)
                        .color(t::TEXT)
                        .strong(),
                );
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
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);
            egui::ScrollArea::both()
                .max_height((size.y - 230.0).max(120.0))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&model.info_text).monospace())
                            .selectable(true),
                    );
                });
        });
    ctx.move_to_top(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("info-window"),
    ));
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        model.modal = model.info_return.take();
    }
}
