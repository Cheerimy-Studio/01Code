use crate::{
    model::{Modal, Model},
    settings::{self, Upstream},
    theme as t,
};
use eframe::egui::{self, RichText, Vec2};

const SETTING_TABS: [(&str, usize); 4] = [
    ("上游与模型", 0),
    ("网关管理", 3),
    ("更新与存储", 4),
    ("任务与扩展", 5),
];
fn settings_content(ui: &mut egui::Ui, model: &mut Model) {
    match model.settings.tab {
        0 => upstreams(ui, model),
        1 | 2 | 3 => gateway(ui, model),
        4 => updates(ui, model),
        _ => tasks(ui, model),
    }
}
pub fn show(ctx: &egui::Context, model: &mut Model) {
    let Some(modal) = model.modal.clone() else {
        return;
    };
    // 直连模式与 Agent 预设已隐藏,回落到首个标签
    if matches!(model.settings.tab, 1 | 2) {
        model.settings.tab = 0;
    }
    let screen = ctx.screen_rect();
    let size = Vec2::new(
        (screen.width() - 40.0).min(960.0),
        (screen.height() - 40.0).min(680.0),
    );
    let inner = size - Vec2::splat(40.0);
    egui::Area::new(egui::Id::new("settings_shade"))
        .order(egui::Order::Middle)
        .constrain(false)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
            ui.painter()
                .rect_filled(r, 0.0, egui::Color32::from_black_alpha(65));
        });
    let mut close = false;
    egui::Area::new(egui::Id::new("01_settings"))
        .order(egui::Order::Foreground)
        .constrain(false)
        .fixed_pos(screen.center() - size * 0.5)
        .show(ctx, |ui| {
            egui::Frame::window(&ctx.style())
                .fill(t::WHITE)
                .rounding(22.0)
                .inner_margin(20.0)
                .show(ui, |ui| {
                    ui.set_width(inner.x);
                    ui.set_max_width(inner.x);
                    ui.horizontal(|ui| {
                        ui.heading(if modal == Modal::Settings {
                            "设置"
                        } else {
                            &model.info_title
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("×").clicked() {
                                close = true;
                            }
                            if modal == Modal::Settings && ui.button("打开配置").clicked() {
                                model.settings.status = settings::open_config();
                            }
                        });
                    });
                    ui.add_space(10.0);
                    if modal == Modal::Info {
                        egui::ScrollArea::both()
                            .max_height(inner.y - 50.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(&model.info_text).monospace())
                                        .selectable(true),
                                );
                            });
                        return;
                    }
                    let compact = inner.x < 720.0;
                    let height = (inner.y - if compact { 125.0 } else { 88.0 }).max(100.0);
                    if compact {
                        egui::ScrollArea::horizontal()
                            .id_source("settings_tabs")
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    for (label, idx) in SETTING_TABS.iter() {
                                        ui.selectable_value(&mut model.settings.tab, *idx, *label);
                                    }
                                });
                            });
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .id_source(("settings_content", model.settings.tab))
                            .max_height(height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width(inner.x - 12.0);
                                settings_content(ui, model);
                            });
                    } else {
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui_with_layout(
                                Vec2::new(156.0, height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::ScrollArea::vertical().max_height(height).show(
                                        ui,
                                        |ui| {
                                            for (label, idx) in SETTING_TABS.iter() {
                                                let selected = model.settings.tab == *idx;
                                                if ui
                                                    .add_sized(
                                                        [156.0, 42.0],
                                                        egui::Button::new(*label)
                                                            .fill(if selected {
                                                                t::SELECTED
                                                            } else {
                                                                t::WHITE
                                                            })
                                                            .stroke(egui::Stroke::NONE)
                                                            .rounding(10.0),
                                                    )
                                                    .clicked()
                                                {
                                                    model.settings.tab = *idx;
                                                }
                                            }
                                        },
                                    );
                                },
                            );
                            ui.add_space(12.0);
                            let width = inner.x - 180.0;
                            ui.allocate_ui_with_layout(
                                Vec2::new(width, height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_width(width);
                                    egui::ScrollArea::vertical()
                                        .id_source(("settings_content", model.settings.tab))
                                        .max_height(height)
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            ui.set_width(width - 12.0);
                                            settings_content(ui, model);
                                        });
                                },
                            );
                        });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !model.is_busy(),
                                egui::Button::new(RichText::new("保存并应用").color(t::WHITE))
                                    .fill(t::TEXT)
                                    .rounding(9.0),
                            )
                            .clicked()
                            && model.settings.save()
                        {
                            model.apply_saved_settings();
                        }
                        let status = if model.is_busy() {
                            "任务结束或停止后可应用设置"
                        } else {
                            &model.settings.status
                        };
                        ui.add(
                            egui::Label::new(RichText::new(status).size(12.0).color(t::MUTED))
                                .truncate(true),
                        )
                        .on_hover_text(status);
                    });
                });
        });
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        model.modal = None;
    }
}
fn field(ui: &mut egui::Ui, label: &str, text: &mut String, password: bool, hint: &str) {
    ui.label(RichText::new(label).size(12.5).color(t::MUTED));
    egui::Frame::none()
        .fill(t::PANEL)
        .stroke(egui::Stroke::new(1.0_f32, t::BORDER))
        .rounding(7.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            let width = (ui.available_width() - 16.0).max(100.0);
            ui.add_sized(
                [width, 22.0],
                egui::TextEdit::singleline(text)
                    .frame(false)
                    .password(password)
                    .hint_text(hint),
            );
        });
    ui.add_space(5.0);
}

