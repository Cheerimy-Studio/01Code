use crate::{
    config::{home_dir, write_private, ClientCfg},
    settings::Preferences,
};
use serde_json::{json, Value};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

pub struct Gateway {
    child: Option<crate::process_tree::ProcessTree>,
    ready: Option<std::sync::mpsc::Receiver<bool>>,
    stopping: Option<std::sync::mpsc::Receiver<()>>,
    pending_start: Option<(String, Preferences, ClientCfg)>,
    ready_cancel: Arc<std::sync::atomic::AtomicBool>,
    logs: Arc<Mutex<Vec<u8>>>,
    pub status: String,
    pub sync_keys: bool,
    pub cron_expression: String,
    pub cron_prompt: String,
    pub cron_id: String,
    pub confirm_remove: bool,
}
impl Default for Gateway {
    fn default() -> Self {
        Self {
            child: None,
            ready: None,
            stopping: None,
            pending_start: None,
            ready_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            logs: Arc::new(Mutex::new(Vec::new())),
            status: "未由 01Code 启动；可连接已运行的网关".into(),
            sync_keys: false,
            cron_expression: "0 9 * * *".into(),
            cron_prompt: String::new(),
            cron_id: String::new(),
            confirm_remove: false,
        }
    }
}
impl Gateway {
    pub fn running(&self) -> bool {
        self.child.is_some() || self.stopping.is_some()
    }
    pub fn start(&mut self, bin: &str, prefs: &Preferences, connection: &ClientCfg) {
        if self.running() {
            self.status = "网关正在运行或停止中".into();
            return;
        }
        let result = (|| {
            let url = connection.validate()?;
            if !matches!(
                url.host_str(),
                Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
            ) {
                return Err("远程网关请在远程主机启动，本地启动仅支持回环地址".into());
            }
            if url.scheme() != "ws" {
                return Err("本地启动使用 WS；WSS 网关请自行配置 TLS 后连接".into());
            }
            let web_port = url.port_or_known_default().ok_or("WebChannel 端口无效")?;
            if web_port == connection.gateway_port {
                return Err("WebChannel 和 HTTP 网关端口不能相同".into());
            }
            let host = if url.host_str().is_some_and(|h| h.contains(':')) {
                "::1"
            } else {
                "127.0.0.1"
            };
            for port in [connection.gateway_port, web_port] {
                let socket =
                    std::net::SocketAddr::new(host.parse().map_err(|_| "网关主机无效")?, port);
                if std::net::TcpStream::connect_timeout(
                    &socket,
                    std::time::Duration::from_millis(100),
                )
                .is_ok()
                {
                    return Err(format!("本机端口 {port} 已被占用；可直接连接已有网关"));
                }
            }
            let mut command = Command::new(bin);
            command
                .args([
                    "gateway",
                    "--host",
                    host,
                    "--port",
                    &connection.gateway_port.to_string(),
                    "--workspace",
                    &prefs.workspace,
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child =
                crate::process_tree::ProcessTree::spawn(&mut command).map_err(|e| e.to_string())?;
            if let Ok(mut logs) = self.logs.lock() {
                logs.clear();
            }
            for pipe in [
                child
                    .child
                    .stdout
                    .take()
                    .map(|p| Box::new(p) as Box<dyn Read + Send>),
                child
                    .child
                    .stderr
                    .take()
                    .map(|p| Box::new(p) as Box<dyn Read + Send>),
            ]
            .into_iter()
            .flatten()
            {
                let logs = self.logs.clone();
                std::thread::spawn(move || {
                    let mut pipe = pipe;
                    let mut buf = [0u8; 4096];
                    while let Ok(n) = pipe.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        if let Ok(mut logs) = logs.lock() {
                            logs.extend_from_slice(&buf[..n]);
                            if logs.len() > 64 * 1024 {
                                let count = logs.len() - 64 * 1024;
                                logs.drain(..count);
                            }
                        }
                    }
                });
            }
            self.child = Some(child);
            let port = url.port_or_known_default().ok_or("WebChannel 端口无效")?;
            let address = if url.host_str().is_some_and(|s| s.contains(':')) {
                format!("[::1]:{port}")
            } else {
                format!("127.0.0.1:{port}")
            };
            let (tx, rx) = std::sync::mpsc::channel();
            self.ready = Some(rx);
            self.ready_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let cancelled = self.ready_cancel.clone();
            std::thread::spawn(move || {
                let start = std::time::Instant::now();
                while start.elapsed().as_secs() < 20 {
                    if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }
                    if std::net::TcpStream::connect(&address).is_ok() {
                        let _ = tx.send(true);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                let _ = tx.send(false);
            });
            Ok::<(), String>(())
        })();
        self.status = match result {
            Ok(()) => "网关进程已启动，等待 WebChannel 就绪；可点击重新连接".into(),
            Err(e) => format!("启动失败：{e}"),
        };
    }
    pub fn can_restart(&self) -> bool {
        self.child.is_some()
    }
    pub fn restart(&mut self, bin: String, prefs: Preferences, connection: ClientCfg) {
        if self.child.is_none() {
            self.status = "只能重启本客户端启动的网关".into();
            return;
        }
        self.stop();
        self.pending_start = Some((bin, prefs, connection));
    }
    pub fn stop(&mut self) {
        self.pending_start = None;
        self.ready_cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let Some(mut child) = self.child.take() else {
            self.status = "此网关不是由 01Code 启动，未结束外部进程".into();
            return;
        };
        self.ready = None;
        let (tx, rx) = std::sync::mpsc::channel();
        self.stopping = Some(rx);
        std::thread::spawn(move || {
            child.terminate();
            let _ = tx.send(());
        });
        self.status = "已请求停止本客户端启动的网关".into();
    }
    pub fn shutdown(&mut self) {
        self.pending_start = None;
        self.ready_cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.ready = None;
        if let Some(mut child) = self.child.take() {
            child.terminate();
        }
        if let Some(rx) = self.stopping.take() {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
        }
    }
    pub fn poll(&mut self) -> bool {
        if let Some(child) = &mut self.child {
            if let Ok(Some(code)) = child.try_wait() {
                self.status = format!("网关进程已退出：{code}。请检查诊断日志");
                self.child = None;
                self.ready_cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                self.ready = None;
            }
        }
        if self
            .stopping
            .as_ref()
            .is_some_and(|rx| rx.try_recv().is_ok())
        {
            self.stopping = None;
            self.status = "网关已停止".into();
            if let Some((bin, prefs, connection)) = self.pending_start.take() {
                self.start(&bin, &prefs, &connection);
            }
        }
        if let Some(ready) = self.ready.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.ready = None;
            self.status = if ready {
                "WebChannel 已就绪，正在连接"
            } else {
                "进程仍在运行，但 WebChannel 未就绪，请检查日志与配置"
            }
            .into();
            return ready;
        }
        false
    }
    pub fn logs(&self) -> String {
        self.logs
            .lock()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    }
}
impl Drop for Gateway {
    fn drop(&mut self) {
        self.shutdown();
    }
}
pub fn config_path() -> PathBuf {
    std::env::var_os("NULLCLAW_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".nullclaw"))
        .join("config.json")
}
fn object<'a>(value: &'a mut Value, key: &str) -> Result<&'a mut Value, String> {
    let map = value
        .as_object_mut()
        .ok_or_else(|| format!("{key} 的父节点不是对象"))?;
    let child = map.entry(key).or_insert_with(|| json!({}));
    if !child.is_object() {
        return Err(format!("{key} 配置类型错误，未覆盖"));
    }
    Ok(child)
}
pub fn merge_config(
    mut cfg: Value,
    prefs: &Preferences,
    connection: &ClientCfg,
    sync_keys: bool,
) -> Result<Value, String> {
    if !cfg.is_object() {
        return Err("网关配置根节点必须为对象".into());
    }
    let url = connection.validate()?;
    if url.scheme() != "ws"
        || !matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
        )
    {
        return Err("配置同步仅支持本机 WS 网关".into());
    }
    if connection.gateway_port == 0 {
        return Err("HTTP 网关端口不能为 0".into());
    }
    let port = url.port_or_known_default().ok_or("WebChannel 端口无效")?;
    if port == 0 {
        return Err("WebChannel 端口不能为 0".into());
    }
    if !Path::new(&prefs.workspace).is_dir() {
        return Err("工作区目录不存在".into());
    }
    if port == connection.gateway_port {
        return Err("WebChannel 端口不能与 HTTP 网关端口相同".into());
    }
    let provider = |p: &crate::settings::Upstream| format!("01code-{}", p.id);
    let providers = object(object(&mut cfg, "models")?, "providers")?;
    for p in &prefs.upstreams {
        p.validate()?;
        let endpoint = p.base_url.trim().trim_end_matches('/');
        let mut keys = std::collections::HashSet::new();
        if let Some(existing) = providers.as_object() {
            for (name, entry) in existing {
                let base = entry
                    .get("base_url")
                    .and_then(Value::as_str)
                    .or_else(|| name.strip_prefix("custom:"));
                if base.is_some_and(|b| b.trim_end_matches('/') == endpoint) {
                    if let Some(key) = entry
                        .get("api_key")
                        .and_then(Value::as_str)
                        .filter(|k| !k.is_empty())
                    {
                        keys.insert(key.to_owned());
                    }
                }
            }
        }
        let inherited = if keys.len() == 1 {
            keys.into_iter().next()
        } else {
            None
        };
        let entry = object(providers, &provider(p))?;
        let changed = entry.get("api_key").is_some()
            && entry
                .get("base_url")
                .and_then(Value::as_str)
                .is_none_or(|old| old.trim_end_matches('/') != endpoint);
        if changed {
            entry.as_object_mut().unwrap().remove("api_key");
        }
        if entry
            .get("api_key")
            .is_none_or(|v| v.is_null() || v.as_str() == Some(""))
        {
            if let Some(key) = inherited {
                entry["api_key"] = json!(key);
            }
        }
        entry["base_url"] = json!(p.base_url.trim().trim_end_matches('/'));
        entry["native_tools"] = json!(true);
        entry["api_mode"] = json!("chat_completions");
        if sync_keys {
            let key = p.key()?;
            if !key.is_empty() {
                entry["api_key"] = json!(key);
            }
        }
    }
    let model = object(object(object(&mut cfg, "agents")?, "defaults")?, "model")?;
    model["provider"] = json!(provider(prefs.active()));
    model["primary"] = json!(prefs.active().model);
    let web = object(object(&mut cfg, "channels")?, "web")?;
    // Preserve both supported layouts and every unrelated account.
    let account = if web.get("accounts").is_some() {
        object(object(web, "accounts")?, "default")?
    } else if web.get("listen").is_some() || web.get("port").is_some() {
        web
    } else {
        object(object(web, "accounts")?, "default")?
    };
    account["listen"] = json!(if url.host_str().is_some_and(|h| h.contains(':')) {
        "::1"
    } else {
        "127.0.0.1"
    });
    account["port"] = json!(port);
    account["path"] = json!(if url.path().is_empty() {
        "/ws"
    } else {
        url.path()
    });
    account["transport"] = json!("local");
    account["message_auth_mode"] = json!(if connection.pairing {
        "pairing"
    } else {
        "token"
    });
    if !connection.pairing {
        let token = connection.token.as_deref().ok_or("请先生成通道令牌")?;
        if !(16..=128).contains(&token.len()) || token.chars().any(char::is_control) {
            return Err("通道令牌应为 16–128 字节且不能包含控制字符".into());
        }
        account["auth_token"] = json!(token);
    }
    cfg["workspace"] = json!(prefs.workspace);
    if cfg.get("autonomy").is_none() {
        cfg["autonomy"] = json!({"level":"supervised","workspace_only":true});
    }
    Ok(cfg)
}
pub fn sync_config(
    prefs: &Preferences,
    connection: &ClientCfg,
    sync_keys: bool,
) -> Result<(), String> {
    let path = config_path();
    let bytes = if path.exists() {
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(|e| e.to_string())?
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        bytes
    } else {
        b"{}".to_vec()
    };
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("网关配置超过 4 MiB，未修改".into());
    }
    let cfg: Value = serde_json::from_slice(&bytes).map_err(|_| "现有网关配置无效，未覆盖")?;
    let merged = merge_config(cfg, prefs, connection, sync_keys)?;
    if path.exists() {
        write_private(&path.with_file_name("config.01code-backup.json"), &bytes)
            .map_err(|e| e.to_string())?;
    }
    write_private(
        &path,
        &serde_json::to_vec_pretty(&merged).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn open_gateway_config() -> String {
    let path = config_path();
    let app = if cfg!(windows) {
        "notepad"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match Command::new(app).arg(path).spawn() {
        Ok(_) => "已打开 NullClaw 配置".into(),
        Err(e) => e.to_string(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge_preserves_secrets_other_channels_and_autonomy() {
        let p = Preferences::default();
        let id = format!("01code-{}", p.active().id);
        let cfg = json!({"models":{"providers":{id.clone():{"api_key":"encrypted:old","base_url":p.active().base_url}}},"channels":{"telegram":{"token":"keep"}},"autonomy":{"level":"read_only"},"extra":42});
        let c = ClientCfg {
            token: Some("0123456789abcdef0123456789abcdef".into()),
            ..ClientCfg::default()
        };
        let merged = merge_config(cfg, &p, &c, false).unwrap();
        assert_eq!(
            merged["models"]["providers"][id]["api_key"],
            "encrypted:old"
        );
        assert_eq!(merged["channels"]["telegram"]["token"], "keep");
        assert_eq!(merged["extra"], 42);
        assert_eq!(merged["autonomy"]["level"], "read_only");
    }
    #[test]
    fn refused_bad_schema_and_remote_gateway() {
        let p = Preferences::default();
        assert!(merge_config(json!({"models":[]}), &p, &ClientCfg::default(), false).is_err());
        let c = ClientCfg {
            ws_url: "wss://example.com/ws".into(),
            ..ClientCfg::default()
        };
        assert!(merge_config(json!({}), &p, &c, false).is_err());
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn reuses_unique_existing_ciphertext_for_same_endpoint() {
        let p = Preferences::default();
        let id = format!("01code-{}", p.active().id);
        let cfg = json!({"models":{"providers":{"custom:old":{"base_url":p.active().base_url,"api_key":"enc:existing-key"}}}});
        let c = ClientCfg {
            token: Some("0123456789abcdef0123456789abcdef".into()),
            ..ClientCfg::default()
        };
        let merged = merge_config(cfg, &p, &c, false).unwrap();
        assert_eq!(
            merged["models"]["providers"][id]["api_key"],
            "enc:existing-key"
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    fn connection() -> ClientCfg {
        ClientCfg {
            token: Some("0123456789abcdef".into()),
            ..ClientCfg::default()
        }
    }
    #[test]
    fn endpoint_change_cannot_reuse_previous_host_secret() {
        let p = Preferences::default();
        let id = format!("01code-{}", p.active().id);
        let cfg = json!({"models":{"providers":{id.clone():{"base_url":"https://old.example/v1","api_key":"enc:old-secret"}}}});
        let merged = merge_config(cfg, &p, &connection(), false).unwrap();
        assert!(merged["models"]["providers"][&id].get("api_key").is_none());
    }
    #[test]
    fn ambiguous_shared_endpoint_secrets_are_not_inherited() {
        let p = Preferences::default();
        let cfg = json!({"models":{"providers":{"a":{"base_url":p.active().base_url,"api_key":"enc:a"},"b":{"base_url":p.active().base_url,"api_key":"enc:b"}}}});
        let merged = merge_config(cfg, &p, &connection(), false).unwrap();
        assert!(
            merged["models"]["providers"][format!("01code-{}", p.active().id)]
                .get("api_key")
                .is_none()
        );
    }
    #[test]
    fn implicit_websocket_port_matches_synced_gateway() {
        let c = ClientCfg {
            ws_url: "ws://localhost/ws".into(),
            ..connection()
        };
        let merged = merge_config(json!({}), &Preferences::default(), &c, false).unwrap();
        assert_eq!(merged["channels"]["web"]["accounts"]["default"]["port"], 80);
    }
    #[test]
    #[cfg(unix)]
    fn owned_restart_waits_for_stop_and_shutdown_reaps_child() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("runtime");
        std::fs::write(&bin, b"#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let http = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let web = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let c = ClientCfg {
            gateway_port: http.local_addr().unwrap().port(),
            ws_url: format!("ws://127.0.0.1:{}/ws", web.local_addr().unwrap().port()),
            ..connection()
        };
        drop(http);
        drop(web);
        let prefs = Preferences {
            workspace: dir.path().to_string_lossy().into_owned(),
            ..Default::default()
        };
        let mut gateway = Gateway::default();
        gateway.start(bin.to_str().unwrap(), &prefs, &c);
        assert!(gateway.can_restart(), "{}", gateway.status);
        let old = gateway.child.as_ref().unwrap().child.id();
        gateway.restart(bin.to_string_lossy().into_owned(), prefs, c);
        let start = std::time::Instant::now();
        while gateway.stopping.is_some() && start.elapsed() < std::time::Duration::from_secs(2) {
            gateway.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(gateway.can_restart(), "{}", gateway.status);
        let new = gateway.child.as_ref().unwrap().child.id();
        assert_ne!(old, new);
        gateway.shutdown();
        assert!(!gateway.running());
        assert_eq!(unsafe { libc::kill(old as i32, 0) }, -1);
        assert_eq!(unsafe { libc::kill(new as i32, 0) }, -1);
    }
    #[test]
    fn occupied_port_is_never_adopted_and_external_gateway_cannot_restart() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let c = ClientCfg {
            gateway_port: listener.local_addr().unwrap().port(),
            ..connection()
        };
        let mut gateway = Gateway::default();
        gateway.start("binary-must-not-be-executed", &Preferences::default(), &c);
        assert!(gateway.status.contains("已被占用"));
        assert!(!gateway.running());
        gateway.restart(
            "binary-must-not-be-executed".into(),
            Preferences::default(),
            c,
        );
        assert!(gateway.status.contains("只能重启"));
    }
}
