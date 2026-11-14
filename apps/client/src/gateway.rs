use serde_json::{json, Value};
pub const CLIENT: &str = "01Code/0.2.2";
pub fn http() -> Result<reqwest::blocking::Client, String> {
    // 慢网络/移动网络下 30s 容易在首包前超时；连接与总超时分开设置
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(60))
        .user_agent(CLIENT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}
pub fn base(input: &str) -> Result<String, String> {
    let u = reqwest::Url::parse(input.trim()).map_err(|_| "请输入有效的网关地址")?;
    let host = u.host_str().unwrap_or("");
    let loopback = host == "localhost"
        || host == "::1"
        || host.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false);
    // 生产必须 HTTPS；本机回环允许 HTTP，便于与本地网关联调
    if (u.scheme() != "https" && !(u.scheme() == "http" && loopback))
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("请使用 HTTPS 地址（本地联调可用 http://127.0.0.1:端口）".into());
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
        .map_err(|_| "请先登录言灵中转".into())
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
    // 网络抖动时重试 3 次（1s/2s 退避）；HTTP 4xx 不重试，直接给出可读原因
    let mut last = String::new();
    for attempt in 0..3 {
        let resp = match http()?
            .post(format!("{base}/api/device/code"))
            .json(&json!({"client_id":"01code","scope":"llm"}))
            .send()
        {
            Ok(resp) => resp,
            Err(e) => {
                last = e.to_string();
                if attempt < 2 {
                    std::thread::sleep(std::time::Duration::from_secs(1 + attempt as u64));
                }
                continue;
            }
        };
        let status = resp.status();
        if status.as_u16() == 429 {
            if attempt < 2 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
            return Err("请求过于频繁，请稍后再试".into());
        }
        if status.is_server_error() {
            last = format!("HTTP {}", status.as_u16());
            if attempt < 2 {
                std::thread::sleep(std::time::Duration::from_secs(1 + attempt as u64));
                continue;
            }
            return Err("网关暂时不可用，请稍后重试".into());
        }
        let v: Value = resp.json().unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(v);
        }
        let err = v["error"].as_str().unwrap_or("invalid_request");
        return Err(match err {
            "invalid_request" => "网关未启用设备登录（请在后台「01Code 设备登录」填写公网入口与可信代理）".into(),
            "invalid_client" => "客户端标识被网关拒绝".into(),
            "invalid_scope" => "授权范围不被支持".into(),
            "slow_down" => "请求过于频繁，请稍后再试".into(),
            other => format!("设备授权失败:{other}"),
        });
    }
    Err(format!("无法连接授权服务（已自动重试，请检查网络后重试）:{last}"))
}
pub fn poll(base: &str, code: &str) -> Result<Value, String> {
    let r=http()?.post(format!("{base}/api/device/token")).json(&json!({"grant_type":"urn:ietf:params:oauth:grant-type:device_code","device_code":code,"client_id":"01code"})).send().map_err(|e|e.to_string())?;
    let status = r.status();
    // 限速时按协议视为 slow_down,由调用方退避重试
    if status.as_u16() == 429 {
        return Ok(json!({"error":"slow_down"}));
    }
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
        assert_eq!(
            base(crate::settings::DEFAULT_BASE_URL).unwrap(),
            "https://ai.gzmath.cn"
        );
        // 本地联调：仅回环地址允许 HTTP
        assert_eq!(
            base("http://127.0.0.1:8100/v1/").unwrap(),
            "http://127.0.0.1:8100"
        );
        assert_eq!(
            base("http://localhost:8100").unwrap(),
            "http://localhost:8100"
        );
        for v in [
            "http://example.org",
            "http://192.168.1.10:8100",
            "https://user:pw@example.org",
            "https://example.org/?key=x",
            "https://example.org/#x",
        ] {
            assert!(base(v).is_err());
        }
    }
}

#[cfg(test)]
mod device_tests {
    use super::*;
    use std::io::Read;
    use tiny_http::{Response, Server, StatusCode};

    #[test]
    fn device_poll_handles_protocol_states() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let handle = std::thread::spawn(move || {
            for _ in 0..5 {
                let mut req = server.recv().unwrap();
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).unwrap();
                let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let dc = v["device_code"].as_str().unwrap_or("").to_string();
                let (code, payload) = match dc.as_str() {
                    "pending" => (400, json!({"error":"authorization_pending"})),
                    "slow" => (400, json!({"error":"slow_down"})),
                    "denied" => (400, json!({"error":"access_denied"})),
                    "token" => (200, json!({"access_token":"sk-usr-test","token_type":"Bearer","expires_in":0,"scope":"llm"})),
                    _ => (429, json!({"error":"rate_limited"})),
                };
                let header = tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json"[..],
                )
                .unwrap();
                let _ = req.respond(
                    Response::from_string(payload.to_string())
                        .with_status_code(StatusCode(code))
                        .with_header(header),
                );
            }
        });
        assert_eq!(poll(&base, "pending").unwrap()["error"], "authorization_pending");
        assert_eq!(poll(&base, "slow").unwrap()["error"], "slow_down");
        assert_eq!(poll(&base, "denied").unwrap()["error"], "access_denied");
        assert_eq!(poll(&base, "token").unwrap()["access_token"], "sk-usr-test");
        assert_eq!(poll(&base, "any").unwrap()["error"], "slow_down");
        handle.join().unwrap();
    }

    #[test]
    fn device_begin_maps_invalid_request() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let handle = std::thread::spawn(move || {
            let req = server.recv().unwrap();
            let _ = req.respond(
                Response::from_string("{\"error\":\"invalid_request\"}")
                    .with_status_code(StatusCode(400)),
            );
        });
        let err = begin(&format!("http://127.0.0.1:{port}")).unwrap_err();
        assert!(err.contains("设备登录"));
        handle.join().unwrap();
    }
}
