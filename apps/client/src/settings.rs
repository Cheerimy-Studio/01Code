use crate::{config, gateway, runtime};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc,
};
pub const DEFAULT_BASE_URL: &str = "https://ai.gzmath.cn/v1";
pub const DEFAULT_MODEL: &str = "deepseek-v4-pro";
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelMeta {
    pub ctx: u64,
    pub out: u64,
    pub tps: u64,
    pub free: bool,
    pub paid: bool,
}
pub enum ResultMsg {
    LoggedOut,
    Status(Result<String, String>),
    /// 非致命提示：登录已成功但运行时启动失败等
    Notice(String),
    Code {
        code: String,
        url: String,
        expires_in: u64,
    },
    Bound,
    LoginFailed(String),
    Saved(Option<String>),
    Stopped(Result<String, String>),
    SyncError(String),
    Account(Value, Vec<Value>),
    Models(Vec<String>, HashMap<String, ModelMeta>),
    ModelsError(String),
    Output(String, Result<String, String>),
}
pub struct SettingsState {
    pub nullclaw: Option<String>,
    pub base_url: String,
    pub model: String,
    pub key_configured: bool,
    pub status: String,
    pub tab: usize,
    pub models: Vec<String>,
    pub model_meta: HashMap<String, ModelMeta>,
    pub devices: Vec<Value>,
    pub account: Value,
    pub device_code: String,
    pub verification_uri: String,
    pub device_expires: Option<std::time::Instant>,
    pub busy: bool,
    pub syncing: bool,
    pub syncing_models: bool,
    /// 设备登录流程进行中（防止重复点击生成多个验证码/浏览器页）
    pub logging_in: bool,
    pub reconnect: bool,
    /// 访问模式：0=普通（有变更询问）1=自主（有风险询问）2=完全访问
    pub access_mode: usize,
    pub runtime_status: String,
    /// 本客户端托管的 NullClaw 是否正在运行（暂停时禁止发送）
    pub runtime_running: bool,
    /// 思考强度 0=最低 … 4=最高，对应 NullClaw reasoning_effort
    pub thinking_level: usize,
    pub cron_schedule: String,
    pub cron_prompt: String,
    pub cron_id: String,
    tx: Sender<ResultMsg>,
    rx: Receiver<ResultMsg>,
    cancel: Arc<AtomicBool>,
    last_sync: std::time::Instant,
    last_models_sync: std::time::Instant,
}
/// 解析 /v1/models：只依赖 id；上下文/输出/速度缺失时按 0 处理，
/// 兼容旧网关或第三方 OpenAI 兼容服务。
fn parse_models(data: &[Value]) -> (Vec<String>, HashMap<String, ModelMeta>) {
    let mut models = Vec::with_capacity(data.len());
    let mut meta = HashMap::new();
    for m in data {
        let Some(id) = m["id"].as_str() else { continue };
        models.push(id.to_string());
        meta.insert(
            id.to_string(),
            ModelMeta {
                ctx: m["context_length"]
                    .as_u64()
                    .or_else(|| m["context_window"].as_u64())
                    .unwrap_or(0),
                out: m["max_output_tokens"].as_u64().unwrap_or(0),
                tps: m["tps"].as_u64().unwrap_or(0),
                free: m["free"].as_bool().unwrap_or(false),
                paid: m["paid"].as_bool().unwrap_or(false),
            },
        );
    }
    (models, meta)
}
impl SettingsState {
    pub fn load() -> Self {
        let v = config::read();
        let (tx, rx) = mpsc::channel();
        // 客户端只服务言灵中转：网关地址固定，不再允许用户改到其他地址
        let base = DEFAULT_BASE_URL.to_string();
        // 仅网页登录:配置标记已绑定但系统凭据缺失时,回到未登录状态
        let mut key_configured = v["bound"].as_bool().unwrap_or(false);
        if key_configured {
            key_configured = gateway::base(&base).is_ok_and(|b| gateway::key(&b).is_ok());
        }
        Self {
            nullclaw: runtime::installed(),
            base_url: base,
            model: v["model"].as_str().unwrap_or(DEFAULT_MODEL).into(),
            key_configured,
            status: String::new(),
            tab: 0,
            models: Vec::new(),
            model_meta: HashMap::new(),
            devices: Vec::new(),
            account: Value::Null,
            device_code: String::new(),
            verification_uri: String::new(),
            device_expires: None,
            busy: false,
            syncing: false,
            syncing_models: false,
            logging_in: false,
            reconnect: false,
            access_mode: v["access_mode"]
                .as_u64()
                .map(|m| m.min(2) as usize)
                .unwrap_or_else(|| {
                    if v["workspace_only"].as_bool().unwrap_or(true) {
                        0
                    } else {
                        1
                    }
                }),
            runtime_status: "准备中".into(),
            runtime_running: runtime::is_running(),
            thinking_level: v["thinking_level"].as_u64().unwrap_or(4).min(4) as usize,
            cron_schedule: String::new(),
            cron_prompt: String::new(),
            cron_id: String::new(),
            tx,
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            last_sync: std::time::Instant::now(),
            last_models_sync: std::time::Instant::now(),
        }
    }
    /// 清空设置页底部的临时状态文字（新会话、切换页面时调用）。
    pub fn clear_status(&mut self) {
        self.status.clear();
    }
    pub fn thinking_effort(&self) -> &'static str {
        config::thinking_effort(self.thinking_level as u64)
    }
    pub fn thinking_label(&self) -> &'static str {
        config::thinking_label(self.thinking_level as u64)
    }
    /// 保存思考强度并重启运行时使其生效（拖动时只预览，松手再调用）。
    pub fn set_thinking_level(&mut self, level: usize) {
        self.thinking_level = level.min(4);
        let mut c = config::read();
        c["thinking_level"] = json!(self.thinking_level);
        let _ = config::write(&c);
        if !self.busy {
            self.save();
        }
    }
    pub fn access_label(&self) -> &'static str {
        match self.access_mode {
            0 => "普通模式",
            1 => "自主模式",
            _ => "完全访问",
        }
    }
    pub fn set_access_mode(&mut self, mode: usize) {
        self.access_mode = mode.min(2);
        let mut c = config::read();
        c["access_mode"] = json!(self.access_mode);
        let _ = config::write(&c);
        if !self.busy {
            self.save();
        }
    }
    /// 当前模型在网关侧标注的上下文上限（0=未标注）。
    pub fn context_limit(&self, model: &str) -> f32 {
        self.model_meta
            .get(model)
            .map(|m| m.ctx as f32)
            .filter(|v| *v > 0.0)
            .unwrap_or(0.0)
    }
    /// 设备码剩余有效秒数（未开始登录时为 None）。
    pub fn device_remaining(&self) -> Option<u64> {
        self.device_expires.map(|t| {
            t.saturating_duration_since(std::time::Instant::now())
                .as_secs()
        })
    }
    /// 账户显示名：优先用户名，其次邮箱；未登录返回空。
    pub fn account_name(&self) -> String {
        for k in ["username", "email", "name"] {
            if let Some(v) = self.account[k].as_str() {
                if !v.trim().is_empty() {
                    return v.trim().to_string();
                }
            }
        }
        String::new()
    }
    pub fn bootstrap(&mut self) {
        self.busy = true;
        self.status = "正在准备 NullClaw".into();
        let tx = self.tx.clone();
        let base = self.base_url.clone();
        let model = self.model.clone();
        let key_configured = self.key_configured;
        let thinking = self.thinking_effort();
        let access_mode = self.access_mode;
        std::thread::spawn(move || {
            let r = (|| {
                let status = runtime::install(false)?;
                if key_configured {
                    let b = gateway::base(&base)?;
                    let k = gateway::key(&b)?;
                    runtime::configure(&format!("{b}/v1"), &model, thinking, access_mode)?;
                    runtime::start(&k)?;
                }
                Ok(status)
            })();
            let _ = tx.send(ResultMsg::Status(r));
        });
    }
    /// 凭据被网关判定失效：清除本地状态并停止运行时，下次启动回到未登录
    fn clear_auth(&mut self) {
        self.key_configured = false;
        self.account = Value::Null;
        self.models.clear();
        self.devices.clear();
        self.device_code.clear();
        self.verification_uri.clear();
        self.device_expires = None;
        self.logging_in = false;
        self.runtime_running = false;
        let base = self.base_url.clone();
        std::thread::spawn(move || {
            let _ = runtime::stop();
            if let Ok(b) = gateway::base(&base) {
                let _ = gateway::forget(&b);
            }
            let mut c = config::read();
            c["bound"] = json!(false);
            let _ = config::write(&c);
        });
    }
    pub fn tick(&mut self) -> Vec<(String, String)> {
        let mut outputs = Vec::new();
        while let Ok(r) = self.rx.try_recv() {
            match r {
                ResultMsg::LoggedOut => {
                    self.busy = false;
                    self.syncing = false;
                    self.syncing_models = false;
                    self.logging_in = false;
                    self.runtime_running = false;
                    self.key_configured = false;
                    self.account = Value::Null;
                    self.models.clear();
                    self.devices.clear();
                    self.device_code.clear();
                    self.verification_uri.clear();
                    self.device_expires = None;
                    self.status = "已退出登录".into();
                }
                ResultMsg::Status(r) => {
                    self.syncing = false;
                    self.syncing_models = false;
                    self.nullclaw = runtime::installed();
                    // 登录进行中时，其他操作的完成消息不得清掉验证码或解除忙碌
                    if !self.logging_in {
                        self.busy = false;
                        self.device_code.clear();
                        self.verification_uri.clear();
                        self.device_expires = None;
                    }
                    if r.as_ref().is_err_and(|e| e.contains("登录已失效")) {
                        self.clear_auth();
                    }
                    let ok = r.is_ok();
                    self.runtime_status = r.clone().unwrap_or_else(|e| e);
                    self.status = self.runtime_status.clone();
                    if !self.key_configured && !self.logging_in {
                        // 登录页不展示“已安装”等运行时准备状态
                        self.status.clear();
                    }
                    self.reconnect = ok;
                    self.runtime_running = runtime::is_running();
                    if ok && self.key_configured {
                        self.last_sync = std::time::Instant::now();
                        // 启动时账户已并行同步；这里只补缺失的部分，避免重复请求
                        if self.models.is_empty() {
                            self.sync_models();
                        }
                        if self.account.is_null() {
                            self.sync_account();
                        }
                    }
                }
                ResultMsg::Notice(n) => {
                    self.status = n.clone();
                    self.runtime_status = n;
                    self.runtime_running = runtime::is_running();
                }
                ResultMsg::Code {
                    code,
                    url,
                    expires_in,
                } => {
                    self.device_code = code;
                    self.verification_uri = url;
                    self.device_expires = Some(
                        std::time::Instant::now()
                            + std::time::Duration::from_secs(expires_in.max(30)),
                    );
                    self.status = "等待浏览器确认".into();
                }
                ResultMsg::Bound => {
                    self.busy = false;
                    self.syncing = false;
                    self.syncing_models = false;
                    self.logging_in = false;
                    self.key_configured = true;
                    self.device_code.clear();
                    self.verification_uri.clear();
                    self.device_expires = None;
                    self.status = "已登录言灵中转".into();
                    self.reconnect = true;
                    self.runtime_running = runtime::is_running();
                    self.last_sync = std::time::Instant::now();
                    self.last_models_sync = std::time::Instant::now();
                    self.sync();
                }
                ResultMsg::LoginFailed(e) => {
                    self.busy = false;
                    self.logging_in = false;
                    self.device_code.clear();
                    self.verification_uri.clear();
                    self.device_expires = None;
                    self.status = e;
                }
                ResultMsg::Saved(warning) => {
                    self.busy = false;
                    self.key_configured = true;
                    self.status = warning.unwrap_or_else(|| "已应用设置".into());
                    self.runtime_status = self.status.clone();
                    self.reconnect = true;
                    self.runtime_running = runtime::is_running();
                    self.last_sync = std::time::Instant::now();
                    self.sync();
                }
                ResultMsg::Stopped(r) => {
                    self.busy = false;
                    self.runtime_running = runtime::is_running();
                    self.status = r.unwrap_or_else(|e| e);
                    self.runtime_status = self.status.clone();
                }
                ResultMsg::SyncError(e) => {
                    self.syncing = false;
                    if e.contains("登录已失效") {
                        self.clear_auth();
                    }
                    self.status = format!("同步失败：{e}");
                }
                ResultMsg::Account(me, devices) => {
                    self.syncing = false;
                    // 退出/凭据失效后忽略迟到的同步结果
                    if !self.key_configured {
                        continue;
                    }
                    self.account = me;
                    self.devices = devices;
                    self.status = "已同步".into();
                }
                ResultMsg::Models(models, meta) => {
                    self.syncing_models = false;
                    if !self.key_configured {
                        continue;
                    }
                    self.models = models;
                    self.model_meta = meta;
                    if self.models.is_empty() {
                        self.status = "网关暂无可用模型，请联系管理员接入".into();
                    }
                    // 模型被管理员下线/改名：自动切到第一个可用模型并应用
                    if !self.models.is_empty() && !self.models.iter().any(|m| m == &self.model) {
                        let old = self.model.clone();
                        self.model = self.models[0].clone();
                        self.status = format!("模型 {old} 已不可用，已切换到 {}", self.model);
                        self.save();
                    }
                }
                ResultMsg::ModelsError(e) => {
                    self.syncing_models = false;
                    if e.contains("登录已失效") {
                        self.clear_auth();
                    }
                    self.status = format!("模型同步失败：{e}");
                }
                ResultMsg::Output(title, r) => {
                    self.busy = false;
                    outputs.push((title, r.unwrap_or_else(|e| e)));
                }
            }
        }
        if self.key_configured
            && !self.busy
            && !self.syncing
            && self.last_sync.elapsed().as_secs() >= 60
        {
            self.sync();
        }
        // 运行时异常退出时及时反映到输入区，避免继续向已停止的网关发消息
        if self.key_configured && !self.busy && self.runtime_running && !runtime::is_running() {
            self.runtime_running = false;
            self.status = "NullClaw 已停止".into();
        }
        outputs
    }
    pub fn login(&mut self) {
        if self.logging_in {
            self.status = "登录进行中，请在浏览器中确认".into();
            return;
        }
        if self.busy {
            return;
        }
        let b = match gateway::base(&self.base_url) {
            Ok(b) => b,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        self.busy = true;
        self.logging_in = true;
        self.syncing = false;
        self.cancel.store(false, Ordering::Relaxed);
        self.device_code.clear();
        self.verification_uri.clear();
        self.device_expires = None;
        self.status = "正在获取验证码".into();
        let cancel = self.cancel.clone();
        let tx = self.tx.clone();
        let model = self.model.clone();
        let thinking = self.thinking_effort();
        let access_mode = self.access_mode;
        std::thread::spawn(move || {
            let r: Result<Option<String>, String> = (|| {
                let v = gateway::begin(&b)?;
                let dc = v["device_code"].as_str().ok_or("授权响应无效")?;
                let code = v["user_code"].as_str().ok_or("授权响应无效")?.to_string();
                let url = v["verification_uri"]
                    .as_str()
                    .ok_or("授权响应无效")?
                    .to_string();
                let parsed = reqwest::Url::parse(&url).map_err(|_| "授权地址无效")?;
                let expected = reqwest::Url::parse(&b).map_err(|_| "地址无效")?;
                // 授权页必须与网关同源同协议（生产 https；本地回环允许 http）
                if parsed.origin() != expected.origin() || parsed.scheme() != expected.scheme() {
                    return Err("授权地址与网关不一致".into());
                }
                let ttl = v["expires_in"].as_u64().unwrap_or(600).clamp(60, 600);
                let _ = tx.send(ResultMsg::Code {
                    code,
                    url: url.clone(),
                    expires_in: ttl,
                });
                // 打不开浏览器不影响登录：验证码仍显示在客户端，可手动复制链接
                if open::that(&url).is_err() {
                    let _ = tx.send(ResultMsg::Notice(
                        "无法自动打开浏览器，请手动打开授权页".into(),
                    ));
                }
                let started = std::time::Instant::now();
                let mut interval = v["interval"].as_u64().unwrap_or(5).clamp(5, 60);
                let mut transient = 0u32;
                loop {
                    // 等待下一次轮询；不超过验证码剩余有效期
                    let remaining = ttl.saturating_sub(started.elapsed().as_secs());
                    if remaining == 0 {
                        return Err("验证码已过期，请重新登录".into());
                    }
                    for _ in 0..interval.min(remaining) * 5 {
                        if cancel.load(Ordering::Relaxed) {
                            return Err("已取消登录".into());
                        }
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                    if started.elapsed().as_secs() >= ttl {
                        return Err("验证码已过期，请重新登录".into());
                    }
                    let result = match gateway::poll(&b, dc) {
                        Ok(v) => {
                            transient = 0;
                            v
                        }
                        Err(e) => {
                            // 慢网络/抖动：最多重试 6 次并逐步退避，避免一次超时中断登录
                            transient += 1;
                            if transient >= 6 {
                                return Err(format!("网络不稳定，登录未完成：{e}"));
                            }
                            std::thread::sleep(std::time::Duration::from_millis(
                                (500 * transient as u64).min(3_000),
                            ));
                            continue;
                        }
                    };
                    if let Some(k) = result["access_token"].as_str() {
                        if cancel.load(Ordering::Relaxed) {
                            let _ = gateway::revoke(&b, k);
                            return Err("已取消登录".into());
                        }
                        if let Err(e) = gateway::save_key(&b, k) {
                            let _ = gateway::revoke(&b, k);
                            return Err(e);
                        }
                        // 凭据已落地但配置写入失败：回滚，避免留下不可用的登录状态
                        if let Err(e) = persist(&b, &model) {
                            let _ = gateway::revoke(&b, k);
                            return Err(e);
                        }
                        let mut cfg = config::read();
                        cfg["auth_mode"] = json!("device");
                        let _ = config::write(&cfg);
                        // 登录已经成立；运行时配置失败只提示，不回滚登录
                        let runtime_err = (|| {
                            runtime::configure(&format!("{b}/v1"), &model, thinking, access_mode)?;
                            runtime::stop()?;
                            runtime::start(k)?;
                            Ok::<(), String>(())
                        })()
                        .err();
                        return Ok(runtime_err.map(|e| format!("登录成功，但运行时启动失败：{e}")));
                    }
                    let desc = result["error_description"].as_str().unwrap_or("");
                    match result["error"].as_str() {
                        Some("authorization_pending") => {}
                        Some("slow_down") => interval = (interval + 5).min(120),
                        Some("access_denied") => return Err("登录请求已被拒绝".into()),
                        Some("expired_token") => return Err("验证码已过期".into()),
                        Some(_) if !desc.is_empty() => return Err(format!("设备授权失败：{desc}")),
                        Some(other) => return Err(format!("设备授权失败：{other}")),
                        None => return Err("设备授权失败".into()),
                    }
                }
            })();
            match r {
                Ok(warning) => {
                    let _ = tx.send(ResultMsg::Bound);
                    if let Some(e) = warning {
                        let _ = tx.send(ResultMsg::Notice(e));
                    }
                }
                Err(e) => {
                    let _ = tx.send(ResultMsg::LoginFailed(e));
                }
            }
        });
    }
    pub fn cancel_login(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.device_code.clear();
        self.verification_uri.clear();
        self.device_expires = None;
        self.status = "正在取消登录…".into();
    }
    /// 账户/用量每 60 秒同步；模型列表较慢（网关侧可能 10s+），5 分钟或按需刷新
    pub fn sync(&mut self) {
        self.sync_account();
        if self.models.is_empty() || self.last_models_sync.elapsed().as_secs() >= 300 {
            self.sync_models();
        }
    }
    pub fn sync_account(&mut self) {
        if self.busy || self.syncing {
            return;
        }
        self.last_sync = std::time::Instant::now();
        self.syncing = true;
        let base = self.base_url.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = (|| {
                let b = gateway::base(&base)?;
                let k = gateway::key(&b)?;
                let me = optional_get(&b, &k, "/v1/me")?;
                let devices = optional_get(&b, &k, "/v1/devices")
                    .ok()
                    .and_then(|v| v["rows"].as_array().cloned())
                    .unwrap_or_default();
                Ok((me, devices))
            })();
            let _ = tx.send(match r {
                Ok((me, devices)) => ResultMsg::Account(me, devices),
                Err(e) => ResultMsg::SyncError(e),
            });
        });
    }
    pub fn sync_models(&mut self) {
        if self.busy || self.syncing_models {
            return;
        }
        self.last_models_sync = std::time::Instant::now();
        self.syncing_models = true;
        let base = self.base_url.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = (|| {
                let b = gateway::base(&base)?;
                let k = gateway::key(&b)?;
                let data = gateway::get(&b, &k, "/v1/models")?["data"]
                    .as_array()
                    .ok_or("模型列表格式无效")?
                    .clone();
                Ok(parse_models(&data))
            })();
            let _ = tx.send(match r {
                Ok((models, meta)) => ResultMsg::Models(models, meta),
                Err(e) => ResultMsg::ModelsError(e),
            });
        });
    }
    pub fn save(&mut self) -> bool {
        if self.busy {
            return false;
        }
        let b = match gateway::base(&self.base_url) {
            Ok(b) => b,
            Err(e) => {
                self.status = e;
                return false;
            }
        };
        if self.model.trim().is_empty() {
            self.status = "请选择模型".into();
            return false;
        }
        self.busy = true;
        let tx = self.tx.clone();
        let model = self.model.clone();
        let access_mode = self.access_mode;
        let thinking = self.thinking_effort();
        std::thread::spawn(move || {
            let r = (|| {
                let k = gateway::key(&b)?;
                persist(&b, &model)?;
                let mut cfg = config::read();
                cfg["access_mode"] = json!(access_mode);
                config::write(&cfg)?;
                runtime::configure(&format!("{b}/v1"), &model, thinking, access_mode)?;
                // 配置已写入，运行时重启失败只提示
                let warning = runtime::stop()
                    .and_then(|_| runtime::start(&k))
                    .err()
                    .map(|e| format!("模型已保存，但运行时重启失败：{e}"));
                Ok(warning)
            })();
            let _ = tx.send(match r {
                Ok(warning) => ResultMsg::Saved(warning),
                Err(e) => ResultMsg::Status(Err(e)),
            });
        });
        true
    }
    pub fn logout(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        let base = self.base_url.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = (|| {
                let b = gateway::base(&base)?;
                // 先撤销远端令牌；离线时仍允许本地退出
                if let Ok(k) = gateway::key(&b) {
                    let _ = gateway::revoke(&b, &k);
                }
                let _ = gateway::forget(&b);
                let _ = runtime::stop();
                let mut v = config::read();
                v["bound"] = json!(false);
                v["auth_mode"] = json!("device");
                config::write(&v)?;
                Ok::<(), String>(())
            })();
            let _ = tx.send(match r {
                Ok(()) => ResultMsg::LoggedOut,
                Err(e) => ResultMsg::Status(Err(e)),
            });
        });
    }
    /// 暂停托管的 NullClaw；恢复通过保存/重启运行时完成
    pub fn stop_runtime(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = runtime::stop().map(|_| "NullClaw 已暂停".to_string());
            let _ = tx.send(ResultMsg::Stopped(r));
        });
    }
    pub fn update_runtime(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        let base = self.base_url.clone();
        let model = self.model.clone();
        let key_configured = self.key_configured;
        let thinking = self.thinking_effort();
        let access_mode = self.access_mode;
        std::thread::spawn(move || {
            let r = (|| {
                let status = runtime::install(true)?;
                if key_configured {
                    let b = gateway::base(&base)?;
                    let k = gateway::key(&b)?;
                    runtime::configure(&format!("{b}/v1"), &model, thinking, access_mode)?;
                    runtime::start(&k)?;
                }
                Ok(status)
            })();
            let _ = tx.send(ResultMsg::Status(r));
        });
    }
    pub fn cli(&mut self, title: &str, args: Vec<String>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        let title = title.to_string();
        std::thread::spawn(move || {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let _ = tx.send(ResultMsg::Output(title, runtime::run(&refs)));
        });
    }
}
fn optional_get(b: &str, k: &str, path: &str) -> Result<Value, String> {
    match gateway::get(b, k, path) {
        Ok(v) => Ok(v),
        // 凭据失效必须上抛；其他可选接口失败按空值处理，不阻塞登录状态展示
        Err(e) if e.contains("登录已失效") => Err(e),
        Err(_) => Ok(Value::Null),
    }
}
fn persist(b: &str, model: &str) -> Result<(), String> {
    let mut v = config::read();
    v["base_url"] = json!(format!("{b}/v1"));
    v["model"] = json!(model);
    v["bound"] = json!(true);
    config::write(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_model_metadata_and_tolerates_missing_fields() {
        let data = vec![
            json!({"id":"a","context_length":128000,"max_output_tokens":4096,"tps":55,"free":true}),
            json!({"id":"b","context_window":64000,"paid":true}),
        ];
        let (models, meta) = parse_models(&data);
        assert_eq!(models, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            meta["a"],
            ModelMeta {
                ctx: 128000,
                out: 4096,
                tps: 55,
                free: true,
                paid: false,
            }
        );
        assert_eq!(meta["b"].ctx, 64000, "兼容 context_window");
        assert!(meta["b"].paid);
    }
}
