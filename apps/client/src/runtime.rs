use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
};
fn child() -> &'static Mutex<Option<Child>> {
    static C: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}
pub fn home() -> PathBuf {
    crate::config::data_dir().join("nullclaw")
}
pub fn bin() -> PathBuf {
    crate::config::data_dir()
        .join("bin")
        .join(if cfg!(windows) {
            "nullclaw.exe"
        } else {
            "nullclaw"
        })
}
pub fn installed() -> Option<String> {
    let p = bin();
    p.is_file().then(|| p.to_string_lossy().into_owned())
}

/// 安装日志文件(独立于运行时日志,便于排查下载/校验问题)
pub fn install_log_path() -> PathBuf {
    crate::config::data_dir().join("runtime-install.log")
}

fn log_install(line: &str) {
    use std::io::Write;
    let path = install_log_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(
            f,
            "[{}] {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            line
        );
    }
}

/// 国内 GitHub 加速镜像(前缀形式);CODING01_GH_MIRROR 可覆盖并优先使用
const GH_MIRRORS: [&str; 5] = [
    "https://ghfast.top/",
    "https://gh-proxy.com/",
    "https://ghproxy.net/",
    "https://mirror.ghproxy.com/",
    "https://gh.llkk.cc/",
];

fn mirror_urls(url: &str) -> Vec<String> {
    let mut list = vec![url.to_string()];
    let mut prefixes = Vec::new();
    if let Ok(m) = std::env::var("CODING01_GH_MIRROR") {
        let m = m.trim();
        if !m.is_empty() {
            prefixes.push(if m.ends_with('/') {
                m.to_string()
            } else {
                format!("{m}/")
            });
        }
    }
    for m in GH_MIRRORS {
        prefixes.push(m.to_string());
    }
    for p in prefixes {
        list.push(format!("{p}{url}"));
    }
    list
}

