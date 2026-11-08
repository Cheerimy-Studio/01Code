use serde_json::{json, Value};
pub const CLIENT: &str = "01Code/0.2.1";
pub fn http() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(CLIENT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}
pub fn base(input: &str) -> Result<String, String> {
    let u = reqwest::Url::parse(input.trim()).map_err(|_| "请输入有效的 HTTPS 地址")?;
    if u.scheme() != "https"
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("请使用 HTTPS 地址".into());
    }
    Ok(input
        .trim()
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .into())
}
fn entry(base: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("01Code", base).map_err(|e| e.to_string())
}
pub fn key(base: &str) -> Result<String, String> {
    entry(base)?
        .get_password()
        .map_err(|_| "请先登录或填写 API Key".into())
}
pub fn save_key(base: &str, key: &str) -> Result<(), String> {
    entry(base)?
        .set_password(key)
        .map_err(|e| format!("无法保存到系统凭据管理器：{e}"))
}
pub fn forget(base: &str) -> Result<(), String> {
    match entry(base)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
pub fn get(base: &str, key: &str, path: &str) -> Result<Value, String> {
    let r = http()?
        .get(format!("{base}{path}"))
        .bearer_auth(key)
        .header("X-01Code-Client", CLIENT)
        .send()
        .map_err(|e| e.to_string())?;
    if r.status() == 401 {
        return Err("登录已失效，请重新绑定".into());
    }
    r.error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())
}
pub fn revoke(base: &str, key: &str) -> Result<(), String> {
    let r = http()?
        .delete(format!("{base}/api/device/token"))
        .bearer_auth(key)
        .header("X-01Code-Client", CLIENT)
        .send()
        .map_err(|e| e.to_string())?;
    if r.status() != 401 {
        r.error_for_status().map_err(|e| e.to_string())?;
    }
    forget(base)
}
pub fn begin(base: &str) -> Result<Value, String> {
    http()?
        .post(format!("{base}/api/device/code"))
        .json(&json!({"client_id":"01code","scope":"llm"}))
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())
}
pub fn poll(base: &str, code: &str) -> Result<Value, String> {
    let r=http()?.post(format!("{base}/api/device/token")).json(&json!({"grant_type":"urn:ietf:params:oauth:grant-type:device_code","device_code":code,"client_id":"01code"})).send().map_err(|e|e.to_string())?;
    let status = r.status();
    let v: Value = r.json().map_err(|e| e.to_string())?;
    if status.is_success() || status == 400 {
        Ok(v)
    } else {
        Err(format!("授权服务返回 {status}"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origin_validation() {
        assert_eq!(
            base("https://example.org/v1/").unwrap(),
            "https://example.org"
        );
        for v in [
            "http://example.org",
            "https://user:pw@example.org",
            "https://example.org/?key=x",
            "https://example.org/#x",
        ] {
            assert!(base(v).is_err());
        }
    }
}
