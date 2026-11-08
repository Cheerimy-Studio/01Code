//! NullClaw WebChannel v1 客户端(信封 {v,type,session_id,payload,...})
//!
//! 流程:
//! - 有凭据 → 直接以 token/access_token 发送 user_message
//! - 无凭据 → loopback 先发 pairing_request,pairing_result.ok 后取得 access_token
//! - assistant_chunk/final、tool_call/result、approval_request/error → UI 事件

#![allow(dead_code)]

use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

use super::InMsg;
use crate::config::ClientCfg;
use crate::events::Event;

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

enum Cmd {
    User {
        sid: String,
        text: String,
    },
    Approval {
        sid: String,
        request_id: String,
        approved: bool,
    },
}

pub struct Ws {
    rx: Receiver<Event>,
    tx: Sender<Event>,
    cfg: ClientCfg,
    workers: std::collections::HashMap<String, (Sender<Cmd>, Arc<AtomicBool>)>,
}
impl Ws {
    pub fn connect(cfg: &ClientCfg) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            rx,
            tx,
            cfg: cfg.clone(),
            workers: std::collections::HashMap::new(),
        }
    }
}
impl Drop for Ws {
    fn drop(&mut self) {
        for (_, stop) in self.workers.values() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}
impl super::Transport for Ws {
    fn poll(&mut self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }
    fn send(&mut self, msg: InMsg) {
        let (sid, mut command) = match msg {
            InMsg::UserText { sid, text } => (sid.clone(), Cmd::User { sid, text }),
            InMsg::Approval {
                sid,
                request_id,
                approved,
            } => (
                sid.clone(),
                Cmd::Approval {
                    sid,
                    request_id,
                    approved,
                },
            ),
        };
        if let Some((worker, _)) = self.workers.get(&sid) {
            match worker.send(command) {
                Ok(()) => return,
                Err(e) => command = e.0,
            }
        }
        if let Some((_, stop)) = self.workers.remove(&sid) {
            stop.store(true, Ordering::Relaxed);
        }
        let (cmd, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        let session = sid.clone();
        thread::spawn(move || run(&cfg.ws_url, cfg.token, tx, rx, stopped, &session));
        let _ = cmd.send(command);
        self.workers.insert(sid, (cmd, stop));
    }
}

fn envelope(
    typ: &str,
    sid: &str,
    payload: serde_json::Value,
    auth: Option<&str>,
    request_id: Option<&str>,
    pairing: bool,
) -> String {
    let mut obj = serde_json::json!({
        "v": 1,
        "type": typ,
        "session_id": sid,
        "payload": payload,
    });
    if let Some(rid) = request_id {
        obj["request_id"] = serde_json::Value::String(rid.to_string());
    }
    if let Some(tok) = auth {
        // token 模式仅携带 auth_token;pairing 模式的 access_token 由配对流程单独使用
        obj[if pairing {
            "access_token"
        } else {
            "auth_token"
        }] = serde_json::Value::String(tok.to_string());
    }
    obj.to_string()
}

fn run(
    url: &str,
    token: Option<String>,
    tx: Sender<Event>,
    cmd: Receiver<Cmd>,
    shutdown: Arc<AtomicBool>,
    session: &str,
) {
    // token 模式:升级请求携带 ?token=;配对模式:明文升级后走 pairing_request
    let Ok(mut target) = reqwest::Url::parse(url) else {
        return;
    };
    {
        let mut q = target.query_pairs_mut();
        q.append_pair("session_id", session);
        if let Some(t) = &token {
            q.append_pair("token", t);
        }
    }
    let target = target.to_string();
    let sid_home = session.to_string();
    let mut attempts = 0u64;
    loop {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        match connect(&target) {
            Err(e) => {
                if attempts % 10 == 0 {
                    let _ = tx.send(Event::Sys {
                        sid: String::new(),
                        text: format!("正在连接 NullClaw：{e}"),
                    });
                }
                attempts += 1;
                for _ in 0..10 {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
            Ok((mut ws, _resp)) => {
                let pairing = token.is_none();
                let mut last_activity = std::time::Instant::now();
                let mut auth: Option<String> = token.clone();
                if auth.is_none() {
                    // loopback 配对:免码
                    let req = envelope(
                        "pairing_request",
                        &sid_home,
                        serde_json::json!({}),
                        None,
                        None,
                        pairing,
                    );
                    ws.send(Message::Text(req)).ok();
                }
                if let MaybeTlsStream::Plain(stream) = ws.get_ref() {
                    let _ = stream.set_nonblocking(true);
                }
                let mut auto_call: u64 = 1;
                let mut stream_open = false;
                loop {
                    if shutdown.load(Ordering::Relaxed) || last_activity.elapsed().as_secs() > 180 {
                        let _ = ws.close(None);
                        return;
                    }
                    match ws.read() {
                        Ok(Message::Text(s)) => {
                            last_activity = std::time::Instant::now();
                            let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
                                continue;
                            };
                            let Some(ev) = Event::from_webchannel(&v, auto_call) else {
                                continue;
                            };
                            match &ev {
                                Event::Paired { token } => {
                                    auth = Some(token.clone());
                                    stream_open = false;
                                }
                                Event::MessageDelta { .. } => {
                                    if !stream_open {
                                        // 首个 chunk 前补 begin(协议只有 chunk)
                                        if let Event::MessageDelta { sid, .. } = &ev {
                                            tx.send(Event::MessageBegin {
                                                sid: sid.clone(),
                                                role: crate::events::Role::Assistant,
                                            })
                                            .ok();
                                        }
                                        stream_open = true;
                                    }
                                }
                                Event::MessageFinal { .. } => {
                                    stream_open = false;
                                }
                                Event::ToolCall { .. } => {
                                    auto_call += 1;
                                }
                                _ => {}
                            }
                            tx.send(ev).ok();
                        }
                        Ok(Message::Ping(p)) => {
                            ws.send(Message::Pong(p)).ok();
                        }
                        Ok(Message::Close(_)) => {
                            tx.send(Event::Sys {
                                sid: String::new(),
                                text: "网关已关闭连接".into(),
                            })
                            .ok();
                            break;
                        }
                        Ok(_) => {}
                        // 分片 UTF-8 帧:忽略该帧,保持连接
                        Err(tungstenite::Error::Utf8) => {
                            continue;
                        }
                        Err(tungstenite::Error::Io(e))
                            if e.kind() == std::io::ErrorKind::WouldBlock =>
                        {
                            while let Ok(c) = cmd.try_recv() {
                                last_activity = std::time::Instant::now();
                                match c {
                                    Cmd::User { sid, text } => {
                                        let frame = envelope(
                                            "user_message",
                                            &sid,
                                            serde_json::json!({ "content": text }),
                                            auth.as_deref(),
                                            None,
                                            pairing,
                                        );
                                        if let Err(e) = ws.send(Message::Text(frame)) {
                                            eprintln!("[ws] send user_message failed: {e}");
                                        }
                                        let _ = ws.flush();
                                    }
                                    Cmd::Approval {
                                        sid,
                                        request_id,
                                        approved,
                                    } => {
                                        let frame = envelope(
                                            "approval_response",
                                            &sid,
                                            serde_json::json!({ "approved": approved }),
                                            auth.as_deref(),
                                            Some(&request_id),
                                            pairing,
                                        );
                                        ws.send(Message::Text(frame)).ok();
                                        let _ = ws.flush();
                                    }
                                }
                            }
                            thread::sleep(Duration::from_millis(25));
                        }
                        Err(e) => {
                            tx.send(Event::Sys {
                                sid: String::new(),
                                text: format!("连接中断:{}", e),
                            })
                            .ok();
                            break;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairing_and_token_fields_are_distinct() {
        let pair: serde_json::Value = serde_json::from_str(&envelope(
            "user_message",
            "s",
            serde_json::json!({}),
            Some("secret"),
            None,
            true,
        ))
        .unwrap();
        assert_eq!(pair["access_token"], "secret");
        assert!(pair.get("auth_token").is_none());
        let token: serde_json::Value = serde_json::from_str(&envelope(
            "user_message",
            "s",
            serde_json::json!({}),
            Some("secret"),
            None,
            false,
        ))
        .unwrap();
        assert_eq!(token["auth_token"], "secret");
    }
}
