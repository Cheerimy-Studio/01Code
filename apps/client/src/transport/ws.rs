//! NullClaw WebChannel v1 客户端(信封 {v,type,session_id,payload,...})
//!
//! 流程:
//! - 有凭据 → 直接以 token/access_token 发送 user_message
//! - 无凭据 → loopback 先发 pairing_request,pairing_result.ok 后取得 access_token
//! - assistant_chunk/final、tool_call/result、approval_request/error → UI 事件

#![allow(dead_code)]

use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

use super::InMsg;
use crate::config::ClientCfg;
use crate::events::Event;

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

enum Cmd {
    User { sid: String, text: String },
    Approval { sid: String, request_id: String, approved: bool },
}

pub struct Ws {
    rx: Receiver<Event>,
    cmd: Sender<Cmd>,
}

impl Ws {
    pub fn connect(cfg: &ClientCfg) -> Self {
        let (tx, rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        {
            let tx = tx.clone();
            let url = cfg.ws_url.clone();
            let token = cfg.token.clone();
            thread::spawn(move || run(&url, token, tx, cmd_rx));
        }
        Self { rx, cmd: cmd_tx }
    }
}

impl super::Transport for Ws {
    fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
        }
        out
    }

    fn send(&mut self, msg: InMsg) {
        let cmd = match msg {
            InMsg::UserText { sid, text } => Cmd::User { sid, text },
            InMsg::Approval {
                sid,
                request_id,
                approved,
            } => Cmd::Approval {
                sid,
                request_id,
                approved,
            },
        };
        self.cmd.send(cmd).ok();
    }
}

fn envelope(
    typ: &str,
    sid: &str,
    payload: serde_json::Value,
    auth: Option<&str>,
    request_id: Option<&str>,
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
        obj["auth_token"] = serde_json::Value::String(tok.to_string());
    }
    obj.to_string()
}

fn run(url: &str, token: Option<String>, tx: Sender<Event>, cmd: Receiver<Cmd>) {
    // token 模式:升级请求携带 ?token=;配对模式:明文升级后走 pairing_request
    let target = match &token {
        Some(t) => format!(
            "{}{}token={}",
            url,
            if url.contains('?') { "&" } else { "?" },
            t
        ),
        None => url.to_string(),
    };
    let sid_home = "ui-session".to_string();
    match connect(&target) {
        Err(e) => {
            tx.send(Event::Sys {
                sid: String::new(),
                text: format!("网关连接失败:{}", e),
            })
            .ok();
        }
        Ok((mut ws, _resp)) => {
            let mut auth: Option<String> = token;
            if auth.is_none() {
                // loopback 配对:免码
                let req = envelope("pairing_request", &sid_home, serde_json::json!({}), None, None);
                ws.send(Message::Text(req)).ok();
            }
            if let MaybeTlsStream::Plain(stream) = ws.get_ref() {
                let _ = stream.set_nonblocking(true);
            }
            let mut auto_call: u64 = 1;
            let mut stream_open = false;
            loop {
                match ws.read() {
                    Ok(Message::Text(s)) => {
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
                            match c {
                                Cmd::User { sid, text } => {
                                    let frame = envelope(
                                        "user_message",
                                        &sid,
                                        serde_json::json!({ "content": text }),
                                        auth.as_deref(),
                                        None,
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
