//! 统一事件模型 — WebChannel v1 信封 → 01 UI 事件映射
#![allow(dead_code)]

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TodoState {
    Pending,
    Running,
    Done,
}

#[derive(Clone, Debug)]
pub struct TodoItem {
    pub title: String,
    pub state: TodoState,
}

#[derive(Clone, Debug, Default)]
pub struct Diff {
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// 客户端本地新建会话
    SessionStart {
        sid: String,
        model: String,
        workspace: String,
    },
    MessageBegin {
        sid: String,
        role: Role,
    },
    /// 流式增量
    MessageDelta {
        sid: String,
        text: String,
    },
    /// 流式结束(模拟器)
    MessageEnd {
        sid: String,
    },
    /// 以最终文本替换打开的流(WebChannel assistant_final)
    MessageFinal {
        sid: String,
        text: String,
    },
    ToolCall {
        sid: String,
        call: String,
        tool: String,
        meta: String,
    },
    ToolResult {
        sid: String,
        call: String,
        ok: bool,
        meta: String,
        diff: Option<Diff>,
    },
    StateChange {
        sid: String,
        todo: Vec<TodoItem>,
    },
    CtxUsage {
        sid: String,
        used: f32,
        limit: f32,
    },
    SessionDone {
        sid: String,
        cost: String,
        ms: u64,
    },
    /// WebChannel approval_request
    ApprovalRequest {
        sid: String,
        request_id: String,
        action: String,
        reason: String,
    },
    /// pairing_result ok:保存 access_token
    Paired {
        token: String,
    },
    /// 系统级状态(连接失败 / 网关 error 事件)
    Sys {
        sid: String,
        text: String,
    },
}

impl Event {
    /// WebChannel v1 信封 → 事件
    pub fn from_webchannel(v: &Value, auto_call: u64) -> Option<Event> {
        let typ = v.get("type").and_then(Value::as_str)?;
        let sid = v
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("default")
            .to_string();
        let request_id = v
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let p = v.get("payload").cloned().unwrap_or(Value::Null);
        let summary = |val: &Value, max: usize| -> String {
            match val {
                Value::String(s) => {
                    let t: String = s.chars().take(max).collect();
                    t
                }
                Value::Object(o) => {
                    let mut parts = Vec::new();
                    for (k, val) in o.iter().take(3) {
                        let raw = match val {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        let short: String = raw.chars().take(48).collect();
                        parts.push(format!("{}={}", k, short));
                    }
                    parts.join(" ")
                }
                Value::Null => String::new(),
                other => {
                    let t = other.to_string();
                    t.chars().take(max).collect()
                }
            }
        };
        match typ {
            "pairing_result" => {
                let ok = p.get("ok").and_then(Value::as_bool).unwrap_or(false);
                if ok {
                    let token = p
                        .get("access_token")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    Some(Event::Paired { token })
                } else {
                    Some(Event::Sys {
                        sid,
                        text: "配对失败:网关拒绝了本次配对".into(),
                    })
                }
            }
            "assistant_chunk" => {
                let content = p
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                Some(Event::MessageDelta { sid, text: content })
            }
            "assistant_final" => {
                let content = p
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                Some(Event::MessageFinal { sid, text: content })
            }
            "tool_call" => {
                let name = p
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let args = p.get("arguments").cloned().unwrap_or(Value::Null);
                let meta = summary(&args, 96);
                let call = if request_id.is_empty() {
                    format!("auto-{}", auto_call)
                } else {
                    request_id
                };
                Some(Event::ToolCall {
                    sid,
                    call,
                    tool: name,
                    meta,
                })
            }
            "tool_result" => {
                let ok = p.get("ok").and_then(Value::as_bool).unwrap_or(true);
                let meta = if ok {
                    let r = p.get("result").cloned().unwrap_or(Value::Null);
                    summary(&r, 96)
                } else {
                    let e = p
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("执行失败");
                    format!("失败:{}", e)
                };
                Some(Event::ToolResult {
                    sid,
                    call: request_id,
                    ok,
                    meta,
                    diff: None,
                })
            }
            "approval_request" => {
                let action = p
                    .get("action")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let reason = p
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                Some(Event::ApprovalRequest {
                    sid,
                    request_id: request_id,
                    action,
                    reason,
                })
            }
            "error" => {
                let msg = p
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("未知错误");
                Some(Event::Sys {
                    sid,
                    text: format!("网关错误:{}", msg),
                })
            }
            _ => None,
        }
    }
}