fn probe_binary(path: &std::path::Path) -> Result<(), String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version").stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err("安装文件无法运行".into())
                };
            }
            Ok(None) if started.elapsed().as_secs() > 10 => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("安装文件运行超时".into());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    }
}
pub fn command() -> Command {
    let mut c = Command::new(bin());
    c.env("NULLCLAW_HOME", home());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    c
}
fn proxy() -> &'static Mutex<Option<crate::local_proxy::Proxy>> {
    static P: OnceLock<Mutex<Option<crate::local_proxy::Proxy>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(None))
}
pub fn stop() -> Result<(), String> {
    proxy().lock().unwrap().take();
    if let Some(mut c) = child().lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    Ok(())
}
pub fn start(key: &str) -> Result<String, String> {
    let mut guard = child().lock().unwrap();
    if let Some(c) = guard.as_mut() {
        if c.try_wait().map_err(|e| e.to_string())?.is_none() {
            return Ok("运行中".into());
        }
    }
    let cfg = crate::config::read();
    let remote = cfg["base_url"].as_str().ok_or("请先绑定服务")?;
    let model = cfg["model"]
        .as_str()
        .unwrap_or(crate::settings::DEFAULT_MODEL);
    let local = crate::local_proxy::Proxy::start(remote, key)?;
    configure(
        &local.base,
        model,
        cfg["workspace_only"].as_bool().unwrap_or(true),
    )?;
    let local_token = local.token.clone();
    *proxy().lock().unwrap() = Some(local);
    let logs = home().join("runtime.log");
    std::fs::create_dir_all(home()).map_err(|e| e.to_string())?;
    let log = std::fs::File::create(logs).map_err(|e| e.to_string())?;
    let mut c = command();
    c.args(["gateway", "--port", "32100"])
        .env("NULLCLAW_API_KEY", local_token)
        .stdout(Stdio::null())
        .stderr(log);
    let mut process = c.spawn().map_err(|e| e.to_string())?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    if process.try_wait().map_err(|e| e.to_string())?.is_some() {
        return Err("NullClaw 启动失败，请查看运行日志".into());
    }
    *guard = Some(process);
    Ok("运行中".into())
}
pub fn run(args: &[&str]) -> Result<String, String> {
    let mut process = command()
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    fn reader<R: std::io::Read + Send + 'static>(mut r: R) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = r.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if out.len() < 1024 * 1024 {
                    out.extend_from_slice(&buf[..n.min(1024 * 1024 - out.len())]);
                }
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    }
    let stdout = reader(process.stdout.take().unwrap());
    let stderr = reader(process.stderr.take().unwrap());
    let started = std::time::Instant::now();
    loop {
        if process.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if started.elapsed().as_secs() > 20 {
            let _ = process.kill();
            let _ = process.wait();
            return Err("操作超时".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let status = process.wait().map_err(|e| e.to_string())?;
    let text = format!(
        "{}{}",
        stdout.join().unwrap_or_default(),
        stderr.join().unwrap_or_default()
    );
    if status.success() {
        Ok(text)
    } else {
        Err(text)
    }
}

pub fn install(update: bool) -> Result<String, String> {
    if !update && bin().exists() {
        return Ok("已安装".into());
    }
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => return Err("当前架构需要手动安装 NullClaw".into()),
    };
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "linux" => "linux",
        "macos" => "macos",
        _ => return Err("当前系统需要手动安装 NullClaw".into()),
    };
    let name = format!(
        "nullclaw-{os}-{arch}.{}",
        if cfg!(windows) { "exe" } else { "bin" }
    );
    log_install(&format!(
        "install start: update={} target={} at {}",
        update,
        name,
        bin().display()
    ));
    let http = crate::gateway::http()?;
    let api = std::env::var("CODING01_RELEASE_API")
        .ok()
        .filter(|u| u.starts_with("https://"))
        .unwrap_or_else(|| {
            "https://api.github.com/repos/nullclaw/nullclaw/releases/latest".into()
        });
    let mut release: Option<Value> = None;
    let mut api_errors: Vec<String> = Vec::new();
    for candidate in mirror_urls(&api) {
        log_install(&format!("release api attempt: {candidate}"));
        match http.get(&candidate).send() {
            Ok(resp) => match resp.error_for_status() {
                Ok(resp) => match resp.json::<Value>() {
                    Ok(v) => {
                        log_install(&format!(
                            "release api ok: {}",
                            v["tag_name"].as_str().unwrap_or("?")
                        ));
                        release = Some(v);
                        break;
                    }
                    Err(e) => {
                        let msg = format!("解析失败:{e}");
                        log_install(&format!("release api bad json: {msg}"));
                        api_errors.push(msg);
                    }
                },
                Err(e) => {
                    let msg = format!("{e}");
                    log_install(&format!("release api http error: {msg}"));
                    api_errors.push(msg);
                }
            },
            Err(e) => {
                let msg = format!("{e:?}");
                log_install(&format!("release api network error: {msg}"));
                api_errors.push(msg);
            }
        }
    }
    let release = release.ok_or_else(|| {
        let detail = api_errors.last().cloned().unwrap_or_default();
        format!(
            "无法获取 NullClaw 发布信息(已尝试国内镜像):{detail};详见安装日志 {}",
            install_log_path().display()
        )
    })?;
    let asset = release["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["name"] == name))
        .ok_or("该版本未提供当前系统的安装文件")?;
    let url = asset["browser_download_url"]
        .as_str()
        .ok_or("下载地址无效")?;
    if !url.starts_with("https://github.com/nullclaw/nullclaw/releases/download/") {
        return Err("下载来源无效".into());
    }
    let expected = asset["digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .ok_or("发布文件缺少 SHA-256 校验值")?;
    use std::io::Read;
    let download = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(300))
        .user_agent(crate::gateway::CLIENT)
        // 镜像站点会跨域重定向,统一只允许 https 且限制次数;内容由 SHA-256 校验兜底
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if a.previous().len() >= 6 {
                return a.error("redirect limit");
            }
            if a.url().scheme() == "https" {
                a.follow()
            } else {
                a.stop()
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let mut bytes: Option<Vec<u8>> = None;
    let mut download_errors: Vec<String> = Vec::new();
    let candidates = mirror_urls(url);
    log_install(&format!("asset candidates: {}", candidates.len()));
    for candidate in candidates {
        log_install(&format!("asset attempt: {candidate}"));
        match download.get(&candidate).send() {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    let msg = format!("HTTP {}", status.as_u16());
                    log_install(&format!("asset http error: {msg}"));
                    download_errors.push(msg);
                    continue;
                }
                let mut buf = Vec::new();
                if let Err(e) = resp.take(100 * 1024 * 1024 + 1).read_to_end(&mut buf) {
                    let msg = format!("读取中断:{e}");
                    log_install(&format!("asset read error: {msg}"));
                    download_errors.push(msg);
                    continue;
                }
                if buf.len() > 100 * 1024 * 1024 {
                    log_install("asset too large");
                    download_errors.push("文件过大".into());
                    continue;
                }
                let sha = format!("{:x}", Sha256::digest(&buf));
                if sha != expected {
                    log_install(&format!(
                        "asset sha mismatch: got={sha} want={expected} bytes={}",
                        buf.len()
                    ));
                    download_errors.push("校验失败".into());
                    continue;
                }
                log_install(&format!("asset ok: {} bytes sha={sha}", buf.len()));
                bytes = Some(buf);
                break;
            }
            Err(e) => {
                let msg = format!("{e:?}");
                log_install(&format!("asset network error: {msg}"));
                download_errors.push(msg);
            }
        }
    }
    let bytes = bytes.ok_or_else(|| {
        let detail = download_errors.last().cloned().unwrap_or_default();
        format!(
            "下载失败(已尝试 GitHub 与国内镜像):{detail};详见安装日志 {}",
            install_log_path().display()
        )
    })?;
    let dst = bin();
    std::fs::create_dir_all(dst.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = dst.with_extension(if cfg!(windows) {
        "download.exe"
    } else {
        "download"
    });
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    if let Err(e) = probe_binary(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        log_install(&format!("probe failed: {e}"));
        return Err(e);
    }
    log_install("probe ok");
    stop()?;
    log_install("runtime stopped");
    let old = dst.with_extension("previous");
    if old.exists() {
        std::fs::remove_file(&old).map_err(|e| e.to_string())?;
    }
    if dst.exists() {
        std::fs::rename(&dst, &old).map_err(|e| e.to_string())?;
    }
    if let Err(e) = std::fs::rename(&tmp, &dst) {
        if old.exists() {
            let _ = std::fs::rename(&old, &dst);
        }
        let _ = std::fs::remove_file(&tmp);
        log_install(&format!("replace failed: {e}"));
        return Err(e.to_string());
    }
    let version = release["tag_name"].as_str().unwrap_or("").to_string();
    log_install(&format!("installed: {} -> {}", version, dst.display()));
    Ok(format!("NullClaw {}", version))
}
pub fn configure(base: &str, model: &str, workspace_only: bool) -> Result<(), String> {
    let path = home().join("config.json");
    let mut v: Value = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(json!({}));
    let provider = format!("custom:{}", base.trim_end_matches('/'));
    if !v["models"].is_object() {
        v["models"] = json!({});
    }
    if !v["models"]["providers"].is_object() {
        v["models"]["providers"] = json!({});
    }
    v["models"]["providers"]
        .as_object_mut()
        .unwrap()
        .retain(|k, _| !k.starts_with("custom:http://127.0.0.1:"));
    v["models"]["providers"][&provider] =
        json!({"base_url":base,"native_tools":true,"user_agent":"01Code/0.2.1"});
    if !v["agents"].is_object() {
        v["agents"] = json!({});
    }
    if !v["agents"]["defaults"].is_object() {
        v["agents"]["defaults"] = json!({});
    }
    v["agents"]["defaults"]["model"] = json!({"primary":format!("{provider}/{model}")});
    let token = crate::config::read()["token"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| {
            use rand::RngCore;
            let mut b = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut b);
            b.iter().map(|v| format!("{v:02x}")).collect()
        });
    if !v["channels"].is_object() {
        v["channels"] = json!({});
    }
    v["channels"]["web"] = json!({"accounts":{"default":{"transport":"local","listen":"127.0.0.1","port":32123,"path":"/ws","auth_token":token,"message_auth_mode":"token","allowed_origins":[]}}});
    v["gateway"] = json!({"host":"127.0.0.1","port":32100,"require_pairing":true});
    if !v["memory"].is_object() {
        v["memory"] = json!({"backend":"sqlite","auto_save":true});
    }
    if !v["autonomy"].is_object() {
        v["autonomy"] = json!({});
    }
    v["autonomy"]["level"] = json!("supervised");
    v["autonomy"]["workspace_only"] = json!(workspace_only);
    crate::config::private_write(&path, &v)?;
    let mut c = crate::config::read();
    c["token"] = json!(token);
    crate::config::write(&c)
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    #[ignore = "Downloads and runs the official NullClaw release against a local mock model"]
    fn official_runtime_roundtrip() {
        use crate::transport::Transport;
        let dir = std::env::temp_dir().join(format!("01code-runtime-{}", std::process::id()));
        std::env::set_var("CODING01_HOME", &dir);
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = done.clone();
        let thread = std::thread::spawn(move || {
            while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok(Some(mut r)) = server.recv_timeout(std::time::Duration::from_millis(200))
                else {
                    continue;
                };
                assert!(r.headers().iter().any(|h| h.field.equiv("X-01Code-Client")
                    && h.value.as_str() == crate::gateway::CLIENT));
                assert!(r.headers().iter().any(
                    |h| h.field.equiv("Authorization") && h.value.as_str() == "Bearer test-key"
                ));
                let mut b = String::new();
                r.as_reader().read_to_string(&mut b).unwrap();
                let v: Value = serde_json::from_str(&b).unwrap_or(Value::Null);
                let body = if v["stream"] == true {
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"roundtrip-ok\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\ndata: [DONE]\n\n".to_string()
                } else {
                    json!({"choices":[{"index":0,"message":{"role":"assistant","content":"roundtrip-ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).to_string()
                };
                let h = tiny_http::Header::from_bytes(
                    "Content-Type",
                    if v["stream"] == true {
                        "text/event-stream"
                    } else {
                        "application/json"
                    },
                )
                .unwrap();
                let _ = r.respond(tiny_http::Response::from_string(body).with_header(h));
            }
        });
        let result = (|| -> Result<(), String> {
            install(false)?;
            let remote = format!("http://127.0.0.1:{port}/v1");
            crate::config::write(&json!({"base_url":remote,"model":"test-model"}))?;
            configure(&remote, "test-model", true)?;
            start("test-key")?;
            let mut ws = crate::transport::ws::Ws::connect(&crate::config::ClientCfg::load());
            let sid = "roundtrip-session".to_string();
            ws.send(crate::transport::InMsg::UserText {
                sid: sid.clone(),
                text: "Reply with roundtrip-ok".into(),
            });
            let deadline = std::time::Instant::now();
            while deadline.elapsed().as_secs() < 60 {
                for e in ws.poll() {
                    match e {
                        crate::events::Event::MessageFinal { sid: s, text } => {
                            if s == sid && text.contains("roundtrip-ok") {
                                return Ok(());
                            }
                        }
                        crate::events::Event::Sys { text, .. } => eprintln!("{text}"),
                        _ => {}
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err("No final response in 60 seconds".into())
        })();
        let _ = stop();
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        thread.join().unwrap();
        if let Err(e) = result {
            if let Ok(log) = std::fs::read_to_string(home().join("runtime.log")) {
                eprintln!("{log}");
            }
            panic!("{e}");
        }
    }
}

#[cfg(test)]
mod mirror_tests {
    use super::*;

    #[test]
    fn mirror_urls_order_and_count() {
        std::env::remove_var("CODING01_GH_MIRROR");
        let v = mirror_urls("https://github.com/a/b/releases/download/v1/x.exe");
        assert_eq!(v[0], "https://github.com/a/b/releases/download/v1/x.exe");
        assert!(v.len() >= 6);
        assert!(v[1].starts_with("https://ghfast.top/https://github.com/"));
    }

    #[test]
    #[ignore = "network: downloads official NullClaw with GitHub mirror fallback"]
    fn install_with_mirror_fallback() {
        let r = install(true);
        eprintln!("install result: {r:?}");
        assert!(r.is_ok());
    }
}
