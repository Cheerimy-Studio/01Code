//! 客户端配置:自动发现本地 NullClaw 配置(~/.nullclaw/config.json → channels.web)
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct ClientCfg {
    /// WebChannel v1 地址(由 nullclaw channels.web 推导)
    pub ws_url: String,
    /// 鉴权凭据:token 模式为 auth_token,配对模式为 None(走 pairing_request)
    pub token: Option<String>,
    /// 网关 HTTP 端口(设置页重启网关用)
    pub gateway_port: u16,
}

impl ClientCfg {
    /// 从本地 NullClaw 配置自动发现 WebChannel 参数
    pub fn discover() -> Option<Self> {
        let home = std::env::var("USERPROFILE").ok()?;
        let path = PathBuf::from(home)
            .join(".nullclaw")
            .join("config.json");
        let text = fs::read_to_string(path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let acct = v
            .get("channels")?
            .get("web")?
            .get("accounts")?
            .get("default")?
            .clone();
        let listen = acct
            .get("listen")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("127.0.0.1")
            .to_string();
        let port = acct
            .get("port")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(32123);
        let path = acct
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("/ws")
            .to_string();
        let mode = acct
            .get("message_auth_mode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("pairing")
            .to_string();
        let auth = acct
            .get("auth_token")
            .and_then(serde_json::Value::as_str)
            .map(String::from);
        let token = if mode == "token" { auth } else { None };
        Some(Self {
            ws_url: format!("ws://{}:{}{}", listen, port, path),
            token,
            gateway_port: Self::load_gateway_port(),
        })
    }

    fn load_gateway_port() -> u16 {
        let Ok(text) = fs::read_to_string(Self::client_cfg_path()) else {
            return 32100;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            return 32100;
        };
        v.get("gateway_port")
            .and_then(serde_json::Value::as_u64)
            .map(|p| p as u16)
            .unwrap_or(32100)
    }

    fn client_cfg_path() -> PathBuf {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base).join("coding01").join("config.json")
    }

    /// 设置页保存连接参数(ws 地址 / 通道令牌 / 网关端口)
    pub fn save_connection(web_port: u64, token: Option<&str>) {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        let dir = PathBuf::from(base).join("coding01");
        let _ = fs::create_dir_all(&dir);
        let gateway_port = Self::load_gateway_port();
        let body = serde_json::json!({
            "ws_url": format!("ws://127.0.0.1:{}/ws", web_port),
            "token": token,
            "gateway_port": gateway_port,
        });
        let _ = fs::write(
            dir.join("config.json"),
            serde_json::to_string_pretty(&body).unwrap_or_default(),
        );
    }

    fn _unused_path(&self) {
        let _ = Self::client_cfg_path();
    }

    /// 发现失败时的兜底默认值
    pub fn load() -> Self {
        Self::discover().unwrap_or(Self {
            ws_url: "ws://127.0.0.1:32123/ws".to_string(),
            token: None,
            gateway_port: Self::load_gateway_port(),
        })
    }

    /// pairing_result 取得 access_token 后持久化(仅用于配对模式)
    pub fn save_token(token: &str) {
        let mut cfg = Self::load();
        cfg.token = Some(token.to_string());
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        let dir = PathBuf::from(base).join("coding01");
        let _ = fs::create_dir_all(&dir);
        let body = serde_json::json!({
            "ws_url": cfg.ws_url,
            "token": cfg.token,
            "gateway_port": cfg.gateway_port,
        });
        let _ = fs::write(
            dir.join("config.json"),
            serde_json::to_string_pretty(&body).unwrap_or_default(),
        );
    }
}
