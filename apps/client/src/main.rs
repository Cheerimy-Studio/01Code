#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod events;
mod gateway;
mod icons;
mod logo;
mod model;
mod process_tree;
mod settings;
mod storage;
mod theme;
mod transport;
mod ui;
mod updater;
mod util;
mod widgets;

use eframe::egui;

use model::Model;
use transport::Transport;

struct App {
    model: Model,
    transport: Box<dyn Transport>,

    /// 测试钩子:环境变量 CODING01_SEND 启动后自动提交一条消息
    autosend: Option<String>,
    autosend_at: Option<std::time::Instant>,
    /// 测试钩子:启动后自动执行一次设置保存
    test_save: bool,
    test_save_at: Option<std::time::Instant>,
    /// 测试钩子:启动后附加一个文件/图片
    attach_path: Option<std::path::PathBuf>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        setup_fonts(&cc.egui_ctx);
        let use_sim = std::env::var("CODING01_SIM").is_ok();
        let mut model = Model::new(use_sim);
        let transport: Box<dyn Transport> = if use_sim {
            Box::new(transport::sim::Sim::new())
        } else {
            make_transport(&model)
        };
        match std::env::var("CODING01_MODAL").as_deref() {
            Ok("settings") => model.modal = Some(model::Modal::Settings),
            Ok("info") => {
                model.info_title = "扩展 · 运行时能力".into();
                model.load_info(vec!["capabilities".into()]);
                model.modal = Some(model::Modal::Info);
            }
            _ => {}
        }
        Self {
            model,
            transport,

            autosend: std::env::var("CODING01_SEND").ok(),
            autosend_at: None,
            test_save: std::env::var("CODING01_TEST_SAVE").is_ok(),
            test_save_at: None,
            attach_path: std::env::var("CODING01_ATTACH")
                .ok()
                .map(std::path::PathBuf::from),
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.transport.shutdown();
        self.model.save_history();
        self.model.gateway.shutdown();
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.model.gateway.poll() && self.model.gateway_mode && !self.model.is_busy() {
            self.model.request_reconnect = true;
        }
        self.model.client_updater.poll();
        self.model.nullclaw_updater.poll();
        if self.model.exit_for_update {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.model.settings.poll();
        self.model.poll_info();
        for ev in self.transport.poll() {
            self.model.apply(ev);
        }
        // 1.1 看门狗:超时无响应时复位忙碌状态
        self.model.watchdog_tick();
        // 1.2 测试钩子:附加附件
        if let Some(p) = self.attach_path.take() {
            self.model.add_attachment(ctx, p);
        }
        self.model.restore_image_previews(ctx);
        for event in ctx.input(|i| i.events.clone()) {
            match event {
                egui::Event::CompositionStart | egui::Event::CompositionUpdate(_) => {
                    self.model.composing = true
                }
                egui::Event::CompositionEnd(_) => self.model.composing = false,
                _ => {}
            }
        }
        // 2. 布局:侧栏 + 主列(头部/会话流/输入区/状态栏)
        ui::sidebar::show(ctx, &mut self.model);
        ui::chat::show(ctx, &mut self.model);
        // Apply runtime changes before dispatching new requests.
        if std::mem::take(&mut self.model.request_reconnect) {
            if self.model.gateway_mode {
                self.model.gateway_connected = false;
                self.model.connection_status = "连接中".into();
            }
            self.transport = make_transport(&self.model);
        }
        // 3. 出站消息
        for msg in self.model.drain_outbox() {
            self.transport.send(msg);
        }
        // 3.2 测试钩子:自动提交一条消息
        if let Some(msg) = self.autosend.clone() {
            let started = *self.autosend_at.get_or_insert_with(std::time::Instant::now);
            if started.elapsed().as_secs() >= 2 {
                self.model.input = msg;
                self.model.user_submit();
                self.autosend = None;
            }
        }
        // 3.3 测试钩子:自动执行设置保存
        if self.test_save {
            let started = *self
                .test_save_at
                .get_or_insert_with(std::time::Instant::now);
            if started.elapsed().as_secs() >= 2 {
                let ok = self.model.settings.save();
                eprintln!("[save] ok={} status={}", ok, self.model.settings.status);
                self.test_save = false;
            }
        }
        // 4. 常态心跳:事件到达可立即呈现(空闲 5fps,占用可忽略);忙碌时提频
        if self.model.waiting_approval() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        } else if self.model.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        } else if self.model.settings.fetching
            || self.model.client_updater.busy
            || self.model.nullclaw_updater.busy
            || self.model.info_loading()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }
}

fn make_transport(model: &Model) -> Box<dyn Transport> {
    if model.runtime.mode == "nullclaw" {
        Box::new(transport::ws::Ws::connect(
            &model.runtime_connection,
            &model.active_sid,
        ))
    } else {
        Box::new(transport::direct::Direct::new(model.runtime.clone()))
    }
}

fn setup_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.override_text_color = Some(theme::TEXT2);
    style.visuals.panel_fill = theme::PANEL;
    style.visuals.window_fill = theme::WHITE;
    style.visuals.extreme_bg_color = theme::WHITE;
    style.visuals.selection.bg_fill = theme::SELECTION_BG;
    style.visuals.selection.stroke = egui::Stroke::NONE;
    // 只允许显式复制的文本(如代码块)被选择,界面标签禁止拖选
    style.interaction.selectable_labels = false;
    style.visuals.widgets.inactive.bg_fill = theme::WHITE;
    style.visuals.widgets.inactive.weak_bg_fill = theme::WHITE;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, theme::BORDER);
    style.visuals.widgets.hovered.weak_bg_fill = theme::HOVER;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, theme::BORDER);
    style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, theme::TEXT2);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // 无衬线中文(UI)
    if let Some(bytes) = load_font(&["msyh.ttc", "msyhl.ttc", "simhei.ttf", "Deng.ttf"]) {
        fonts
            .font_data
            .insert("cjk".to_string(), egui::FontData::from_owned(bytes));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("cjk".to_string());
        }
    }
    // Reuse one CJK font payload for all UI families to avoid loading the same TTC twice.
    fonts.families.insert(
        egui::FontFamily::Name("o1bold".into()),
        fonts.families[&egui::FontFamily::Proportional].clone(),
    );
    ctx.set_fonts(fonts);
}

fn load_font(names: &[&str]) -> Option<Vec<u8>> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    for name in names {
        let path = format!("{}\\Fonts\\{}", windir, name);
        if let Ok(bytes) = std::fs::read(&path) {
            return Some(bytes);
        }
    }
    for path in [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        "/System/Library/Fonts/PingFang.ttc",
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if updater::handle_update_cli() {
        return Ok(());
    }
    std::fs::create_dir_all(config::app_dir())?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let instance = options.open(config::app_dir().join("client.lock"))?;
    if instance.try_lock().is_err() {
        rfd::MessageDialog::new()
            .set_title("01Code")
            .set_description("01Code 已在运行，请切换到现有窗口。")
            .show();
        return Ok(());
    }
    let [w, h] = [1280.0, 800.0];
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("01 编码")
            .with_inner_size([w, h])
            .with_min_inner_size([640.0, 420.0])
            .with_icon(logo::window_icon()),
        ..Default::default()
    };
    eframe::run_native("01 编码", options, Box::new(|cc| Box::new(App::new(cc))))?;
    Ok(())
}
