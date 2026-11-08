use super::{InMsg, Transport};
use crate::{config::ClientCfg, events::Event};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, SyncSender},
    Arc,
};
use std::time::Duration;
use tungstenite::{
    client::IntoClientRequest, client_tls_with_config, stream::MaybeTlsStream, Message,
};

pub struct Ws {
    rx: Receiver<Event>,
    tx: Sender<InMsg>,
    stop: Arc<AtomicBool>,
}
impl Ws {
    pub fn connect(cfg: &ClientCfg, sid: &str) -> Self {
        let (tx, rx) = mpsc::sync_channel(256);
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let cfg = cfg.clone();
        let sid = sid.to_owned();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        std::thread::spawn(move || {
            if let Err(text) = run(cfg, &sid, cmd_rx, &tx, &worker_stop) {
                if !worker_stop.load(Ordering::SeqCst) {
                    let _ = tx.send(Event::ConnectionStatus {
                        ready: false,
                        text: "连接中断".into(),
                    });
                    let _ = tx.send(Event::Sys { sid, text });
                }
            }
        });
        Self {
            rx,
            tx: cmd_tx,
            stop,
        }
    }
}
impl Drop for Ws {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
impl Transport for Ws {
    fn poll(&mut self) -> Vec<Event> {
        if self.stop.load(Ordering::SeqCst) {
            self.rx.try_iter().for_each(drop);
            Vec::new()
        } else {
            self.rx.try_iter().collect()
        }
    }
    fn send(&mut self, msg: InMsg) {
        if matches!(msg, InMsg::Cancel) {
            self.stop.store(true, Ordering::SeqCst);
        }
        let _ = self.tx.send(msg);
    }
}
fn envelope(
    typ: &str,
    sid: &str,
    payload: serde_json::Value,
    auth: Option<&str>,
    pairing: bool,
    rid: Option<&str>,
) -> String {
    let mut v = serde_json::json!({"v":1,"type":typ,"session_id":sid,"payload":payload});
    if let Some(token) = auth {
        v[if pairing {
            "access_token"
        } else {
            "auth_token"
        }] = serde_json::json!(token);
    }
    if let Some(id) = rid {
        v["request_id"] = serde_json::json!(id);
    }
    v.to_string()
}
fn stream_timeout(stream: &mut MaybeTlsStream<TcpStream>) -> std::io::Result<()> {
    let tcp = match stream {
        MaybeTlsStream::Plain(t) => t,
        MaybeTlsStream::Rustls(t) => &mut t.sock,
        _ => return Err(std::io::Error::other("unsupported TLS stream")),
    };
    tcp.set_read_timeout(Some(Duration::from_millis(100)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(5)))
}
fn run(
    cfg: ClientCfg,
    sid: &str,
    cmd: Receiver<InMsg>,
    tx: &SyncSender<Event>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut url = cfg.validate()?;
    url.query_pairs_mut().append_pair("session_id", sid);
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "WebSocket 请求无效")?;
    // Authenticate the upgrade without putting secrets in URLs or error text.
    if let Some(token) = cfg.token.as_ref().filter(|_| !cfg.pairing) {
        let value = format!("Bearer {token}")
            .parse()
            .map_err(|_| "通道令牌格式无效")?;
        request.headers_mut().insert("Authorization", value);
    }
    let host = url
        .host_str()
        .ok_or("网关地址缺少主机")?
        .trim_matches(['[', ']']);
    let port = url.port_or_known_default().ok_or("网关端口无效")?;
    let addresses: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|_| "网关域名解析失败")?
        .collect();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut tcp = None;
    for address in addresses {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        if let Ok(s) = TcpStream::connect_timeout(&address, remaining) {
            tcp = Some(s);
            break;
        }
    }
    let tcp = tcp.ok_or("网关连接失败，请检查地址、令牌和网关状态")?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|_| "设置握手超时失败")?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|_| "设置握手超时失败")?;
    let ws_config = tungstenite::protocol::WebSocketConfig {
        max_message_size: Some(8 * 1024 * 1024),
        max_frame_size: Some(2 * 1024 * 1024),
        ..Default::default()
    };
    let (mut ws, _) = client_tls_with_config(request, tcp, Some(ws_config), None)
        .map_err(|_| "网关握手失败，请检查地址、令牌和网关状态")?;
    stream_timeout(ws.get_mut()).map_err(|_| "无法设置连接超时")?;
    let mut auth = if cfg.pairing { None } else { cfg.token };
    if cfg.pairing {
        ws.send(Message::Text(envelope(
            "pairing_request",
            sid,
            serde_json::json!({}),
            None,
            true,
            None,
        )))
        .map_err(|_| "发送配对请求失败")?;
    }
    let _ = tx.send(Event::ConnectionStatus {
        ready: !cfg.pairing,
        text: if cfg.pairing {
            "正在配对"
        } else {
            "已连接"
        }
        .into(),
    });
    let mut auto = 0;
    let mut open = false;
    let started = std::time::Instant::now();
    loop {
        if stop.load(Ordering::SeqCst) {
            let _ = ws.close(None);
            return Ok(());
        }
        if cfg.pairing && auth.is_none() && started.elapsed().as_secs() > 20 {
            return Err("网关配对超时，请检查本地配对配置".into());
        }
        if !cfg.pairing || auth.is_some() {
            loop {
                match cmd.try_recv() {
                    Ok(InMsg::UserText { sid, text, .. }) => ws
                        .send(Message::Text(envelope(
                            "user_message",
                            &sid,
                            serde_json::json!({"content":text}),
                            auth.as_deref(),
                            cfg.pairing,
                            None,
                        )))
                        .map_err(|_| "发送消息失败，连接已中断")?,
                    Ok(InMsg::Approval {
                        sid,
                        request_id,
                        approved,
                    }) => ws
                        .send(Message::Text(envelope(
                            "approval_response",
                            &sid,
                            serde_json::json!({"approved":approved}),
                            auth.as_deref(),
                            cfg.pairing,
                            Some(&request_id),
                        )))
                        .map_err(|_| "发送审批失败")?,
                    Ok(InMsg::Cancel) => return Ok(()),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                }
            }
        }
        match ws.read() {
            Ok(Message::Text(s)) => {
                let v: serde_json::Value =
                    serde_json::from_str(&s).map_err(|_| "网关返回无效 JSON")?;
                let Some(ev) = Event::from_webchannel(&v, auto) else {
                    continue;
                };
                match &ev {
                    Event::Paired { token } => {
                        if token.is_empty() {
                            return Err("网关返回空配对凭据".into());
                        }
                        auth = Some(token.clone());
                        let _ = tx.send(Event::ConnectionStatus {
                            ready: true,
                            text: "已连接".into(),
                        });
                    }
                    Event::MessageDelta { sid, .. } if !open => {
                        let _ = tx.send(Event::MessageBegin {
                            sid: sid.clone(),
                            role: crate::events::Role::Assistant,
                        });
                        open = true;
                    }
                    Event::MessageFinal { .. } => open = false,
                    Event::ToolCall { .. } => auto += 1,
                    _ => {}
                }
                if tx.send(ev).is_err() {
                    return Ok(());
                }
            }
            Ok(Message::Close(_)) => return Err("网关已关闭连接，请重新连接后继续".into()),
            Ok(_) => {
                let _ = ws.flush();
            }
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return Err("网关连接中断，请重新连接后继续".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authentication_modes_do_not_mix() {
        let token: serde_json::Value = serde_json::from_str(&envelope(
            "user_message",
            "s",
            serde_json::json!({}),
            Some("secret"),
            false,
            None,
        ))
        .unwrap();
        assert_eq!(token["auth_token"], "secret");
        assert!(token.get("access_token").is_none());
        let paired: serde_json::Value = serde_json::from_str(&envelope(
            "user_message",
            "s",
            serde_json::json!({}),
            Some("access"),
            true,
            None,
        ))
        .unwrap();
        assert_eq!(paired["access_token"], "access");
        assert!(paired.get("auth_token").is_none());
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use std::net::TcpListener;
    #[allow(clippy::result_large_err)] // tungstenite's server callback uses an HTTP response error type.
    fn verify_connection(pairing: bool) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
            let mut ws = tungstenite::accept_hdr(
                tcp,
                move |request: &tungstenite::handshake::server::Request,
                      response: tungstenite::handshake::server::Response| {
                    assert_eq!(request.uri().query(), Some("session_id=test-session"));
                    if pairing {
                        assert!(request.headers().get("Authorization").is_none());
                    } else {
                        assert_eq!(request.headers()["Authorization"], "Bearer upgrade-secret");
                    }
                    Ok(response)
                },
            )
            .unwrap();
            if pairing {
                let req: serde_json::Value =
                    serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(req["type"], "pairing_request");
                ws.send(Message::Text(serde_json::json!({"type":"pairing_result","session_id":"test-session","payload":{"ok":true,"access_token":"paired-secret"}}).to_string())).unwrap();
            }
            let req: serde_json::Value =
                serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(req["session_id"], "test-session");
            assert_eq!(req["payload"]["content"], "你好");
            if pairing {
                assert_eq!(req["access_token"], "paired-secret");
                assert!(req.get("auth_token").is_none());
            } else {
                assert_eq!(req["auth_token"], "upgrade-secret");
                assert!(req.get("access_token").is_none());
            }
            for (kind, text) in [
                ("assistant_chunk", "真实增量"),
                ("assistant_final", "真实增量结束"),
            ] {
                ws.send(Message::Text(serde_json::json!({"type":kind,"session_id":"test-session","payload":{"content":text}}).to_string())).unwrap();
            }
        });
        let cfg = ClientCfg {
            ws_url: format!("ws://{address}/ws"),
            pairing,
            token: Some("upgrade-secret".into()),
            ..ClientCfg::default()
        };
        let mut ws = Ws::connect(&cfg, "test-session");
        ws.send(InMsg::UserText {
            sid: "test-session".into(),
            text: "你好".into(),
            history: Vec::new(),
            content: serde_json::json!("你好"),
        });
        let start = std::time::Instant::now();
        let mut final_seen = false;
        while start.elapsed() < Duration::from_secs(4) {
            for event in ws.poll() {
                if matches!(event,Event::MessageFinal{ref sid,ref text} if sid=="test-session" && text=="真实增量结束")
                {
                    final_seen = true;
                }
            }
            if final_seen {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(final_seen);
        server.join().unwrap();
    }
    #[test]
    fn token_upgrade_binds_correct_session() {
        verify_connection(false);
    }
    #[test]
    fn pairing_queues_messages_until_access_token_received() {
        verify_connection(true);
    }
}

#[cfg(test)]
mod real_gateway_test {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    #[test]
    #[ignore = "requires CODING01_NULLCLAW_TEST_BIN pointing to a verified official release"]
    fn official_nullclaw_round_trip() {
        let binary =
            std::env::var("CODING01_NULLCLAW_TEST_BIN").expect("official test binary path");
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let api = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        api.set_nonblocking(true).unwrap();
        let api_port = api.local_addr().unwrap().port();
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let web_port = reserve.local_addr().unwrap().port();
        drop(reserve);
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let http_port = reserve.local_addr().unwrap().port();
        drop(reserve);
        let token = "01code-real-release-test-token-000000";
        let connection = ClientCfg {
            ws_url: format!("ws://127.0.0.1:{web_port}/ws"),
            token: Some(token.into()),
            gateway_port: http_port,
            ..ClientCfg::default()
        };
        let mut prefs = crate::settings::Preferences {
            workspace: dir.path().to_string_lossy().into_owned(),
            ..Default::default()
        };
        prefs.upstreams[0].base_url = format!("http://127.0.0.1:{api_port}/v1");
        prefs.upstreams[0].model = "smoke-model".into();
        prefs.upstreams[0].api_key = "test-only".into();
        let config = crate::gateway::merge_config(serde_json::json!({"memory":{"backend":"none","auto_save":false},"autonomy":{"level":"read_only","workspace_only":true}}), &prefs, &connection, true).unwrap();
        std::fs::write(
            home.join("config.json"),
            serde_json::to_vec_pretty(&config).unwrap(),
        )
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let api_worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::SeqCst) {
                match api.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        let request_line = line.clone();
                        let mut len = 0;
                        loop {
                            line.clear();
                            reader.read_line(&mut line).unwrap();
                            if line == "\r\n" {
                                break;
                            }
                            if let Some((key, value)) = line.split_once(':') {
                                if key.eq_ignore_ascii_case("content-length") {
                                    len = value.trim().parse::<usize>().unwrap();
                                }
                            }
                        }
                        let mut data = vec![0; len];
                        reader.read_exact(&mut data).unwrap();
                        let value: serde_json::Value =
                            serde_json::from_slice(&data).unwrap_or_default();
                        let (kind, body) = if request_line.starts_with("GET ") {
                            (
                                "application/json",
                                serde_json::json!({"data":[{"id":"smoke-model"}]}).to_string(),
                            )
                        } else if value["stream"].as_bool().unwrap_or(false) {
                            (
                                "text/event-stream",
                                format!(
                                    "data: {}\n\ndata: [DONE]\n\n",
                                    serde_json::json!({"id":"smoke","choices":[{"index":0,"delta":{"content":"gateway-smoke-ok"},"finish_reason":"stop"}]})
                                ),
                            )
                        } else {
                            ("application/json",serde_json::json!({"id":"smoke","choices":[{"index":0,"message":{"role":"assistant","content":"gateway-smoke-ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).to_string())
                        };
                        let _=write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        let log = std::fs::File::create(dir.path().join("gateway.log")).unwrap();
        let mut command = std::process::Command::new(binary);
        command
            .args([
                "gateway",
                "--host",
                "127.0.0.1",
                "--port",
                &http_port.to_string(),
            ])
            .env("NULLCLAW_HOME", &home)
            .env("HOME", dir.path())
            .env("USERPROFILE", dir.path())
            .stdout(log.try_clone().unwrap())
            .stderr(log);
        let mut child = crate::process_tree::ProcessTree::spawn(&mut command).unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let start = std::time::Instant::now();
            while std::net::TcpStream::connect(("127.0.0.1", web_port)).is_err() {
                assert!(
                    start.elapsed() < Duration::from_secs(10),
                    "gateway did not start: {}",
                    std::fs::read_to_string(dir.path().join("gateway.log")).unwrap()
                );
                std::thread::sleep(Duration::from_millis(50));
            }
            let cfg = ClientCfg {
                ws_url: format!("ws://127.0.0.1:{web_port}/ws"),
                token: Some(token.into()),
                pairing: false,
                ..ClientCfg::default()
            };
            let mut client = Ws::connect(&cfg, "01code-smoke-session");
            client.send(InMsg::UserText {
                sid: "01code-smoke-session".into(),
                text: "Reply briefly".into(),
                history: Vec::new(),
                content: serde_json::json!("Reply briefly"),
            });
            let mut received = String::new();
            let mut errors = Vec::new();
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(35) {
                for event in client.poll() {
                    match event {
                        Event::MessageFinal { text, .. } => received = text,
                        Event::Sys { text, .. } => errors.push(text),
                        _ => {}
                    }
                }
                if received.contains("gateway-smoke-ok") {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                received.contains("gateway-smoke-ok"),
                "response={received:?} errors={errors:?} log={}",
                std::fs::read_to_string(dir.path().join("gateway.log")).unwrap()
            );
        }));
        child.terminate();
        stop.store(true, Ordering::SeqCst);
        api_worker.join().unwrap();
        if let Err(e) = outcome {
            std::panic::resume_unwind(e);
        }
    }
}
