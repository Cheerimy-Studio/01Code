#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod events;
mod icons;
mod logo;
mod model;
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
    ws_failed: bool,
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
        let cfg = config::ClientCfg::load();
        let transport: Box<dyn Transport> = if use_sim {
            Box::new(transport::sim::Sim::new())
        } else {
            Box::new(transport::ws::Ws::connect(&cfg))
        };
        let [w, h] = fit_size(1150.0, 660.0);
        let mut model = Model::new(use_sim);
        match std::env::var("CODING01_MODAL").as_deref() {
            Ok("settings") => model.modal = Some(model::Modal::Settings),
            Ok("info") => {
                model.info_title = "扩展 · 运行时能力".into();
                model.info_text = match model.settings.nullclaw.clone() {
                    Some(bin) => settings::run_cli(&bin, &["capabilities"]),
                    None => "未找到 nullclaw 可执行文件".into(),
                };
                model.modal = Some(model::Modal::Info);
            }
            _ => {}
        }
        Self {
            model,
            transport,
            ws_failed: false,
            requested_size: [w, h],
            resized: false,
            autosend: std::env::var("CODING01_SEND").ok(),
            autosend_at: None,
            test_save: std::env::var("CODING01_TEST_SAVE").is_ok(),
            test_save_at: None,
            attach_path: std::env::var("CODING01_ATTACH").ok().map(std::path::PathBuf::from),
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
        // 首帧硬同步窗口物理尺寸 = 画布逻辑 × ppp(修复创建期 DPI 感知切换错位)
        if !self.resized {
            self.resized = true;
            let ppp = ctx.pixels_per_point();
            force_window_size(
                self.requested_size[0] * ppp,
                self.requested_size[1] * ppp,
            );
            logo::apply_window_icons();
        }
        // 1. 拉取事件(网关失败时一次性回退本地演示)
        let mut events = self.transport.poll();
        for ev in events.drain(..) {            match &ev {
                Event::Sys { text, .. } if text.starts_with("网关连接失败") && !self.ws_failed => {
                    self.ws_failed = true;
                    self.model = Model::new(true);
                    self.transport = Box::new(transport::sim::Sim::new());
                    break;
                }
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
            let started = *self.test_save_at.get_or_insert_with(std::time::Instant::now);
            if started.elapsed().as_secs() >= 2 {
                let ok = self.model.settings.save();
                eprintln!(
                    "[save] ok={} status={}",
                    ok, self.model.settings.status
                );
                self.test_save = false;
            }
        }
        // 4. 常态心跳:事件到达可立即呈现(空闲 5fps,占用可忽略);忙碌时提频
        if self.model.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
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
    style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, theme::TEXT2);
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

fn main() -> eframe::Result<()> {
    enable_dpi_awareness();
    let [w, h] = fit_size(1150.0, 660.0);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("01 编码")
            .with_inner_size([w, h])
            .with_min_inner_size([640.0, 420.0])
            .with_icon(logo::window_icon()),
        ..Default::default()
    };
    eframe::run_native("01 编码", options, Box::new(|cc| Box::new(App::new(cc))))
}
