//! 内置会话模拟器 — 网关不可用时的本地演示替身(CODING01_SIM=1 强制)

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{InMsg, Transport};
use crate::events::{Diff, Event, Role, TodoItem, TodoState};

pub struct Sim {
    rx: Receiver<Event>,
    tx: Sender<Event>,
}

impl Sim {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        {
            let tx = tx.clone();
            thread::spawn(move || canned_main(tx));
        }
        Self { rx, tx }
    }
}

impl Transport for Sim {
    fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
        }
        out
    }

    fn send(&mut self, msg: InMsg) {
        match msg {
            InMsg::UserText { sid, text } => {
                let tx = self.tx.clone();
                thread::spawn(move || canned_reply(tx, sid, text));
            }
            InMsg::Approval { .. } => {}
        }
    }
}

fn stream_text(tx: &Sender<Event>, sid: &str, text: &str) {
    tx.send(Event::MessageBegin {
        sid: sid.into(),
        role: Role::Assistant,
    })
    .ok();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let end = (i + 6).min(chars.len());
        let chunk: String = chars[i..end].iter().collect();
        thread::sleep(Duration::from_millis(40));
        tx.send(Event::MessageDelta {
            sid: sid.into(),
            text: chunk,
        })
        .ok();
        i = end;
    }
    tx.send(Event::MessageEnd { sid: sid.into() }).ok();
}

fn tool(
    tx: &Sender<Event>,
    sid: &str,
    call: &str,
    tool: &str,
    meta: &str,
    ms: u64,
    diff: Option<Diff>,
) {
    tx.send(Event::ToolCall {
        sid: sid.into(),
        call: call.into(),
        tool: tool.into(),
        meta: String::new(),
    })
    .ok();
    thread::sleep(Duration::from_millis(ms));
    tx.send(Event::ToolResult {
        sid: sid.into(),
        call: call.into(),
        ok: true,
        meta: meta.into(),
        diff,
    })
    .ok();
}

fn canned_main(tx: Sender<Event>) {
    thread::sleep(Duration::from_millis(400));
    let sid = "a1f2";
    tx.send(Event::SessionStart {
        sid: sid.into(),
        model: "deepseek-chat".into(),
        workspace: "demo-shop".into(),
    })
    .ok();
    tx.send(Event::CtxUsage {
        sid: sid.into(),
        used: 41_000.0,
        limit: 200_000.0,
    })
    .ok();
    thread::sleep(Duration::from_millis(500));
    stream_text(
        &tx,
        sid,
        "已定位:cart.ts 使用浮点累加,方案:改为以「分」为单位的整数运算,展示层再转换。执行中 ↓",
    );
    tx.send(Event::StateChange {
        sid: sid.into(),
        todo: vec![
            TodoItem { title: "定位浮点累加根因".into(), state: TodoState::Done },
            TodoItem { title: "改为整数「分」运算".into(), state: TodoState::Done },
            TodoItem { title: "补充单元测试".into(), state: TodoState::Done },
            TodoItem { title: "lint 校验".into(), state: TodoState::Running },
            TodoItem { title: "生成变更说明 commit".into(), state: TodoState::Pending },
        ],
    })
    .ok();
    tool(&tx, sid, "c01", "search", "cart · 3 处命中 · 0.4s", 700, None);
    tool(&tx, sid, "c02", "read", "src/cart/cart.ts · L120-L168", 500, None);
    tool(
        &tx,
        sid,
        "c03",
        "edit",
        "cart.ts(+6 −2)",
        800,
        Some(Diff {
            removed: vec!["total += price * qty;".into()],
            added: vec![
                "const cents = Math.round(price * 100) * qty;".into(),
                "total = cents / 100;".into(),
            ],
        }),
    );
    tx.send(Event::CtxUsage {
        sid: sid.into(),
        used: 128_000.0,
        limit: 200_000.0,
    })
    .ok();
    tool(
        &tx,
        sid,
        "c04",
        "shell",
        "pnpm vitest run cart · 12 passed · 1.8s",
        1300,
        None,
    );
    tool(&tx, sid, "c05", "shell", "pnpm lint --fix · 0 warnings", 1500, None);
    stream_text(&tx, sid, "修复完成,测试全部通过;可以生成 commit 提交本次变更。");
    tx.send(Event::CtxUsage {
        sid: sid.into(),
        used: 133_000.0,
        limit: 200_000.0,
    })
    .ok();
    tx.send(Event::SessionDone {
        sid: sid.into(),
        cost: "¥0.03".into(),
        ms: 4200,
    })
    .ok();
}

fn canned_reply(tx: Sender<Event>, sid: String, text: String) {
    thread::sleep(Duration::from_millis(300));
    let short: String = text.chars().take(18).collect();
    stream_text(
        &tx,
        &sid,
        &format!("收到:「{}」。我先梳理影响面,再动手。", short),
    );
    tool(&tx, &sid, "r1", "read", "src/cart/cart.ts · 全文", 500, None);
    stream_text(&tx, &sid, "已确认相关实现,继续按计划执行中。");
    tx.send(Event::CtxUsage {
        sid,
        used: 136_000.0,
        limit: 200_000.0,
    })
    .ok();
}

#[allow(unused)]
fn _now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
