use crate::{config, gateway, runtime};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc,
};
pub const DEFAULT_BASE_URL: &str = "https://ai.gzmath.cn/v1";
pub const DEFAULT_MODEL: &str = "deepseek-v4-pro";
pub enum ResultMsg {
    LoggedOut,
    Status(Result<String, String>),
    Code { code: String, url: String },
    Bound,
    Snapshot(Value, Vec<String>, Value),
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
    pub account: Value,
    pub usage: Value,
    pub device_code: String,
    pub verification_uri: String,
    pub busy: bool,
    pub reconnect: bool,
    pub workspace_only: bool,
    pub runtime_status: String,
    pub cron_schedule: String,
    pub cron_prompt: String,
    pub cron_id: String,
    tx: Sender<ResultMsg>,
    rx: Receiver<ResultMsg>,
    cancel: Arc<AtomicBool>,
    last_sync: std::time::Instant,
}
impl SettingsState {
    pub fn load() -> Self {
        let v = config::read();
        let (tx, rx) = mpsc::channel();
        let base: String = v["base_url"].as_str().unwrap_or(DEFAULT_BASE_URL).into();
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
            account: Value::Null,
            usage: Value::Null,
            device_code: String::new(),
            verification_uri: String::new(),
            busy: false,
            reconnect: false,
            workspace_only: v["workspace_only"].as_bool().unwrap_or(true),
            runtime_status: "准备中".into(),
            cron_schedule: String::new(),
            cron_prompt: String::new(),
            cron_id: String::new(),
            tx,
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            last_sync: std::time::Instant::now(),
        }
    }
    pub fn bootstrap(&mut self) {
        self.busy = true;
        self.status = "正在准备 NullClaw".into();
        let tx = self.tx.clone();
        let base = self.base_url.clone();
        let model = self.model.clone();
        std::thread::spawn(move || {
            let r = (|| {
                let status = runtime::install(false)?;
                if let Ok(b) = gateway::base(&base) {
                    if let Ok(k) = gateway::key(&b) {
                        runtime::configure(&format!("{b}/v1"), &model, true)?;
                        runtime::start(&k)?;
                    }
                }
                Ok(status)
            })();
            let _ = tx.send(ResultMsg::Status(r));
        });
    }
    pub fn tick(&mut self) -> Vec<(String, String)> {
        let mut outputs = Vec::new();
        while let Ok(r) = self.rx.try_recv() {
            match r {
                ResultMsg::LoggedOut => {
                    self.busy = false;
                    self.key_configured = false;
                    self.account = Value::Null;
                    self.models.clear();
                    self.status = "已断开账户".into();
                }
                ResultMsg::Status(r) => {
                    self.busy = false;
                    self.nullclaw = runtime::installed();
                    // 设备登录流程结束(成功或失败)都清理一次性验证码状态
                    self.device_code.clear();
                    self.verification_uri.clear();
                    if r.as_ref().is_err_and(|e| e.contains("登录已失效")) {
                        self.key_configured = false;
                        self.account = Value::Null;
                        self.models.clear();
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
                    let ok = r.is_ok();
                    self.runtime_status = r.clone().unwrap_or_else(|e| e);
                    self.status = self.runtime_status.clone();
                    self.reconnect = ok;
                    if ok && self.key_configured {
                        self.sync();
                    }
                }
                ResultMsg::Code { code, url } => {
                    self.device_code = code;
                    self.verification_uri = url;
                    self.status = "请在浏览器中确认登录".into();
                }
                ResultMsg::Bound => {
                    self.busy = false;
                    self.key_configured = true;
                    self.device_code.clear();
                    self.verification_uri.clear();
                    self.status = "已绑定言灵中转".into();
                    self.reconnect = true;
                    self.sync();
                }
                ResultMsg::Snapshot(me, models, usage) => {
                    self.busy = false;
                    self.account = me;
                    self.models = models;
                    self.usage = usage;
                    self.status = "已同步".into();
                }
                ResultMsg::Output(title, r) => {
                    self.busy = false;
                    outputs.push((title, r.unwrap_or_else(|e| e)));
                }
            }
        }
        if self.key_configured && !self.busy && self.last_sync.elapsed().as_secs() >= 60 {
            self.sync();
        }
        outputs
    }
    pub fn login(&mut self) {
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
        self.cancel.store(false, Ordering::Relaxed);
        let cancel = self.cancel.clone();
        let tx = self.tx.clone();
        let model = self.model.clone();
        std::thread::spawn(move || {
            let r = (|| {
                let v = gateway::begin(&b)?;
                let dc = v["device_code"].as_str().ok_or("授权响应无效")?;
                let code = v["user_code"].as_str().ok_or("授权响应无效")?.to_string();
                let url = v["verification_uri"]
                    .as_str()
                    .ok_or("授权响应无效")?
                    .to_string();
                let parsed = reqwest::Url::parse(&url).map_err(|_| "授权地址无效")?;
                let expected = reqwest::Url::parse(&b).map_err(|_| "地址无效")?;
                if parsed.origin() != expected.origin() || parsed.scheme() != "https" {
                    return Err("授权地址与网关不一致".into());
                }
                let _ = tx.send(ResultMsg::Code {
                    code,
                    url: url.clone(),
                });
                open::that(url).map_err(|e| e.to_string())?;
                let started = std::time::Instant::now();
                let mut interval = v["interval"].as_u64().unwrap_or(5).clamp(5, 60);
                let ttl = v["expires_in"].as_u64().unwrap_or(600).min(600);
                let mut transient = 0u32;
                loop {
                    for _ in 0..interval * 5 {
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
                            // 瞬时网络错误重试三次,避免一次抖动中断登录
                            transient += 1;
                            if transient >= 3 {
                                return Err(e);
                            }
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
                        persist(&b, &model)?;
                        let mut cfg = config::read();
                        cfg["auth_mode"] = json!("device");
                        config::write(&cfg)?;
                        runtime::configure(&format!("{b}/v1"), &model, true)?;
                        runtime::stop()?;
                        runtime::start(k)?;
                        return Ok(());
                    }
                    match result["error"].as_str() {
                        Some("authorization_pending") => {}
                        Some("slow_down") => interval = (interval + 5).min(120),
                        Some("access_denied") => return Err("登录请求已被拒绝".into()),
                        Some("expired_token") => return Err("验证码已过期".into()),
                        _ => return Err("设备授权失败".into()),
                    }
                }
            })();
            let _ = tx.send(match r {
                Ok(()) => ResultMsg::Bound,
                Err(e) => ResultMsg::Status(Err(e)),
            });
        });
    }
    pub fn cancel_login(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn sync(&mut self) {
        if self.busy {
            return;
        }
        self.last_sync = std::time::Instant::now();
        self.busy = true;
        let base = self.base_url.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = (|| {
                let b = gateway::base(&base)?;
                let k = gateway::key(&b)?;
                let me = gateway::get(&b, &k, "/v1/me").unwrap_or(Value::Null);
                let models = gateway::get(&b, &k, "/v1/models")?["data"]
                    .as_array()
                    .ok_or("模型列表格式无效")?
                    .iter()
                    .filter_map(|m| m["id"].as_str().map(String::from))
                    .collect();
                let usage = gateway::get(&b, &k, "/v1/usage?days=7").unwrap_or(Value::Null);
                Ok((me, models, usage))
            })();
            let _ = tx.send(match r {
                Ok((me, models, usage)) => ResultMsg::Snapshot(me, models, usage),
                Err(e) => ResultMsg::Status(Err(e)),
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
        let workspace_only = self.workspace_only;
        std::thread::spawn(move || {
            let r = (|| {
                let k = gateway::key(&b)?;
                persist(&b, &model)?;
                let mut cfg = config::read();
                cfg["workspace_only"] = json!(workspace_only);
                config::write(&cfg)?;
                runtime::configure(&format!("{b}/v1"), &model, workspace_only)?;
                runtime::stop()?;
                runtime::start(&k)?;
                Ok(())
            })();
            let _ = tx.send(match r {
                Ok(()) => ResultMsg::Bound,
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
                let k = gateway::key(&b)?;
                if config::read()["auth_mode"] == "manual" {
                    gateway::forget(&b)?;
                } else {
                    gateway::revoke(&b, &k)?;
                }
                runtime::stop()?;
                let mut v = config::read();
                v["bound"] = json!(false);
                config::write(&v)?;
                Ok::<(), String>(())
            })();
            let _ = tx.send(match r {
                Ok(()) => ResultMsg::LoggedOut,
                Err(e) => ResultMsg::Status(Err(e)),
            });
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
        std::thread::spawn(move || {
            let r = (|| {
                let status = runtime::install(true)?;
                let b = gateway::base(&base)?;
                if let Ok(k) = gateway::key(&b) {
                    runtime::configure(&format!("{b}/v1"), &model, true)?;
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
fn persist(b: &str, model: &str) -> Result<(), String> {
    let mut v = config::read();
    v["base_url"] = json!(format!("{b}/v1"));
    v["model"] = json!(model);
    v["bound"] = json!(true);
    config::write(&v)
}
