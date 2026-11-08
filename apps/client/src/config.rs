use serde_json::{json, Value};
use std::path::PathBuf;
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("CODING01_HOME") {
        return p.into();
    }
    if let Some(p) = std::env::var_os("APPDATA") {
        return PathBuf::from(p).join("01Code");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    #[cfg(target_os = "macos")]
    {
        return home.join("Library/Application Support/01Code");
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("01Code")
    }
}
pub fn read() -> Value {
    std::fs::read(data_dir().join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_slice(&s).ok())
        .unwrap_or(json!({}))
}
pub fn write(v: &Value) -> Result<(), String> {
    private_write(&data_dir().join("config.json"), v)
}
pub fn private_write(path: &std::path::Path, v: &Value) -> Result<(), String> {
    use std::io::Write;
    let dir = path.parent().ok_or("配置目录无效")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(&tmp).map_err(|e| e.to_string())?;
    f.write_all(&serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}
#[derive(Clone, Debug)]
pub struct ClientCfg {
    pub ws_url: String,
    pub token: Option<String>,
}
impl ClientCfg {
    pub fn load() -> Self {
        let v = read();
        Self {
            ws_url: v["ws_url"]
                .as_str()
                .unwrap_or("ws://127.0.0.1:32123/ws")
                .into(),
            token: v["token"].as_str().map(String::from),
        }
    }
    pub fn save_token(token: &str) {
        let mut v = read();
        v["token"] = json!(token);
        let _ = write(&v);
    }
}