fn upstreams(ui: &mut egui::Ui, model: &mut Model) {
    ui.heading("上游与模型");
    ui.label(
        RichText::new(
            "每个上游独立保存地址、凭据和模型目录。支持 OpenAI Chat Completions 兼容接口。",
        )
        .color(t::MUTED),
    );
    ui.add_space(12.0);
    let prefs = &mut model.settings.prefs;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_source("upstream_edit")
            .selected_text(&prefs.active().name)
            .show_ui(ui, |ui| {
                for (i, p) in prefs.upstreams.iter().enumerate() {
                    ui.selectable_value(&mut prefs.selected, i, &p.name);
                }
            });
        if ui.button("＋ 添加上游").clicked() {
            let p = Upstream {
                name: format!("上游 {}", prefs.upstreams.len() + 1),
                ..Upstream::default()
            };
            prefs.upstreams.push(p);
            prefs.selected = prefs.upstreams.len() - 1;
        }
        if ui
            .add_enabled(prefs.upstreams.len() > 1, egui::Button::new("移除"))
            .clicked()
        {
            prefs.upstreams.remove(prefs.selected);
            prefs.selected = prefs.selected.min(prefs.upstreams.len() - 1);
        }
    });
    ui.add_space(10.0);
    let idx = prefs.selected;
    let p = &mut prefs.upstreams[idx];
    field(ui, "显示名称", &mut p.name, false, "例如：言灵中转");
    let old = p.base_url.clone();
    field(
        ui,
        "API 基础地址",
        &mut p.base_url,
        false,
        "https://example.com/v1",
    );
    if p.endpoint_edited(&old) {
        model.settings.status = "上游主机改变，原凭据已清空，请重新填写".into();
    }
    field(
        ui,
        "API Key",
        &mut p.api_key,
        true,
        "本机配置保存；推荐使用下方环境变量",
    );
    field(
        ui,
        "Key 环境变量（优先使用）",
        &mut p.key_env,
        false,
        "例如 OPENAI_API_KEY，留空使用上方 Key",
    );
    field(
        ui,
        "模型 ID",
        &mut p.model,
        false,
        "支持手动填写，不依赖模型列表",
    );
    if !p.models.is_empty() {
        egui::ComboBox::from_id_source("model_catalog")
            .selected_text("从模型目录选择")
            .show_ui(ui, |ui| {
                for id in &p.models {
                    ui.selectable_value(&mut p.model, id.clone(), id);
                }
            });
    }
    ui.add_space(8.0);
    if ui
        .add_enabled(
            !model.settings.fetching,
            egui::Button::new(if model.settings.fetching {
                "正在获取模型…"
            } else {
                "获取上游模型列表"
            }),
        )
        .clicked()
    {
        model.settings.fetch_models();
    }
    ui.label(
        RichText::new("保存后可在「网关管理」同步当前模型与全部上游到 NullClaw。")
            .size(12.0)
            .color(t::MUTED),
    );
}
fn runtime(ui: &mut egui::Ui, model: &mut Model) {
    ui.heading("运行模式");
    ui.radio_value(
        &mut model.settings.prefs.mode,
        "direct".into(),
        "直连 · 对话 / 普通 Agent",
    );
    ui.label("客户端直接请求所选上游，可使用本地文件工具与命令审批。");
    ui.add_space(12.0);
    ui.radio_value(
        &mut model.settings.prefs.mode,
        "nullclaw".into(),
        "NullClaw · WebChannel",
    );
    ui.label("使用网关的模型、工具与安全配置。每个会话独立连接，保留上下文。");
    ui.add_space(12.0);
    field(
        ui,
        "WebSocket 地址",
        &mut model.settings.connection.ws_url,
        false,
        "ws://127.0.0.1:32123/ws",
    );
    let mut token = model.settings.connection.token.clone().unwrap_or_default();
    field(
        ui,
        "通道令牌",
        &mut token,
        true,
        "token 模式所需的 auth_token",
    );
    model.settings.connection.token = if token.trim().is_empty() {
        None
    } else {
        Some(token)
    };
    ui.checkbox(
        &mut model.settings.connection.pairing,
        "使用本机配对模式（access_token）",
    );
    if ui.button("从 NullClaw 配置重新发现").clicked() {
        if let Some(c) = crate::config::ClientCfg::discover() {
            model.settings.connection = c;
            model.settings.status = "已发现网关配置，请保存应用".into();
        } else {
            model.settings.status = "未找到可用的 NullClaw WebChannel 配置".into();
        }
    }
    ui.add_space(8.0);
    ui.label(
        RichText::new("工具轨迹与审批仅在网关实际发送相应事件时展示。停止接收不会撤销网关端任务。")
            .color(t::MUTED),
    );
}
fn presets(ui: &mut egui::Ui, model: &mut Model) {
    let p = &mut model.settings.prefs;
    ui.heading("Agent 预设");
    ui.checkbox(&mut p.agent, "启用本地 Agent 工具");
    ui.label("对话模式只回答问题；Agent 可列目录、读写文件和执行命令。写入和命令每次均需批准。");
    ui.add_space(8.0);
    field(
        ui,
        "工作区目录",
        &mut p.workspace,
        false,
        "选择你希望处理的项目目录",
    );
    if ui.button("选择工作区").clicked() {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            p.workspace = path.to_string_lossy().into();
        }
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label("工具循环上限");
        ui.add(egui::DragValue::new(&mut p.max_rounds).clamp_range(1..=32));
    });
    ui.label("系统提示词");
    ui.add(
        egui::TextEdit::multiline(&mut p.prompt)
            .desired_rows(6)
            .desired_width(f32::INFINITY),
    );
    if ui.button("恢复精简预设").clicked() {
        p.prompt = settings::AGENT_PROMPT.into();
    }
    ui.label(
        RichText::new(
            "命令以当前用户身份执行，不是系统沙箱；批准前请检查完整命令。更换项目建议新建会话。",
        )
        .size(12.0)
        .color(t::MUTED),
    );
}

