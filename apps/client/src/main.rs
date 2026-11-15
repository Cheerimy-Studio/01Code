#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod events;
mod gateway;
mod history;
mod icons;
mod local_proxy;
mod logo;
mod model;
mod runtime;
mod settings;
mod theme;
mod transport;
mod ui;
mod util;
mod widgets;

use eframe::egui;
use events::Event;
use model::Model;
use transport::Transport;

struct App {
    model: Model,
    transport: Box<dyn Transport>,
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
    /// 测试钩子:启动完成后自动发起网页登录
    auto_login: bool,
    auto_login_at: Option<std::time::Instant>,
    /// 测试钩子:启动后切到空会话(等模拟器初始事件结束)
    new_task_at: Option<std::time::Instant>,
    screenshot_at: std::time::Instant,
    screenshot_requested: bool,
    history_at: std::time::Instant,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        setup_fonts(&cc.egui_ctx);
        let use_sim = std::env::var("CODING01_SIM").is_ok();
        let cfg = config::ClientCfg::load();
        let transport: Box<dyn Transport> = if use_sim {
            Box::new(transport::sim::Sim::new())
        } else {
            Box::new(transport::ws::Ws::connect(&cfg))
        };
        let [w, h] = if std::env::var("CODING01_NARROW").is_ok() {
            [760.0, 560.0]
        } else {
            fit_size(1150.0, 660.0)
        };
        let mut model = Model::new(use_sim);
        match std::env::var("CODING01_MODAL").as_deref() {
            Ok("settings") => {
                model.settings.tab = std::env::var("CODING01_SETTINGS_TAB")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                model.modal = Some(model::Modal::Settings);
            }
            Ok("info") => {
                model.info_title = "扩展 · 运行时能力".into();
                model.info_text = match model.settings.nullclaw.clone() {
                    Some(_) => "请在运行环境中查看能力".into(),
                    None => "未找到 nullclaw 可执行文件".into(),
                };
                model.modal = Some(model::Modal::Info);
            }
            _ => {}
        }
        if !use_sim {
            if !model.settings.key_configured {
                model.modal = Some(model::Modal::Settings);
                model.settings.clear_status();
            } else {
                // 账户信息与运行时启动并行，用户名/余额不必等 NullClaw 就绪
                model.settings.sync_account();
            }
            model.settings.bootstrap();
        }
        Self {
            model,
            transport,
            requested_size: [w, h],
            resized: false,
            autosend: std::env::var("CODING01_SEND").ok(),
            autosend_at: None,
            test_save: std::env::var("CODING01_TEST_SAVE").is_ok(),
            test_save_at: None,
            screenshot_at: std::time::Instant::now(),
            screenshot_requested: false,
            history_at: std::time::Instant::now(),
            attach_path: std::env::var("CODING01_ATTACH")
                .ok()
                .map(std::path::PathBuf::from),
            auto_login: std::env::var("CODING01_LOGIN").is_ok(),
            auto_login_at: None,
            new_task_at: std::env::var("CODING01_NEW")
                .is_ok()
                .then(std::time::Instant::now),
        }
    }
}

