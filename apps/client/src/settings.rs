//! 设置(模型 API / Web 渠道)——读写 NullClaw 配置,支持重启网关
#![allow(dead_code)]

use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};

pub const DEFAULT_BASE_URL: &str = "https://ai.gzmath.cn/v1";
pub const DEFAULT_MODEL: &str = "deepseek-v4-pro";

#[derive(Clone, Debug)]
pub struct SettingsState {
    pub nullclaw: Option<String>,
    pub base_url: String,
    pub model: String,
    /// 空 = 不修改
    pub key_input: String,
    pub key_configured: bool,
    pub web_port: String,
    /// 空 = 不修改
    pub web_token_input: String,
    pub token_configured: bool,
    pub status: String,
    /// 设置页左侧导航:0=模型 API,1=Web 渠道
    pub tab: usize,
}

fn nullclaw_config_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("USERPROFILE").ok()?;
    Some(Path::new(&home).join(".nullclaw").join("config.json"))
}

pub fn read_nullclaw() -> Option<Value> {
    let text = std::fs::read_to_string(nullclaw_config_path()?).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn discover_nullclaw() -> Option<String> {
    if let Ok(p) = std::env::var("NULLCLAW_BIN") {
        if Path::new(&p).exists() {
            return Some(p);
        }
    }
    let candidates = [
        r"C:\Users\PC\Desktop\01\downloads\nullclaw\nullclaw.exe",
        r"C:\Users\PC\.nullclaw\nullclaw.exe",
    ];
    for c in candidates {
        if Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    if let Ok(out) = Command::new("where").arg("nullclaw").output() {
        if out.status.success() {
            if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                let p = line.trim().to_string();
                if !p.is_empty() {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub fn run_cli(bin: &str, args: &[&str]) -> String {
    match Command::new(bin).args(args).output() {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).to_string();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                text.push_str(&err);
            }
            let text = text.trim();
            if text.is_empty() {
                "(无输出)".to_string()
            } else {
                text.to_string()
            }
        }
        Err(e) => format!("执行失败:{}", e),
    }
}

impl SettingsState {
    pub fn load() -> Self {
        let mut s = Self {
            nullclaw: discover_nullclaw(),
            base_url: DEFAULT_BASE_URL.into(),
            model: DEFAULT_MODEL.into(),
            key_input: String::new(),
            key_configured: false,
            web_port: "32123".into(),
            web_token_input: String::new(),
            token_configured: false,
            status: String::new(),
            tab: 0,
        };
        let Some(cfg) = read_nullclaw() else {
            return s;
        };
        if let Some(providers) = cfg.pointer("/models/providers").and_then(Value::as_object) {
            for (name, pcf) in providers {
                if name.starts_with("custom:") {
                    if let Some(b) = pcf.get("base_url").and_then(Value::as_str) {
                        s.base_url = b.to_string();
                    }
                    s.key_configured = pcf
                        .get("api_key")
                        .and_then(Value::as_str)
                        .map(|k| !k.is_empty())
                        .unwrap_or(false);
                }
            }
        }
        if let Some(prim) = cfg
            .pointer("/agents/defaults/model/primary")
            .and_then(Value::as_str)
        {
            if let Some((_, m)) = prim.rsplit_once('/') {
                s.model = m.to_string();
            }
        }
        if let Some(p) = cfg
            .pointer("/channels/web/accounts/default/port")
            .and_then(Value::as_u64)
        {
            s.web_port = p.to_string();
        }
        s.token_configured = cfg
            .pointer("/channels/web/accounts/default/auth_token")
            .and_then(Value::as_str)
            .map(|t| !t.is_empty())
            .unwrap_or(false);
        s
    }

    /// 写入 NullClaw 配置与客户端配置;返回是否成功
    pub fn save(&mut self) -> bool {
        let Some(bin) = self.nullclaw.clone() else {
            self.status = "未找到 nullclaw 可执行文件".into();
            return false;
        };
        let url = self.base_url.trim().trim_end_matches('/').to_string();
        let model = self.model.trim().to_string();
        if url.is_empty() || model.is_empty() {
            self.status = "API 地址与模型不能为空".into();
            return false;
        }
        let provider_name = format!("custom:{}", url);
        let key = self.key_input.trim().to_string();
        // 1) 有 Key 时用 onboard 一次性写入(自动加密 Key);否则直接改 JSON
        if !key.is_empty() {
            let out = Command::new(&bin)
                .args([
                    "onboard",
                    "--api-key",
                    &key,
                    "--provider",
                    &provider_name,
                    "--model",
                    &model,
                ])
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    self.key_configured = true;
                    self.key_input.clear();
                    self.status = "API 配置已更新".into();
                }
                Ok(o) => {
                    self.status = format!(
                        "写入失败:{}",
                        String::from_utf8_lossy(&o.stderr).trim()
                    );
                    return false;
                }
                Err(e) => {
                    self.status = format!("调用 nullclaw 失败:{}", e);
                    return false;
                }
            }
        }
        // 2) 直接修补 JSON:provider base_url、模型主键、Web 渠道
        let Some(path) = nullclaw_config_path() else {
            self.status = "未找到配置目录".into();
            return false;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            self.status = "读取配置失败".into();
            return false;
        };
        let Ok(mut cfg) = serde_json::from_str::<Value>(&text) else {
            self.status = "配置解析失败".into();
            return false;
        };
        // providers
        let providers = cfg
            .pointer_mut("/models/providers")
            .and_then(Value::as_object_mut);
        if let Some(providers) = providers {
            let entry = providers
                .entry(provider_name.clone())
                .or_insert_with(|| json!({}));
            if let Some(obj) = entry.as_object_mut() {
                obj.insert("base_url".into(), json!(url));
                if !key.is_empty() {
                    // onboard 已写入加密 Key,这里不再覆盖
                }
            }
        }
        // 模型主键
        if let Some(obj) = cfg
            .pointer_mut("/agents/defaults/model")
            .and_then(Value::as_object_mut)
        {
            obj.insert("primary".into(), json!(format!("{}/{}", provider_name, model)));
        }
        // Web 渠道
        if let Ok(port) = self.web_port.trim().parse::<u64>() {
            if let Some(acct) = cfg
                .pointer_mut("/channels/web/accounts/default")
                .and_then(Value::as_object_mut)
            {
                acct.insert("port".into(), json!(port));
                if !self.web_token_input.trim().is_empty() {
                    acct.insert("auth_token".into(), json!(self.web_token_input.trim()));
                    acct.insert("message_auth_mode".into(), json!("token"));
                    self.token_configured = true;
                    self.web_token_input.clear();
                }
            }
        }
        if let Err(e) = std::fs::write(
            &path,
            serde_json::to_string_pretty(&cfg).unwrap_or_default(),
        ) {
            self.status = format!("保存失败:{}", e);
            return false;
        }
        // 3) 同步客户端连接配置(ws 地址 / 通道令牌)
        let token = cfg
            .pointer("/channels/web/accounts/default/auth_token")
            .and_then(Value::as_str)
            .map(String::from);
        let port = cfg
            .pointer("/channels/web/accounts/default/port")
            .and_then(Value::as_u64)
            .unwrap_or(32123);
        crate::config::ClientCfg::save_connection(port, token.as_deref());
        self.status = "已保存;重启网关后生效".into();
        true
    }
}

/// 重启网关(taskkill 旧进程 → 后台拉起新进程)
pub fn restart_gateway(bin: &str, gateway_port: u16) -> String {    let _ = Command::new("taskkill")
        .args(["/IM", "nullclaw.exe", "/F"])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(400));
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new(bin);
    cmd.args([
        "gateway",
        "--port",
        &gateway_port.to_string(),
    ])
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    match cmd.spawn() {
        Ok(_) => {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            "网关已重启".to_string()
        }
        Err(e) => format!("网关启动失败:{}", e),
    }
}

/// 用记事本打开 nullclaw 配置文件
pub fn open_config() -> String {
    let Some(path) = nullclaw_config_path() else {
        return "未找到配置目录".into();
    };
    match std::process::Command::new("notepad").arg(&path).spawn() {
        Ok(_) => format!("已打开 {}", path.display()),
        Err(e) => format!("打开失败:{}", e),
    }
}