fn gateway(ui: &mut egui::Ui, model: &mut Model) {
    ui.heading("NullClaw 网关");
    ui.label(RichText::new(&model.gateway.status).color(t::MUTED));
    ui.label(format!("WebChannel：{}", model.connection_status));
    ui.add_space(8.0);
    field(
        ui,
        "自定义 NullClaw 程序（留空自动发现）",
        &mut model.settings.prefs.nullclaw_bin,
        false,
        "优先使用 01Code 安装的版本或 PATH",
    );
    if ui.button("选择程序").clicked() {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            model.settings.prefs.nullclaw_bin = path.to_string_lossy().into();
        }
    }
    ui.label(format!(
        "当前程序：{}",
        model.settings.nullclaw.as_deref().unwrap_or("未安装")
    ));
    ui.horizontal(|ui| {
        ui.label("HTTP 端口");
        ui.add(
            egui::DragValue::new(&mut model.settings.connection.gateway_port)
                .clamp_range(1..=65535),
        );
    });
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                !model.is_busy() && !model.gateway.running(),
                egui::Button::new("启动网关"),
            )
            .clicked()
        {
            if let Some(bin) = model.settings.nullclaw.clone() {
                model
                    .gateway
                    .start(&bin, &model.runtime, &model.runtime_connection);
            } else {
                model.gateway.status = "请先安装 NullClaw，或选择程序并保存".into();
            }
        }
        if ui
            .add_enabled(
                !model.is_busy() && model.gateway.can_restart(),
                egui::Button::new("重启网关"),
            )
            .clicked()
        {
            if let Some(bin) = model.settings.nullclaw.clone() {
                model
                    .gateway
                    .restart(bin, model.runtime.clone(), model.runtime_connection.clone());
            }
        }
        if ui
            .add_enabled(
                !model.is_busy() && model.gateway.running(),
                egui::Button::new("停止网关"),
            )
            .clicked()
        {
            model.gateway.stop();
        }
        if ui
            .add_enabled(!model.is_busy(), egui::Button::new("重新连接"))
            .clicked()
        {
            model.request_reconnect = true;
        }
        if ui.button("打开网关配置").clicked() {
            model.settings.status = crate::gateway::open_gateway_config();
        }
    });
    ui.add_space(12.0);
    ui.separator();
    ui.label(RichText::new("同步本机配置").strong());
    ui.label("将所有上游、当前选定模型、项目目录与本机 WebChannel 写入 NullClaw，保留其他渠道和权限设置。已有配置保留一份备份。");
    ui.checkbox(
        &mut model.gateway.sync_keys,
        "同步 API Key（会以明文写入网关配置，留空不覆盖已有凭据）",
    );
    if ui.button("生成通道令牌").clicked() {
        model.settings.connection.token = Some(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ));
        model.settings.connection.pairing = false;
    }
    if ui
        .add_enabled(
            !model.is_busy() && !model.gateway.running(),
            egui::Button::new("应用模型与本机网关配置"),
        )
        .clicked()
    {
        match crate::gateway::sync_config(
            &model.settings.prefs,
            &model.settings.connection,
            model.gateway.sync_keys,
        ) {
            Ok(()) => {
                if model.settings.save() {
                    model.apply_saved_settings();
                    model.settings.status = "已同步。请启动或自行重启网关，再重新连接".into();
                } else {
                    model.settings.status = format!(
                        "网关配置已同步，但客户端设置未保存：{}",
                        model.settings.status
                    );
                }
            }
            Err(e) => model.settings.status = e,
        }
    }
    ui.label(RichText::new("停止按钮只管理本客户端启动的进程；外部网关请在对应主机管理。配置修改不会自动放宽现有权限。").size(12.0).color(t::MUTED));
    egui::CollapsingHeader::new("进程诊断日志（最多 64 KiB）").show(ui, |ui| {
        let logs = model.gateway.logs();
        ui.add(
            egui::Label::new(RichText::new(logs).monospace())
                .wrap(true)
                .selectable(true),
        );
    });
}
fn updates(ui: &mut egui::Ui, model: &mut Model) {
    ui.heading("更新与存储");
    ui.label(format!(
        "01Code {} · {} / {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    ui.label("从官方 GitHub Release 获取当前平台版本并核对 SHA-256。NullClaw 安装到 01Code 管理目录，自定义程序路径不会被覆盖。");
    ui.add_space(12.0);
    for client in [false, true] {
        let owned_running = model.gateway.running();
        let busy = model.is_busy();
        let updater = if client {
            &mut model.client_updater
        } else {
            &mut model.nullclaw_updater
        };
        let card_width = ui.available_width();
        egui::Frame::none()
            .stroke(egui::Stroke::new(1.0_f32, t::BORDER))
            .rounding(12.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.set_min_width((card_width - 24.0).max(200.0));
                ui.label(RichText::new(updater.product.title()).strong());
                ui.horizontal_wrapped(|ui| {
                    if updater.can_cancel() && ui.button("取消").clicked() {
                        updater.cancel();
                    }
                    if ui
                        .add_enabled(!updater.busy, egui::Button::new("检查更新"))
                        .clicked()
                    {
                        updater.check();
                    }
                    if ui
                        .add_enabled(
                            !updater.busy
                                && updater.asset.is_some()
                                && !busy
                                && (client || !owned_running),
                            egui::Button::new(if client {
                                "下载更新"
                            } else {
                                "下载并安装 NullClaw"
                            }),
                        )
                        .clicked()
                    {
                        updater.download();
                    }
                    if client
                        && updater.staged.is_some()
                        && ui
                            .add_enabled(!busy, egui::Button::new("安装并重启 01Code"))
                            .clicked()
                    {
                        match updater.apply_client_update() {
                            Ok(()) => model.exit_for_update = true,
                            Err(e) => updater.status = e,
                        }
                    }
                });
                if updater.busy {
                    ui.add(egui::ProgressBar::new(updater.progress).animate(true));
                }
                ui.label(RichText::new(&updater.status).size(12.0).color(t::MUTED));
                if let Some(release) = &updater.release {
                    ui.hyperlink_to(
                        format!("发布说明 · {}", release.tag_name),
                        &release.html_url,
                    );
                    if let Some(body) = &release.body {
                        egui::CollapsingHeader::new("查看更新说明")
                            .id_source(if client {
                                "client_notes"
                            } else {
                                "nullclaw_notes"
                            })
                            .show(ui, |ui| {
                                ui.label(body.chars().take(2000).collect::<String>());
                            });
                    }
                }
                if !client && owned_running {
                    ui.label("安装前请先在网关管理中停止本客户端启动的网关。");
                }
            });
        ui.add_space(10.0);
    }
    if !model.nullclaw_updater.busy
        && crate::updater::managed_nullclaw().is_file()
        && model.runtime.nullclaw_bin.is_empty()
    {
        model.settings.nullclaw = crate::settings::discover_nullclaw();
    }
    ui.separator();
    ui.label(RichText::new("本地存储").strong());
    if model.settings.storage_info.is_empty() {
        model.settings.storage_info = crate::storage::summary();
    }
    ui.label(&model.settings.storage_info);
    if ui.small_button("刷新占用").clicked() {
        model.settings.storage_info = crate::storage::summary();
    }
    ui.label(format!(
        "会话数：{} · 已加载消息：{}",
        model.sessions.len(),
        model.transcripts.values().map(Vec::len).sum::<usize>()
    ));
    ui.horizontal(|ui| {
        ui.label("保留最近会话");
        ui.add(
            egui::DragValue::new(&mut model.settings.prefs.history_sessions).clamp_range(5..=200),
        );
    });
    ui.label("历史会话保留数量在保存设置后生效；当前会话始终保留。更新仅保留一份旧程序，附件按内容去重存储。");
    if model.history_blocked {
        ui.label("历史索引异常，自动保存和附件清理已暂停。");
        if ui.button("备份原记录并重建索引").clicked() {
            model.settings.status = match model.repair_history() {
                Ok(()) => "已保留 history-recovery.json 并重建索引".into(),
                Err(e) => format!("恢复失败：{e}"),
            };
        }
    }
    if ui
        .add_enabled(
            !model.is_busy() && !model.client_updater.busy && !model.nullclaw_updater.busy,
            egui::Button::new("清理未引用附件与下载缓存"),
        )
        .clicked()
    {
        model.settings.status = model.cleanup_storage();
        model.settings.storage_info = crate::storage::summary();
    }
}
fn tasks(ui: &mut egui::Ui, model: &mut Model) {
    ui.heading("任务与扩展");
    ui.horizontal_wrapped(|ui| {
        for (label, args) in [
            ("运行诊断", vec!["doctor"]),
            ("扩展能力", vec!["capabilities"]),
            ("已安装技能", vec!["skills", "list"]),
            (
                "网关会话",
                vec!["history", "list", "--limit", "20", "--json"],
            ),
            ("定时任务", vec!["cron", "list", "--json"]),
        ] {
            if ui
                .add_enabled(!model.info_loading(), egui::Button::new(label))
                .clicked()
            {
                model.load_info(args.into_iter().map(String::from).collect());
            }
        }
    });
    ui.add_space(10.0);
    egui::CollapsingHeader::new("创建 Agent 定时任务").show(ui, |ui| {
        field(
            ui,
            "Cron 表达式",
            &mut model.gateway.cron_expression,
            false,
            "0 9 * * *",
        );
        ui.label("任务提示词");
        ui.add(
            egui::TextEdit::multiline(&mut model.gateway.cron_prompt)
                .desired_rows(3)
                .desired_width(ui.available_width()),
        );
        if ui
            .add_enabled(
                !model.is_busy()
                    && !model.info_loading()
                    && !model.gateway.cron_prompt.trim().is_empty(),
                egui::Button::new("创建任务"),
            )
            .clicked()
        {
            model.load_info(vec![
                "cron".into(),
                "add-agent".into(),
                model.gateway.cron_expression.clone(),
                model.gateway.cron_prompt.clone(),
                "--session-target".into(),
                "isolated".into(),
            ]);
        }
    });
    egui::CollapsingHeader::new("管理已有任务").show(ui, |ui| {
        let old_id = model.gateway.cron_id.clone();
        field(
            ui,
            "任务 ID",
            &mut model.gateway.cron_id,
            false,
            "从任务列表复制 ID",
        );
        if old_id != model.gateway.cron_id {
            model.gateway.confirm_remove = false;
        }
        ui.horizontal_wrapped(|ui| {
            for (label, command) in [
                ("暂停", "pause"),
                ("恢复", "resume"),
                ("立即运行", "run"),
                ("执行记录", "runs"),
            ] {
                if ui
                    .add_enabled(
                        !model.is_busy()
                            && !model.info_loading()
                            && !model.gateway.cron_id.trim().is_empty(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    model.load_info(vec![
                        "cron".into(),
                        command.into(),
                        model.gateway.cron_id.trim().into(),
                    ]);
                }
            }
        });
        ui.checkbox(&mut model.gateway.confirm_remove, "确认删除此任务");
        if ui
            .add_enabled(
                !model.is_busy()
                    && !model.info_loading()
                    && model.gateway.confirm_remove
                    && !model.gateway.cron_id.trim().is_empty(),
                egui::Button::new("删除任务"),
            )
            .clicked()
        {
            model.load_info(vec![
                "cron".into(),
                "remove".into(),
                model.gateway.cron_id.trim().into(),
            ]);
            model.gateway.confirm_remove = false;
        }
    });
    ui.add_space(8.0);
    ui.label(RichText::new("执行结果").strong());
    egui::ScrollArea::vertical()
        .id_source("gateway_action_results")
        .max_height(240.0)
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(&model.info_text).monospace())
                    .wrap(true)
                    .selectable(true),
            );
        });
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn every_settings_tab_fits_small_and_desktop_viewports() {
        for (width, height) in [(640.0, 420.0), (960.0, 640.0), (1440.0, 900.0)] {
            let ctx = egui::Context::default();
            crate::setup_style(&ctx);
            crate::setup_fonts(&ctx);
            let mut model = Model::new(true);
            model.modal = Some(Modal::Settings);
            for tab in 0..SETTING_TABS.len() {
                model.settings.tab = tab;
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, height));
                for _ in 0..2 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |ctx| show(ctx, &mut model),
                    );
                }
                let rect = ctx
                    .memory(|m| m.area_rect(egui::Id::new("01_settings")))
                    .unwrap();
                assert!(
                    screen.expand(1.0).contains_rect(rect),
                    "tab {tab}, viewport {screen:?}, dialog {rect:?}"
                );
            }
        }
    }
}
