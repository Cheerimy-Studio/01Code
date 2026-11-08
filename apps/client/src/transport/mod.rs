//! 传输层抽象:内置模拟器(开发期替身)与 NullClaw WebChannel v1

pub mod direct;
pub mod sim;
pub mod ws;

use crate::events::Event;

pub enum InMsg {
    UserText {
        sid: String,
        text: String,
        history: Vec<serde_json::Value>,
        content: serde_json::Value,
    },
    Cancel,
    Approval {
        sid: String,
        request_id: String,
        approved: bool,
    },
}

pub trait Transport: Send {
    /// 非阻塞拉取一批事件
    fn poll(&mut self) -> Vec<Event>;
    /// 发送用户输入 / 审批响应
    fn send(&mut self, msg: InMsg);
    fn shutdown(&mut self) {
        self.send(InMsg::Cancel);
    }
}