/// 绕过 winit,直接把 OS 窗口客户区调整为 winit 认知的物理尺寸
/// (创建期 DPI 感知切换使 winit 的 resize 通道失灵,只能用 SetWindowPos 硬同步)
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

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(path) = std::env::var("CODING01_SCREENSHOT") {
            if !self.screenshot_requested && self.screenshot_at.elapsed().as_secs() >= 3 {
                self.screenshot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            }
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    let _ = image::save_buffer(
                        &path,
                        &bytes,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        for (title, text) in self.model.settings.tick() {
            // 从设置页打开诊断/能力等窗口时记住来源，关闭后回到原页面
            if self.model.modal != Some(model::Modal::Info) {
                self.model.info_return = self.model.modal.clone();
            }
            self.model.info_title = title;
            self.model.info_text = text;
            self.model.modal = Some(model::Modal::Info);
        }
        // 测试钩子:准备完成后自动发起一次网页登录(便于真机验证设备码流程)
        if self.auto_login && !self.model.settings.busy {
            let started = *self
                .auto_login_at
                .get_or_insert_with(std::time::Instant::now);
            if started.elapsed().as_secs() >= 2 {
                self.auto_login = false;
                self.model.settings.tab = 0;
                self.model.modal = Some(model::Modal::Settings);
                self.model.settings.login();
            }
        }
        // 测试钩子:模拟器初始事件结束后切到空会话,用于检查首页布局
        if let Some(t) = self.new_task_at {
            if t.elapsed().as_secs() >= 2 {
                self.model.new_task();
                self.new_task_at = None;
            }
        }
        if std::mem::take(&mut self.model.settings.reconnect) {
            self.model.request_reconnect = true;
            self.model.model_name = self.model.settings.model.clone();
            // 切换模型时先按网关同步的元数据重置上下文上限；运行时随后会再校准
            self.model.ctx_limit = self.model.settings.context_limit(&self.model.model_name);
        }
        // 首帧硬同步窗口物理尺寸 = 画布逻辑 × ppp(修复创建期 DPI 感知切换错位)
        if !self.resized {
            self.resized = true;
            let ppp = ctx.pixels_per_point();
            force_window_size(self.requested_size[0] * ppp, self.requested_size[1] * ppp);
            logo::apply_window_icons();
        }
        // 1. 拉取事件(网关失败时一次性回退本地演示)
        let mut events = self.transport.poll();
        for ev in events.drain(..) {
            match &ev {
                Event::Paired { token } => {
                    config::ClientCfg::save_token(token);
                }
                _ => {}
            }
            self.model.apply(ev);
        }
        // 1.1 看门狗:超时无响应时复位忙碌状态
        self.model.watchdog_tick();
        // 1.2 测试钩子:附加附件
        if let Some(p) = self.attach_path.take() {
            self.model.add_attachment(ctx, p);
        }
        // 2. 布局:侧栏 + 主列(头部/会话流/输入区/状态栏)
        ui::sidebar::show(ctx, &mut self.model);
        ui::chat::show(ctx, &mut self.model);
        // 2.1 打字机推进(单次返回的流式呈现,低刷新率)
        if self.model.is_typing() {
            self.model.tick();
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // 3. 出站消息
        for msg in self.model.drain_outbox() {
            self.transport.send(msg);
        }
        // 3.1 设置保存后重连网关
        if std::mem::take(&mut self.model.request_reconnect) {
            let cfg = config::ClientCfg::load();
            self.transport = Box::new(transport::ws::Ws::connect(&cfg));
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
        if self.history_at.elapsed().as_secs() >= 5 {
            history::save(&mut self.model);
            self.history_at = std::time::Instant::now();
        }
        // 4. 常态心跳:事件到达可立即呈现(空闲 5fps,占用可忽略);忙碌时提频
        if self.model.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // 正常关窗时一并停止本客户端启动的 NullClaw 与本地转发
        let _ = runtime::stop();
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
    // 衬线中文已不启用(正文使用无衬线),避免额外字体图集占用
    // 粗体(标题/强调)
    if let Some(bytes) = load_font(&["msyhbd.ttc", "msyh.ttc", "simhei.ttf"]) {
        fonts
            .font_data
            .insert("boldcjk".to_string(), egui::FontData::from_owned(bytes));
        let fam = egui::FontFamily::Name("o1bold".into());
        let list = fonts.families.entry(fam).or_default();
        list.push("boldcjk".to_string());
        list.push("cjk".to_string());
    }
    let fallback = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(egui::FontFamily::Name("o1bold".into()))
        .or_insert(fallback);
    ctx.set_fonts(fonts);
}

fn load_font(names: &[&str]) -> Option<Vec<u8>> {
    for path in [
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/PingFang.ttc",
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }
    }

    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    for name in names {
        let path = format!("{}\\Fonts\\{}", windir, name);
        if let Ok(bytes) = std::fs::read(&path) {
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
    [w.min(px_w * 0.92), h.min(px_h * 0.90)]
}

#[cfg(not(windows))]
fn fit_size(w: f32, h: f32) -> [f32; 2] {
    [w, h]
}

/// 单实例：重复启动时聚焦已有窗口，避免多个窗口/遮罩叠加
#[cfg(windows)]
fn acquire_single_instance() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Local\\01Code.Client.SingleInstance\0"
        .encode_utf16()
        .collect();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle == 0 {
        // 无法创建互斥量时放行，避免影响正常启动
        return true;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // 已有实例：能定位到窗口就退出，否则允许本次启动（例如旧实例已无窗口）
        !focus_existing_window()
    } else {
        true
    }
}

#[cfg(windows)]
fn focus_existing_window() -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> isize;
        fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
        fn SetForegroundWindow(hwnd: isize) -> i32;
    }
    let title: Vec<u16> = "01 编码\0".encode_utf16().collect();
    let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if hwnd != 0 {
        unsafe {
            ShowWindow(hwnd, 9); // SW_RESTORE
            SetForegroundWindow(hwnd);
        }
        true
    } else {
        false
    }
}

#[cfg(not(windows))]
fn acquire_single_instance() -> bool {
    true
}

fn main() -> eframe::Result<()> {
    enable_dpi_awareness();
    if !acquire_single_instance() {
        return Ok(());
    }
    let [w, h] = if std::env::var("CODING01_NARROW").is_ok() {
        [760.0, 560.0]
    } else {
        fit_size(1150.0, 660.0)
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("01 编码")
            .with_inner_size([w, h])
            .with_min_inner_size([640.0, 420.0])
            .with_icon(logo::window_icon()),
        // 不恢复上次窗口位置，避免显示器/缩放变化后窗口落在屏幕外
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native("01 编码", options, Box::new(|cc| Box::new(App::new(cc))))
}

impl Drop for App {
    fn drop(&mut self) {
        history::flush(&self.model);
        self.model.settings.cancel_login();
        let _ = runtime::stop();
    }
}
