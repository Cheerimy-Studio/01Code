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
    /// 请求的窗口逻辑尺寸(首帧用于对齐 winit 认知与实际窗口)
    requested_size: [f32; 2],
    resized: bool,

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
        let [w, h] = fit_size(1280.0, 800.0);
        Self {
            model,
            transport,
            requested_size: [w, h],
            resized: false,

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
        // 首帧硬同步窗口物理尺寸 = 画布逻辑 × ppp(修复创建期 DPI 感知切换错位)
        if !self.resized {
            self.resized = true;
            let ppp = ctx.pixels_per_point();
            force_window_size(self.requested_size[0] * ppp, self.requested_size[1] * ppp);
        }
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
    // 强制经由 NullClaw WebChannel;不再支持内置直连 Agent
    Box::new(transport::ws::Ws::connect(
        &model.runtime_connection,
        &model.active_sid,
    ))
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

/// 必须在创建窗口前开启 DPI 感知:否则 winit 在创建之后切换感知,
/// 会导致「egui 画布逻辑尺寸 ≠ 实际窗口物理尺寸」(底部面板画出窗外)
#[cfg(windows)]
fn enable_dpi_awareness() {
    #[link(name = "user32")]
    extern "system" {
        fn SetProcessDPIAware() -> i32;
    }
    unsafe {
        SetProcessDPIAware();
    }
}

#[cfg(not(windows))]
fn enable_dpi_awareness() {}

/// 依据物理工作区计算不越界的窗口物理尺寸
#[cfg(windows)]
fn fit_size(w: f32, h: f32) -> [f32; 2] {
    #[link(name = "user32")]
    extern "system" {
        fn GetSystemMetrics(n: i32) -> i32;
        fn GetDpiForSystem() -> u32;
    }
    // SM_CXFULLSCREEN / SM_CYFULLSCREEN:工作区(去掉任务栏)物理像素
    let px_w = unsafe { GetSystemMetrics(16) } as f32;
    let px_h = unsafe { GetSystemMetrics(17) } as f32;
    let dpi = unsafe { GetDpiForSystem() } as f32;
    if dpi < 1.0 {
        return [w, h];
    }
    let scale = (dpi / 96.0).max(1.0);
    [w.min(px_w / scale * 0.96), h.min(px_h / scale * 0.94)]
}

#[cfg(not(windows))]
fn fit_size(w: f32, h: f32) -> [f32; 2] {
    [w, h]
}

/// 绕过 winit,直接把 OS 窗口客户区调整为 winit 认知的物理尺寸
#[cfg(windows)]
fn force_window_size(client_w: f32, client_h: f32) {
    #[link(name = "user32")]
    extern "system" {
        fn FindWindowW(cls: *const u16, title: *const u16) -> isize;
        fn GetClientRect(h: isize, r: *mut RectI) -> i32;
        fn GetWindowRect(h: isize, r: *mut RectI) -> i32;
        fn SetWindowPos(
            h: isize,
            after: isize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
    }
    #[repr(C)]
    struct RectI {
        l: i32,
        t: i32,
        r: i32,
        b: i32,
    }
    unsafe {
        let mut title: Vec<u16> = "01 编码".encode_utf16().collect();
        title.push(0);
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd == 0 {
            return;
        }
        let mut cli = RectI {
            l: 0,
            t: 0,
            r: 0,
            b: 0,
        };
        let mut outer = RectI {
            l: 0,
            t: 0,
            r: 0,
            b: 0,
        };
        if GetClientRect(hwnd, &mut cli) == 0 || GetWindowRect(hwnd, &mut outer) == 0 {
            return;
        }
        // 外框与客户区的边框差(标题栏 + 边缘)
        let dx = (outer.r - outer.l) - (cli.r - cli.l);
        let dy = (outer.b - outer.t) - (cli.b - cli.t);
        const SWP_NOZORDER: u32 = 0x0004;
        SetWindowPos(
            hwnd,
            0,
            120,
            60,
            (client_w + dx as f32) as i32,
            (client_h + dy as f32) as i32,
            SWP_NOZORDER,
        );
    }
}

#[cfg(not(windows))]
fn force_window_size(_w: f32, _h: f32) {}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if updater::handle_update_cli() {
        return Ok(());
    }
    enable_dpi_awareness();
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
    let [w, h] = fit_size(1280.0, 800.0);
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
