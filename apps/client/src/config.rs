use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
pub fn app_dir() -> PathBuf {
    std::env::var_os("CODING01_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
                .unwrap_or_else(|| home_dir().join(".config"))
                .join("coding01")
        })
}

pub fn bounded_json<T: serde::Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, String> {
    struct Buffer {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for Buffer {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if b.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("JSON 大小超过限制"));
            }
            self.bytes.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut out = Buffer {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut out, value).map_err(|e| e.to_string())?;
    Ok(out.bytes)
}

/// Replace without truncating a working configuration on serialization/write failure.
pub fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("invalid path"))?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct ClientCfg {
    pub ws_url: String,
    pub token: Option<String>,
    pub pairing: bool,
    pub gateway_port: u16,
}
impl Default for ClientCfg {
    fn default() -> Self {
        Self {
            ws_url: "ws://127.0.0.1:32123/ws".into(),
            token: None,
            pairing: false,
            gateway_port: 32100,
        }
    }
}
impl ClientCfg {
    pub fn validate(&self) -> Result<reqwest::Url, String> {
        let u = reqwest::Url::parse(&self.ws_url).map_err(|_| "WebSocket 地址无效")?;
        if !matches!(u.scheme(), "ws" | "wss")
            || u.host_str().is_none()
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
        {
            return Err("WebSocket 地址须不含凭据、查询或片段".into());
        }
        let local = matches!(
            u.host_str(),
            Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
        );
        if !local && (u.scheme() != "wss" || self.pairing) {
            return Err("远程网关须使用 WSS 和通道令牌".into());
        }
        if u.port_or_known_default() == Some(0) || self.gateway_port == 0 {
            return Err("端口不能为 0".into());
        }
        if self
            .token
            .as_ref()
            .is_some_and(|t| t.chars().any(char::is_control))
        {
            return Err("通道令牌不能包含控制字符".into());
        }
        Ok(u)
    }
    pub fn discover() -> Option<Self> {
        let cfg = crate::settings::read_nullclaw()?;
        let acct = cfg
            .pointer("/channels/web/accounts/default")
            .or_else(|| cfg.pointer("/channels/web"))?;
        let host = match acct
            .get("listen")
            .and_then(|v| v.as_str())
            .unwrap_or("127.0.0.1")
        {
            "0.0.0.0" => "127.0.0.1",
            "::" => "[::1]",
            h => h,
        };
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let port = acct
            .get("port")
            .and_then(|v| v.as_u64())
            .filter(|p| *p > 0 && *p <= 65535)
            .unwrap_or(32123);
        let path = acct.get("path").and_then(|v| v.as_str()).unwrap_or("/ws");
        Some(Self {
            ws_url: format!("ws://{host}:{port}{path}"),
            pairing: acct
                .get("message_auth_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("pairing")
                == "pairing",
            token: acct
                .get("auth_token")
                .and_then(|v| v.as_str())
                .map(String::from),
            ..Self::default()
        })
    }
    pub fn load() -> Self {
        let preferences = app_dir().join("preferences.json");
        let current = std::fs::metadata(&preferences)
            .ok()
            .filter(|m| m.len() <= 4 * 1024 * 1024)
            .and_then(|_| std::fs::read(&preferences).ok())
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("connection").cloned())
            .and_then(|v| serde_json::from_value(v).ok());
        current
            .or_else(|| {
                std::fs::read(app_dir().join("connection.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
            })
            .or_else(Self::discover)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_write_replaces_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        write_private(&path, b"first").unwrap();
        write_private(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::*;
    #[test]
    fn serializer_enforces_size_before_writing() {
        let value = serde_json::json!({"text":"你好"});
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(bounded_json(&value, bytes.len()).unwrap(), bytes);
        assert!(bounded_json(&value, bytes.len() - 1).is_err());
    }
    #[test]
    fn websocket_validation_uses_standard_ports_and_protects_credentials() {
        let mut c = ClientCfg {
            ws_url: "ws://localhost/ws".into(),
            ..ClientCfg::default()
        };
        assert_eq!(c.validate().unwrap().port_or_known_default(), Some(80));
        for url in [
            "ws://example.com/ws",
            "wss://user@example.com/ws",
            "wss://example.com/ws?token=secret",
            "ws://localhost:0/ws",
        ] {
            c.ws_url = url.into();
            assert!(c.validate().is_err(), "{url}");
        }
        c.ws_url = "wss://example.com/ws".into();
        c.pairing = true;
        assert!(c.validate().is_err());
        c.pairing = false;
        assert!(c.validate().is_ok());
        c.token = Some("unsafe\nvalue".into());
        assert!(c.validate().is_err());
    }
}
