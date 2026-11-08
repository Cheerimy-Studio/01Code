//! UI 状态模型 — 事件归约
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, TextureHandle};

use crate::events::{Diff, Event, Role, TodoItem};
use crate::settings::SettingsState;
use crate::transport::InMsg;
use crate::util::now_hhmm;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Modal {
    /// 设置(模型 API / Web 渠道)
    Settings,
    /// 只读信息(扩展能力 / 定时任务)
    Info,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolState {
    Pending,
    Running,
    Ok,
    Err,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolNode {
    pub call: String,
    pub tool: String,
    pub meta: String,
    pub state: ToolState,
    #[serde(default)]
    pub input: String,
    #[serde(skip)]
    pub started: Option<std::time::Instant>,
    #[serde(default)]
    pub elapsed_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Stream {
    pub text: String,
    pub done: bool,
    pub time: String,
    /// 已接收字符数；保留以兼容已有历史格式。
    pub typed: usize,
}

#[derive(Clone)]
pub struct ImageAttachment {
    pub texture: TextureHandle,
    pub name: String,
}

#[derive(Clone)]
pub struct PendingAttach {
    pub path: PathBuf,
    pub name: String,
    pub texture: Option<TextureHandle>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct UserMsg {
    pub text: String,
    pub time: String,
    #[serde(skip)]
    pub images: Vec<ImageAttachment>,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(skip)]
    images_restored: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Approval {
    pub request_id: String,
    pub action: String,
    pub reason: String,
    pub responded: Option<bool>,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Item {
    User(UserMsg),
    Ai(Stream),
    Reasoning(String),
    Tool(ToolNode),
    Diff(Diff),
    Approval(Approval),
    Sys(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionContext {
    mode: String,
    workspace: String,
    gateway: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub sid: String,
    pub title: String,
    pub workspace: String,
    #[serde(default)]
    pub context: Option<SessionContext>,
    pub created_at: u64,
    pub done: bool,
}

pub struct Model {
    pub sessions: Vec<Session>,
    pub active_sid: String,
    pub transcripts: HashMap<String, Vec<Item>>,
    api_history: HashMap<String, Vec<Value>>,
    pub todo: Vec<TodoItem>,
    plans: HashMap<String, Vec<TodoItem>>,
    pub ctx_used: f32,
    pub ctx_limit: f32,
    pub model_name: String,
    pub input: String,
    pub composing: bool,
    pub search: String,
    pub trace_view: bool,
    pub demo: bool,
    pub gateway: crate::gateway::Gateway,
    pub client_updater: crate::updater::Updater,
    pub nullclaw_updater: crate::updater::Updater,
    pub exit_for_update: bool,
    pub history_blocked: bool,
    pub gateway_connected: bool,
    pub connection_status: String,
    /// 真实网关模式:一条连接绑定一个会话(default),新会话=重置当前会话
    pub gateway_mode: bool,
    /// 侧栏搜索框展开状态
    pub show_search: bool,
    /// 最近一次完成的执行用时(秒)
    pub last_elapsed: Option<u64>,
    /// 当前弹窗
    pub modal: Option<Modal>,
    pub settings: SettingsState,
    pub runtime: crate::settings::Preferences,
    pub runtime_connection: crate::config::ClientCfg,
    pub info_title: String,
    pub info_text: String,
    info_rx: Option<std::sync::mpsc::Receiver<String>>,
    /// 设置保存后请求重连网关
    pub request_reconnect: bool,
    /// 待发送附件(文件/图片,发送后进入用户消息)
    pub pending: Vec<PendingAttach>,
    outbox: Vec<InMsg>,
    streaming: bool,
    running: usize,
    busy_since: Option<std::time::Instant>,
    last_event_at: Option<std::time::Instant>,
    auto_call: u64,
}

impl Model {
    /// seed_demo=false 时连接真实网关,侧栏从空开始
    pub fn new(seed_demo: bool) -> Self {
        let now = chrono::Local::now().timestamp().max(0) as u64;
        let mut settings = SettingsState::load();
        // 强制经由 NullClaw 网关运行:禁用内置直连 Agent 与直连模式
        if settings.prefs.mode != "nullclaw" || settings.prefs.agent {
            settings.prefs.mode = "nullclaw".into();
            settings.prefs.agent = false;
            let _ = settings.prefs.save();
        }
        let model_name = if settings.prefs.active().model.is_empty() {
            "deepseek-chat".to_string()
        } else {
            settings.prefs.active().model.clone()
        };
        let mut m = Self {
            sessions: Vec::new(),
            active_sid: String::new(),
            transcripts: HashMap::new(),
            api_history: HashMap::new(),
            todo: Vec::new(),
            plans: HashMap::new(),
            ctx_used: 0.0,
            ctx_limit: 200_000.0,
            model_name,
            input: String::new(),
            composing: false,
            search: String::new(),
            trace_view: false,
            demo: seed_demo,
            gateway: crate::gateway::Gateway::default(),
            client_updater: crate::updater::Updater::new(crate::updater::Product::Client),
            nullclaw_updater: crate::updater::Updater::new(crate::updater::Product::NullClaw),
            exit_for_update: false,
            history_blocked: false,
            gateway_connected: false,
            connection_status: "未连接".into(),
            gateway_mode: !seed_demo && settings.prefs.mode == "nullclaw",
            show_search: false,
            last_elapsed: None,
            modal: None,
            runtime: settings.prefs.clone(),
            runtime_connection: settings.connection.clone(),
            settings,
            info_title: String::new(),
            info_text: String::new(),
            info_rx: None,
            request_reconnect: false,
            pending: Vec::new(),
            outbox: Vec::new(),
            streaming: false,
            running: 0,
            busy_since: None,
            last_event_at: None,
            auto_call: 1,
        };
        if seed_demo {
            m.sessions = vec![
                Session {
                    sid: "a1f2".into(),
                    title: "修复购物车金额精度 #482".into(),
                    workspace: "demo-shop".into(),
                    context: None,
                    created_at: now - 240,
                    done: false,
                },
                Session {
                    sid: "s-auth".into(),
                    title: "重构鉴权中间件".into(),
                    workspace: "demo-shop".into(),
                    context: None,
                    created_at: now - 5 * 3600,
                    done: true,
                },
                Session {
                    sid: "s-doc".into(),
                    title: "生成 API 文档".into(),
                    workspace: "docs-site".into(),
                    context: None,
                    created_at: now - 3600,
                    done: true,
                },
                Session {
                    sid: "s-week".into(),
                    title: "周报数据汇总".into(),
                    workspace: "ops".into(),
                    context: None,
                    created_at: now - 9 * 3600,
                    done: true,
                },
            ];
            m.transcripts.insert("a1f2".to_string(), Vec::new());
            m.transcripts.insert(
                "s-auth".to_string(),
                static_transcript(
                    "src/auth/middleware.ts",
                    "已重构为可组合的中间件链,全量回归通过。",
                ),
            );
            m.transcripts.insert(
                "s-doc".to_string(),
                static_transcript(
                    "docs/api.md",
                    "已按 OpenAPI 规范生成接口文档,覆盖 32 个端点。",
                ),
            );
            m.transcripts.insert(
                "s-week".to_string(),
                static_transcript("data/weekly.xlsx", "已汇总本周提交与缺陷数据,报告已导出。"),
            );
            m.active_sid = "a1f2".into();
        } else {
            // 网关模式:单会话,sid 与网关连接的会话一致
            m.sessions = vec![Session {
                sid: format!("ui-{}", uuid::Uuid::new_v4().simple()),
                title: "新会话".into(),
                workspace: "默认工作区".into(),
                context: None,
                created_at: now,
                done: false,
            }];
            m.active_sid = m.sessions[0].sid.clone();
            m.sessions[0].workspace = m.workspace_label();
            m.sessions[0].context = Some(m.current_context());
            m.transcripts.insert(m.active_sid.clone(), Vec::new());
        }
        if !seed_demo {
            m.restore_history();
        }
        m
    }

    pub fn apply_saved_settings(&mut self) {
        let changed = self.demo
            || self.runtime.mode != self.settings.prefs.mode
            || self.runtime.workspace != self.settings.prefs.workspace;
        if self.demo {
            self.sessions.clear();
            self.transcripts.clear();
            self.api_history.clear();
            self.plans.clear();
            self.active_sid.clear();
        }
        self.demo = false;
        self.runtime = self.settings.prefs.clone();
        self.runtime_connection = self.settings.connection.clone();
        self.gateway_mode = self.runtime.mode == "nullclaw";
        self.model_name = self.runtime.active().model.clone();
        if changed {
            let draft = std::mem::take(&mut self.input);
            let pending = std::mem::take(&mut self.pending);
            let old = self.active_sid.clone();
            if self.transcripts.get(&old).is_some_and(Vec::is_empty) {
                self.transcripts.remove(&old);
                self.sessions.retain(|s| s.sid != old);
                self.api_history.remove(&old);
                self.plans.remove(&old);
            }
            self.new_task();
            self.input = draft;
            self.pending = pending;
        }
        self.request_reconnect = true;
        self.save_history();
    }
    fn current_context(&self) -> SessionContext {
        SessionContext {
            mode: self.runtime.mode.clone(),
            workspace: self.runtime.workspace.clone(),
            gateway: if self.gateway_mode {
                self.runtime_connection.ws_url.clone()
            } else {
                String::new()
            },
        }
    }
    pub fn active_session(&self) -> Option<&Session> {
        self.sessions.iter().find(|s| s.sid == self.active_sid)
    }

    pub fn transcript(&self) -> &[Item] {
        self.transcripts
            .get(&self.active_sid)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn steps(&self) -> usize {
        self.transcript()
            .iter()
            .filter(|i| matches!(i, Item::Tool(_)))
            .count()
    }

    /// 会话按工作区分组(保持出现顺序)
    pub fn groups(&self) -> Vec<(String, Vec<usize>)> {
        let mut order: Vec<String> = Vec::new();
        let mut map: HashMap<String, Vec<usize>> = HashMap::new();
        let filter = self.search.to_lowercase();
        for (i, s) in self.sessions.iter().enumerate() {
            if !filter.is_empty() && !s.title.to_lowercase().contains(&filter) {
                continue;
            }
            if !map.contains_key(&s.workspace) {
                order.push(s.workspace.clone());
            }
            map.entry(s.workspace.clone()).or_default().push(i);
        }
        order
            .into_iter()
            .filter_map(|k| map.remove(&k).map(|v| (k, v)))
            .collect()
    }

    fn touch_busy(&mut self) {
        if self.is_busy() && self.busy_since.is_none() {
            self.busy_since = Some(std::time::Instant::now());
        } else if !self.is_busy() {
            self.busy_since = None;
        }
    }

    /// 忙碌结束时记录本次执行耗时
    fn capture_elapsed(&mut self) {
        if self.running == 0 {
            if let Some(t) = self.busy_since {
                let s = t.elapsed().as_secs();
                if s > 0 {
                    self.last_elapsed = Some(s);
                }
            }
        }
    }

    pub fn waiting_approval(&self) -> bool {
        self.transcript()
            .iter()
            .any(|i| matches!(i,Item::Approval(a) if a.responded.is_none()))
    }
    pub fn busy_secs(&self) -> u64 {
        self.busy_since.map(|t| t.elapsed().as_secs()).unwrap_or(0)
    }

    pub fn ensure_session(&mut self, sid: &str) {
        if self.transcripts.contains_key(sid) {
            return;
        }
        if self.sessions.iter().all(|s| s.sid != sid) {
            self.sessions.insert(
                0,
                Session {
                    sid: sid.to_string(),
                    title: "新会话".into(),
                    workspace: self.workspace_label(),
                    context: None,
                    created_at: chrono::Local::now().timestamp().max(0) as u64,
                    done: false,
                },
            );
        }
        self.transcripts.entry(sid.to_string()).or_default();
    }

    pub fn new_task(&mut self) {
        if self.is_busy() {
            return;
        }
        self.input.clear();
        self.pending.clear();
        self.last_elapsed = None;
        self.ctx_used = 0.0;
        self.ctx_limit = 0.0;
        if self.gateway_mode {
            self.request_reconnect = true;
        }
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let _ = ms;
        let sid = format!("ui-{}", uuid::Uuid::new_v4().simple());
        let title = "新会话".to_string();
        self.sessions.insert(
            0,
            Session {
                sid: sid.clone(),
                title,
                workspace: self.workspace_label(),
                context: Some(self.current_context()),
                created_at: chrono::Local::now().timestamp().max(0) as u64,
                done: false,
            },
        );
        self.transcripts.insert(sid.clone(), Vec::new());
        self.active_sid = sid;
        self.todo.clear();
        self.save_history();
    }

    pub fn apply(&mut self, ev: Event) {
        self.last_event_at = Some(std::time::Instant::now());
        let save = matches!(
            &ev,
            Event::MessageFinal { .. } | Event::SessionDone { .. } | Event::Sys { .. }
        );
        match ev {
            Event::ConnectionStatus { ready, text } => {
                self.gateway_connected = ready;
                self.connection_status = text;
            }
            Event::ContextSnapshot { sid, messages } => {
                let mut packed = Value::Array(messages);
                if let Err(e) = crate::storage::pack(&mut packed) {
                    self.settings.status = format!("附件保存失败：{e}");
                }
                if let Value::Array(messages) = packed {
                    self.api_history.insert(sid, messages);
                }
            }
            Event::SessionStart {
                sid,
                model,
                workspace,
            } => {
                let existed = self.sessions.iter().any(|s| s.sid == sid);
                if !existed {
                    self.sessions.insert(
                        0,
                        Session {
                            sid: sid.clone(),
                            title: "新会话".into(),
                            workspace,
                            context: None,
                            created_at: chrono::Local::now().timestamp().max(0) as u64,
                            done: false,
                        },
                    );
                }
                self.transcripts.entry(sid.clone()).or_default();
                self.active_sid = sid;
                if !model.is_empty() {
                    self.model_name = model;
                }
                self.todo.clear();
                self.running = 0;
                self.streaming = false;
                self.touch_busy();
            }
            Event::MessageBegin { sid, role } => {
                self.ensure_session(&sid);
                let list = self.transcripts.entry(sid).or_default();
                match role {
                    Role::Assistant => {
                        list.push(Item::Ai(Stream {
                            text: String::new(),
                            done: false,
                            time: now_hhmm(),
                            typed: 0,
                        }));
                    }
                    Role::User => {
                        list.push(Item::User(UserMsg {
                            text: String::new(),
                            time: now_hhmm(),
                            images: Vec::new(),
                            content: None,
                            images_restored: true,
                        }));
                    }
                }
                self.streaming = true;
                self.touch_busy();
            }
            Event::ReasoningDelta { sid, text } => {
                self.ensure_session(&sid);
                let list = self.transcripts.entry(sid).or_default();
                if let Some(Item::Reasoning(s)) = list.last_mut() {
                    s.push_str(&text);
                } else {
                    list.push(Item::Reasoning(text));
                }
            }
            Event::MessageDelta { sid, text } => {
                self.ensure_session(&sid);
                if let Some(list) = self.transcripts.get_mut(&sid) {
                    if !list.iter().any(|i| matches!(i, Item::Ai(s) if !s.done)) {
                        list.push(Item::Ai(Stream {
                            text: String::new(),
                            done: false,
                            time: now_hhmm(),
                            typed: 0,
                        }));
                    }
                    for item in list.iter_mut().rev() {
                        match item {
                            Item::Ai(st) if !st.done => {
                                st.text.push_str(&text);
                                st.typed += text.chars().count();
                                break;
                            }
                            Item::User(u) if u.text.is_empty() => {
                                u.text.push_str(&text);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                self.touch_busy();
            }
            Event::MessageEnd { sid } => {
                if let Some(list) = self.transcripts.get_mut(&sid) {
                    for item in list.iter_mut().rev() {
                        if let Item::Ai(st) = item {
                            if !st.done {
                                st.done = true;
                                st.typed = st.text.chars().count();
                                break;
                            }
                        }
                    }
                }
                self.touch_busy();
            }
            Event::MessageFinal { sid, text } => {
                self.ensure_session(&sid);
                if let Some(list) = self.transcripts.get_mut(&sid) {
                    let mut replaced = false;
                    for item in list.iter_mut().rev() {
                        if let Item::Ai(st) = item {
                            if !st.done {
                                // Empty terminal frames must preserve already-received chunks.
                                if !text.is_empty() || st.text.is_empty() {
                                    st.text = text.clone();
                                }
                                st.done = true;
                                st.typed = st.text.chars().count();
                                replaced = true;
                                break;
                            }
                        }
                    }
                    if !replaced {
                        let typed = text.chars().count();
                        list.push(Item::Ai(Stream {
                            text,
                            done: true,
                            time: now_hhmm(),
                            typed,
                        }));
                    }
                }
                self.capture_elapsed();
                self.finish_open();
                self.touch_busy();
            }
            Event::ToolCall {
                sid,
                call,
                tool,
                meta,
            } => {
                self.ensure_session(&sid);
                self.transcripts
                    .entry(sid)
                    .or_default()
                    .push(Item::Tool(ToolNode {
                        call,
                        tool,
                        input: meta.clone(),
                        meta,
                        state: ToolState::Running,
                        started: Some(std::time::Instant::now()),
                        elapsed_ms: 0,
                    }));
                self.running += 1;
                self.touch_busy();
            }
            Event::ToolResult {
                sid,
                call,
                ok,
                meta,
                diff,
            } => {
                if let Some(list) = self.transcripts.get_mut(&sid) {
                    let mut hit = false;
                    if !call.is_empty() {
                        for item in list.iter_mut().rev() {
                            if let Item::Tool(node) = item {
                                if node.call == call && node.state == ToolState::Running {
                                    node.state = if ok { ToolState::Ok } else { ToolState::Err };
                                    node.meta = meta.clone();
                                    node.elapsed_ms = node
                                        .started
                                        .map(|t| t.elapsed().as_millis() as u64)
                                        .unwrap_or(0);
                                    hit = true;
                                    break;
                                }
                            }
                        }
                    }
                    if !hit && call.is_empty() {
                        // Only legacy events without an ID may match the latest running tool.
                        for item in list.iter_mut().rev() {
                            if let Item::Tool(node) = item {
                                if node.state == ToolState::Running {
                                    node.state = if ok { ToolState::Ok } else { ToolState::Err };
                                    node.meta = meta.clone();
                                    node.elapsed_ms = node
                                        .started
                                        .map(|t| t.elapsed().as_millis() as u64)
                                        .unwrap_or(0);
                                    break;
                                }
                            }
                        }
                    }
                    if let Some(d) = diff {
                        list.push(Item::Diff(d));
                    }
                }
                self.running = self
                    .transcripts
                    .values()
                    .flat_map(|l| l.iter())
                    .filter(|i| matches!(i,Item::Tool(n) if n.state==ToolState::Running))
                    .count();
                self.touch_busy();
            }
            Event::StateChange { sid, todo } => {
                if sid == self.active_sid {
                    self.todo = todo.clone();
                }
                self.plans.insert(sid, todo);
            }
            Event::CtxUsage { used, limit, .. } => {
                self.ctx_used = used;
                if limit > 0.0 {
                    self.ctx_limit = limit;
                } else {
                    self.ctx_limit = 0.0;
                }
            }
            Event::SessionDone { sid, .. } => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.sid == sid) {
                    s.done = true;
                }
                self.capture_elapsed();
                self.finish_open();
                self.touch_busy();
            }
            Event::ApprovalRequest {
                sid,
                request_id,
                action,
                reason,
            } => {
                self.ensure_session(&sid);
                self.transcripts
                    .entry(sid)
                    .or_default()
                    .push(Item::Approval(Approval {
                        request_id,
                        action,
                        reason,
                        responded: None,
                    }));
                self.touch_busy();
            }
            Event::Paired { .. } => {}
            Event::Sys { sid, text } => {
                self.capture_elapsed();
                self.finish_open();
                self.touch_busy();
                // 连接级消息(sid 为空)路由到当前会话;无会话则先建一个
                let target = if sid.is_empty() {
                    if self.active_sid.is_empty() {
                        self.new_task();
                    }
                    self.active_sid.clone()
                } else {
                    self.ensure_session(&sid);
                    sid
                };
                self.transcripts
                    .entry(target)
                    .or_default()
                    .push(Item::Sys(text));
            }
        }
        if save {
            self.save_history();
        }
    }

    pub fn is_busy(&self) -> bool {
        self.streaming || self.running > 0
    }

    /// 看门狗:忙碌但长时间无事件 → 判定应答超时并复位
    pub fn watchdog_tick(&mut self) {
        if !self.is_busy() {
            return;
        }
        let Some(last) = self.last_event_at else {
            return;
        };
        let awaiting_approval = self
            .transcript()
            .iter()
            .any(|i| matches!(i,Item::Approval(a) if a.responded.is_none()));
        if awaiting_approval {
            return;
        }
        if last.elapsed().as_secs() > 150 {
            self.stop();
            let sid = if self.active_sid.is_empty() {
                "default".to_string()
            } else {
                self.active_sid.clone()
            };
            self.transcripts
                .entry(sid)
                .or_default()
                .push(Item::Sys("响应超时,请重试".into()));
        }
    }

    pub fn user_submit(&mut self) {
        if self.is_busy() {
            return;
        }
        if self.input.trim().is_empty() && self.pending.is_empty() {
            return;
        }
        if !self.demo {
            let context = self.current_context();
            let needs_new = self.active_session().is_some_and(|session| {
                session.context.as_ref().map_or(
                    self.runtime.agent && !self.gateway_mode && !self.transcript().is_empty(),
                    |previous| previous != &context,
                )
            });
            if needs_new {
                let draft = std::mem::take(&mut self.input);
                let pending = std::mem::take(&mut self.pending);
                self.new_task();
                self.input = draft;
                self.pending = pending;
                self.transcripts
                    .entry(self.active_sid.clone())
                    .or_default()
                    .push(Item::Sys(
                        "已为当前项目和运行模式新建会话，原会话仍保留。".into(),
                    ));
            }
            if let Some(session) = self.sessions.iter_mut().find(|s| s.sid == self.active_sid) {
                session.context = Some(context);
            }
            self.request_reconnect = true;
        }
        if !self.demo && self.runtime.mode == "direct" {
            if let Err(text) = self.runtime.active().validate() {
                self.apply(Event::Sys {
                    sid: self.active_sid.clone(),
                    text,
                });
                return;
            }
            self.request_reconnect = true;
        }
        if self.gateway_mode && !self.pending.is_empty() {
            let local = reqwest::Url::parse(&self.runtime_connection.ws_url)
                .ok()
                .is_some_and(|u| {
                    matches!(
                        u.host_str(),
                        Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
                    )
                });
            if !local {
                self.apply(Event::Sys{sid:self.active_sid.clone(),text:"远程 NullClaw 暂不支持上传本机附件；请使用直连模式，或在消息中提供网关可访问的路径。".into()});
                return;
            }
        }
        let display = self.input.trim().to_string();
        // 附件:文件路径随消息带给 agent,图片同时进入气泡展示
        let mut sent = display.clone();
        let mut images = Vec::new();
        for att in &self.pending {
            sent.push_str(&format!("\n@{}", att.path.display()));
            if let Some(tex) = &att.texture {
                images.push(ImageAttachment {
                    texture: tex.clone(),
                    name: att.name.clone(),
                });
            }
        }
        if display.is_empty() && self.pending.is_empty() {
            return;
        }
        if self.active_sid.is_empty() {
            self.new_task();
        }
        let sid0 = self.active_sid.clone();
        self.ensure_session(&sid0);
        let sid = self.active_sid.clone();
        let mut content = json!(sent);
        if !self.gateway_mode && !self.demo {
            let mut parts = vec![json!({"type":"text","text":display})];
            let mut total = 0usize;
            for att in &self.pending {
                let bytes = match std::fs::File::open(&att.path).and_then(|f| {
                    use std::io::Read;
                    let mut bytes = Vec::new();
                    f.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                    Ok(bytes)
                }) {
                    Ok(b) => b,
                    Err(e) => {
                        self.apply(Event::Sys {
                            sid: sid.clone(),
                            text: format!("附件读取失败：{e}"),
                        });
                        return;
                    }
                };
                total += bytes.len();
                if total > 4 * 1024 * 1024 {
                    self.apply(Event::Sys {
                        sid: sid.clone(),
                        text: "附件总大小不能超过 4 MiB".into(),
                    });
                    return;
                }
                if att.texture.is_some() {
                    use base64::Engine;
                    let mime = if bytes.starts_with(b"\x89PNG") {
                        "image/png"
                    } else {
                        "image/jpeg"
                    };
                    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    parts.push(json!({"type":"image_url","image_url":{"url":format!("data:{mime};base64,{data}")}}));
                } else {
                    let text = match String::from_utf8(bytes) {
                        Ok(t) => t,
                        Err(_) => {
                            self.apply(Event::Sys {
                                sid: sid.clone(),
                                text: "附件须为 UTF-8 文本、PNG 或 JPEG 图片".into(),
                            });
                            return;
                        }
                    };
                    if text.len() > 128 * 1024 {
                        self.apply(Event::Sys {
                            sid: sid.clone(),
                            text: "单个文本附件不能超过 128 KiB".into(),
                        });
                        return;
                    }
                    parts.push(json!({"type":"text","text":format!("以下是附件数据 {}：\n{}",att.name,text)}));
                }
            }
            content = if parts.len() == 1 {
                json!(display)
            } else {
                json!(parts)
            };
        }
        let fallback: Vec<Value> = self.transcript().iter().filter_map(|i| match i {
            Item::User(u)=>Some(json!({"role":"user","content":u.content.clone().unwrap_or_else(||json!(u.text))})),
            Item::Ai(a) if !a.text.is_empty()=>Some(json!({"role":"assistant","content":a.text})),
            Item::Tool(n) if n.state!=ToolState::Running=>Some(json!({"role":"assistant","content":format!("工具执行记录（数据）：{}\n参数：{}\n结果：{}",n.tool,n.input,n.meta)})),
            _=>None,
        }).collect();
        let history = self.api_history.remove(&sid).unwrap_or(fallback);
        let mut stored_content = content.clone();
        if !self.demo {
            if let Err(e) = crate::storage::pack(&mut stored_content) {
                self.settings.status = format!("附件保存失败：{e}");
            }
        }
        let time = now_hhmm();
        self.transcripts
            .entry(sid.clone())
            .or_default()
            .push(Item::User(UserMsg {
                text: if self.pending.is_empty() {
                    display.clone()
                } else {
                    format!(
                        "{}\n{}",
                        display,
                        self.pending
                            .iter()
                            .map(|a| a.name.clone())
                            .collect::<Vec<_>>()
                            .join(" · ")
                    )
                },
                time,
                images,
                content: Some(stored_content),
                images_restored: true,
            }));
        if let Some(session) = self.sessions.iter_mut().find(|s| s.sid == sid) {
            if session.title == "新会话" {
                session.title = if display.is_empty() {
                    "附件分析".into()
                } else {
                    display.chars().take(36).collect()
                };
            }
            session.done = false;
        }
        self.outbox.push(InMsg::UserText {
            sid,
            text: sent,
            history,
            content,
        });
        self.streaming = true;
        self.last_elapsed = None;
        self.last_event_at = Some(std::time::Instant::now());
        self.touch_busy();
        self.save_history();
        self.input.clear();
        self.pending.clear();
    }

    /// 添加附件(图片会解码为缩略图;失败则以纯文件路径附加)
    pub fn add_attachment(&mut self, ctx: &egui::Context, path: PathBuf) {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if self.pending.len() >= 4
            || std::fs::metadata(&path)
                .map(|m| m.len() > 4 * 1024 * 1024)
                .unwrap_or(true)
        {
            self.apply(Event::Sys {
                sid: self.active_sid.clone(),
                text: "最多 4 个附件，每个不超过 4 MiB".into(),
            });
            return;
        }
        let mut texture = None;
        if matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
            if let Ok(bytes) = std::fs::read(&path) {
                texture = thumbnail_texture(ctx, &bytes, &name);
            }
        }
        self.pending.push(PendingAttach {
            path,
            name,
            texture,
        });
    }

    pub fn restore_image_previews(&mut self, ctx: &egui::Context) {
        let Some(items) = self.transcripts.get_mut(&self.active_sid) else {
            return;
        };
        for item in items {
            if let Item::User(u) = item {
                if u.images_restored {
                    continue;
                }
                u.images_restored = true;
                if let Some(parts) = u.content.as_ref().and_then(Value::as_array) {
                    for (idx, part) in parts.iter().enumerate() {
                        if let Some(url) = part.pointer("/image_url/url").and_then(Value::as_str) {
                            if let Ok(bytes) = crate::storage::image_bytes(url) {
                                if let Some(texture) =
                                    thumbnail_texture(ctx, &bytes, &format!("history-{idx}"))
                                {
                                    u.images.push(ImageAttachment {
                                        texture,
                                        name: "图片附件".into(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn remove_attachment(&mut self, idx: usize) {
        if idx < self.pending.len() {
            self.pending.remove(idx);
        }
    }

    pub fn respond_approval(&mut self, request_id: &str, approved: bool) {
        let sid = self.active_sid.clone();
        let mut found = false;
        if let Some(list) = self.transcripts.get_mut(&sid) {
            for item in list.iter_mut() {
                if let Item::Approval(a) = item {
                    if a.request_id == request_id && a.responded.is_none() {
                        a.responded = Some(approved);
                        found = true;
                        break;
                    }
                }
            }
        }
        if !found {
            return;
        }
        self.last_event_at = Some(std::time::Instant::now());
        self.outbox.push(InMsg::Approval {
            sid,
            request_id: request_id.to_string(),
            approved,
        });
    }

    fn finish_open(&mut self) {
        self.streaming = false;
        self.running = 0;
        for list in self.transcripts.values_mut() {
            for item in list {
                match item {
                    Item::Ai(s) => {
                        s.done = true;
                        s.typed = s.text.chars().count();
                    }
                    Item::Tool(n) if n.state == ToolState::Running => n.state = ToolState::Err,
                    Item::Approval(a) if a.responded.is_none() => a.responded = Some(false),
                    _ => {}
                }
            }
        }
    }
    pub fn stop(&mut self) {
        if self.gateway_mode {
            self.gateway_connected = false;
            self.connection_status = "已停止接收".into();
        }
        self.api_history.remove(&self.active_sid);
        self.outbox.push(InMsg::Cancel);
        self.capture_elapsed();
        self.finish_open();
        self.touch_busy();
        self.transcripts
            .entry(self.active_sid.clone())
            .or_default()
            .push(Item::Sys(
                if self.gateway_mode {
                    "已断开接收；NullClaw 后端任务可能继续执行，请在网关端确认。"
                } else {
                    "已停止生成；已经完成的文件修改仍保留。"
                }
                .into(),
            ));
        self.save_history();
    }
    fn retained_sessions(&self) -> std::collections::HashSet<String> {
        let mut ids = std::collections::HashSet::new();
        ids.insert(self.active_sid.clone());
        for session in &self.sessions {
            if ids.len() >= self.runtime.history_sessions.clamp(5, 200) {
                break;
            }
            ids.insert(session.sid.clone());
        }
        ids
    }
    pub fn repair_history(&mut self) -> Result<(), String> {
        let path = crate::config::app_dir().join("history.json");
        let mut source = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let mut backup =
            tempfile::NamedTempFile::new_in(crate::config::app_dir()).map_err(|e| e.to_string())?;
        std::io::copy(&mut source, backup.as_file_mut()).map_err(|e| e.to_string())?;
        backup.as_file().sync_all().map_err(|e| e.to_string())?;
        backup
            .persist(path.with_file_name("history-recovery.json"))
            .map_err(|e| e.to_string())?;
        self.history_blocked = false;
        if self.save_history() {
            Ok(())
        } else {
            self.history_blocked = true;
            Err(self.settings.status.clone())
        }
    }
    pub fn cleanup_storage(&mut self) -> String {
        if self.history_blocked {
            return "会话索引尚未恢复，为保护附件暂不清理".into();
        }
        if !self.save_history() {
            return "会话尚未成功保存，为保护原记录暂不清理附件".into();
        }
        self.client_updater.staged = None;
        self.nullclaw_updater.staged = None;
        let data = json!({"transcripts":self.transcripts,"api_history":self.api_history});
        crate::storage::cleanup(&data).unwrap_or_else(|e| format!("清理失败：{e}"))
    }
    pub fn save_history(&mut self) -> bool {
        self.persist_history(&crate::config::app_dir().join("history.json"))
    }
    fn persist_history(&mut self, path: &std::path::Path) -> bool {
        if self.demo {
            return true;
        }
        if self.history_blocked {
            return false;
        }
        #[derive(Serialize)]
        struct Snapshot<'a> {
            sessions: Vec<&'a Session>,
            transcripts: HashMap<&'a str, &'a Vec<Item>>,
            api_history: HashMap<&'a str, &'a Vec<Value>>,
            plans: HashMap<&'a str, &'a Vec<TodoItem>>,
            active: &'a str,
        }
        let ids = self.retained_sessions();
        let snapshot = Snapshot {
            sessions: self
                .sessions
                .iter()
                .filter(|s| ids.contains(&s.sid))
                .collect(),
            transcripts: self
                .transcripts
                .iter()
                .filter(|(id, _)| ids.contains(*id))
                .map(|(id, v)| (id.as_str(), v))
                .collect(),
            api_history: self
                .api_history
                .iter()
                .filter(|(id, _)| ids.contains(*id))
                .map(|(id, v)| (id.as_str(), v))
                .collect(),
            plans: self
                .plans
                .iter()
                .filter(|(id, _)| ids.contains(*id))
                .map(|(id, v)| (id.as_str(), v))
                .collect(),
            active: &self.active_sid,
        };
        let result = crate::config::bounded_json(&snapshot, 20 * 1024 * 1024)
            .and_then(|b| crate::config::write_private(path, &b).map_err(|e| e.to_string()));
        match result {
            Ok(()) => {
                self.sessions.retain(|s| ids.contains(&s.sid));
                self.transcripts.retain(|id, _| ids.contains(id));
                self.api_history.retain(|id, _| ids.contains(id));
                self.plans.retain(|id, _| ids.contains(id));
                if self.settings.status.starts_with("会话保存失败") {
                    self.settings.status.clear();
                }
                true
            }
            Err(e) => {
                self.settings.status = format!("会话保存失败：{e}");
                false
            }
        }
    }
    fn workspace_label(&self) -> String {
        if self.gateway_mode {
            return "NullClaw".into();
        }
        std::path::Path::new(&self.runtime.workspace)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "默认工作区".into())
    }
    fn restore_history(&mut self) {
        let path = crate::config::app_dir().join("history.json");
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 20 * 1024 * 1024) {
            self.history_blocked = true;
            self.settings.status =
                "会话文件超过 20 MiB，已暂停自动保存；可在存储设置中备份并重建".into();
            return;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let Ok(data) = serde_json::from_slice::<Value>(&bytes) else {
            self.history_blocked = true;
            self.settings.status =
                "会话记录无法解析，已暂停自动保存；可在存储设置中备份并重建".into();
            return;
        };
        let Some(sessions) = data
            .get("sessions")
            .and_then(|v| serde_json::from_value::<Vec<Session>>(v.clone()).ok())
        else {
            self.history_blocked = true;
            self.settings.status = "会话索引格式错误，已暂停自动保存".into();
            return;
        };
        let Some(transcripts) = data
            .get("transcripts")
            .and_then(|v| serde_json::from_value::<HashMap<String, Vec<Item>>>(v.clone()).ok())
        else {
            self.history_blocked = true;
            self.settings.status = "会话正文格式错误，已暂停自动保存".into();
            return;
        };
        if sessions.is_empty() {
            return;
        }
        self.sessions = sessions;
        self.transcripts = transcripts;
        self.plans = data
            .get("plans")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        self.api_history = data
            .get("api_history")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        self.active_sid = data
            .get("active")
            .and_then(Value::as_str)
            .filter(|s| self.sessions.iter().any(|x| x.sid == *s))
            .unwrap_or(&self.sessions[0].sid)
            .into();
        self.todo = self
            .plans
            .get(&self.active_sid)
            .cloned()
            .unwrap_or_default();
        self.finish_open();
    }

    pub fn load_info(&mut self, args: Vec<String>) {
        if self.info_loading() {
            return;
        }
        let Some(bin) = self.settings.nullclaw.clone() else {
            self.info_text =
                "未找到 NullClaw。直连 Agent 可在设置中启用；网关扩展与定时任务需要安装 NullClaw。"
                    .into();
            return;
        };
        self.info_text = "正在读取…".into();
        let (tx, rx) = std::sync::mpsc::channel();
        self.info_rx = Some(rx);
        std::thread::spawn(move || {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let _ = tx.send(crate::settings::run_cli(&bin, &refs));
        });
    }
    pub fn info_loading(&self) -> bool {
        self.info_rx.is_some()
    }
    pub fn poll_info(&mut self) {
        if let Some(rx) = &self.info_rx {
            if let Ok(text) = rx.try_recv() {
                self.info_text = text;
                self.info_rx = None;
            }
        }
    }
    pub fn export_markdown(&self, sid: &str) -> String {
        let title = self
            .sessions
            .iter()
            .find(|s| s.sid == sid)
            .map(|s| s.title.as_str())
            .unwrap_or("会话");
        let mut text = format!("# {title}\n\n");
        for item in self.transcripts.get(sid).into_iter().flatten() {
            match item {
                Item::User(u) => text.push_str(&format!("## 用户\n\n{}\n\n", u.text)),
                Item::Ai(a) => text.push_str(&format!("## 助手\n\n{}\n\n", a.text)),
                Item::Tool(n) => text.push_str(&format!(
                    "### 工具：{} ({:?})\n\n参数：\n\n{}\n\n结果：\n\n{}\n\n",
                    n.tool, n.state, n.input, n.meta
                )),
                Item::Sys(s) => text.push_str(&format!("提示：{s}\n\n")),
                _ => {}
            }
        }
        text
    }
    pub fn drain_outbox(&mut self) -> Vec<InMsg> {
        std::mem::take(&mut self.outbox)
    }

    pub fn switch_session(&mut self, sid: &str) {
        if self.is_busy() || self.active_sid == sid {
            return;
        }
        if self.gateway_mode {
            self.request_reconnect = true;
        }
        if let Some(items) = self.transcripts.get_mut(&self.active_sid) {
            for item in items {
                if let Item::User(u) = item {
                    if u.content.as_ref().is_some_and(Value::is_array) {
                        u.images.clear();
                        u.images_restored = false;
                    }
                }
            }
        }
        self.active_sid = sid.to_string();
        self.ctx_used = 0.0;
        self.ctx_limit = 0.0;
        self.last_elapsed = None;
        self.todo = self.plans.get(sid).cloned().unwrap_or_default();
    }

    /// 删除会话(含记录/计划/历史),必要时切换到剩余会话
    pub fn delete_session(&mut self, sid: &str) {
        if self.is_busy() {
            return;
        }
        self.sessions.retain(|s| s.sid != sid);
        self.transcripts.remove(sid);
        self.api_history.remove(sid);
        self.plans.remove(sid);
        if self.active_sid == sid {
            if let Some(first) = self.sessions.first() {
                self.active_sid = first.sid.clone();
                self.todo = self.plans.get(&self.active_sid).cloned().unwrap_or_default();
            } else {
                self.new_task();
            }
        }
        self.save_history();
    }
}

fn thumbnail_texture(ctx: &egui::Context, bytes: &[u8], name: &str) -> Option<TextureHandle> {
    if bytes.len() > 4 * 1024 * 1024 {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader.decode().ok()?.thumbnail(512, 512).to_rgba8();
    let (w, h) = rgba.dimensions();
    let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
    Some(ctx.load_texture(
        format!("att-{name}-{}", uuid::Uuid::new_v4()),
        color,
        egui::TextureOptions::LINEAR,
    ))
}

fn static_transcript(file: &str, reply: &str) -> Vec<Item> {
    let text = format!("{} ✓", reply);
    vec![
        Item::User(UserMsg {
            text: format!("请处理 {}", file),
            time: "10:20".into(),
            images: Vec::new(),
            content: None,
            images_restored: true,
        }),
        Item::Ai(Stream {
            typed: text.chars().count(),
            text,
            done: true,
            time: "10:20".into(),
        }),
        Item::Tool(ToolNode {
            call: "st-1".into(),
            tool: "shell".into(),
            meta: "pnpm test · all passed".into(),
            state: ToolState::Ok,
            input: String::new(),
            started: None,
            elapsed_ms: 0,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> Model {
        let mut m = Model::new(true);
        m.sessions.clear();
        m.transcripts.clear();
        m.active_sid.clear();
        m.new_task();
        m
    }
    #[test]
    fn submit_is_immediately_busy_and_blocks_duplicate_sends() {
        let mut m = model();
        m.input = "你好".into();
        m.user_submit();
        assert!(m.is_busy());
        m.input = "第二条".into();
        m.user_submit();
        assert_eq!(m.drain_outbox().len(), 1);
        assert_eq!(m.input, "第二条");
        m.apply(Event::MessageBegin {
            sid: m.active_sid.clone(),
            role: Role::Assistant,
        });
        m.apply(Event::MessageEnd {
            sid: m.active_sid.clone(),
        });
        assert!(m.is_busy());
        m.apply(Event::SessionDone {
            sid: m.active_sid.clone(),
            cost: String::new(),
            ms: 0,
        });
        assert!(!m.is_busy());
    }
    #[test]
    fn missing_begin_and_empty_final_preserve_unicode_stream() {
        let mut m = model();
        let sid = m.active_sid.clone();
        m.apply(Event::MessageDelta {
            sid: sid.clone(),
            text: "你好🦀".into(),
        });
        m.apply(Event::MessageFinal {
            sid,
            text: String::new(),
        });
        assert!(
            matches!(&m.transcript()[0],Item::Ai(s) if s.text=="你好🦀" && s.done && s.typed==3)
        );
    }
    #[test]
    fn results_do_not_complete_unrelated_tools() {
        let mut m = model();
        let sid = m.active_sid.clone();
        for call in ["a", "b"] {
            m.apply(Event::ToolCall {
                sid: sid.clone(),
                call: call.into(),
                tool: "read".into(),
                meta: String::new(),
            });
        }
        for call in ["a", "a", "unknown"] {
            m.apply(Event::ToolResult {
                sid: sid.clone(),
                call: call.into(),
                ok: true,
                meta: "ok".into(),
                diff: None,
            });
        }
        assert_eq!(m.running, 1);
        assert!(matches!(&m.transcript()[1],Item::Tool(n) if n.state==ToolState::Running));
    }
    #[test]
    fn approval_is_sent_once_and_wait_does_not_expire() {
        let mut m = model();
        let sid = m.active_sid.clone();
        m.streaming = true;
        m.last_event_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(1000));
        m.apply(Event::ApprovalRequest {
            sid,
            request_id: "a".into(),
            action: "write".into(),
            reason: String::new(),
        });
        m.watchdog_tick();
        assert!(m.is_busy());
        m.respond_approval("a", false);
        m.respond_approval("a", true);
        assert_eq!(m.drain_outbox().len(), 1);
        m.stop();
        assert!(!m.is_busy());
    }
    #[test]
    fn new_gateway_task_keeps_history_and_changes_session() {
        let mut m = model();
        m.gateway_mode = true;
        let old = m.active_sid.clone();
        m.transcripts
            .get_mut(&old)
            .unwrap()
            .push(Item::Sys("保留".into()));
        m.new_task();
        assert_ne!(m.active_sid, old);
        assert_eq!(m.transcripts[&old].len(), 1);
        assert!(m.request_reconnect);
    }
    #[test]
    fn non_image_attachment_can_be_sent_without_text() {
        let mut m = model();
        let path = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::write(&path, "text").unwrap();
        m.pending.push(PendingAttach {
            path: path.clone(),
            name: "a.txt".into(),
            texture: None,
        });
        m.user_submit();
        assert!(m.is_busy());
        assert_eq!(m.drain_outbox().len(), 1);
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::events::TodoState;
    fn isolated_model() -> Model {
        let mut m = Model::new(true);
        m.sessions.clear();
        m.transcripts.clear();
        m.api_history.clear();
        m.plans.clear();
        m.active_sid.clear();
        m.new_task();
        m
    }
    #[test]
    fn failed_save_preserves_sessions_until_successful_retention() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = isolated_model();
        for _ in 0..8 {
            m.new_task();
        }
        let before = m.sessions.len();
        m.runtime.history_sessions = 5;
        m.demo = false;
        assert!(!m.persist_history(dir.path()));
        assert_eq!(m.sessions.len(), before);
        assert!(m.persist_history(&dir.path().join("history.json")));
        assert_eq!(m.sessions.len(), 5);
        assert!(m.transcripts.contains_key(&m.active_sid));
        assert!(!m.settings.status.starts_with("会话保存失败"));
    }
    #[test]
    fn empty_submit_leaves_connection_untouched() {
        let mut m = isolated_model();
        m.gateway_mode = true;
        m.request_reconnect = false;
        m.input = "  ".into();
        m.user_submit();
        assert!(!m.request_reconnect);
        assert!(m.drain_outbox().is_empty());
        assert!(!m.is_busy());
    }
    #[test]
    fn changing_project_starts_fresh_context_and_preserves_draft() {
        let mut m = isolated_model();
        m.demo = false;
        m.history_blocked = true;
        let old = m.active_sid.clone();
        m.transcripts
            .get_mut(&old)
            .unwrap()
            .push(Item::Sys("old context".into()));
        m.input = "尚未发送".into();
        m.settings.prefs.workspace = "/different-project".into();
        m.apply_saved_settings();
        assert_ne!(m.active_sid, old);
        assert_eq!(m.input, "尚未发送");
        assert_eq!(m.transcripts[&old].len(), 1);
        assert!(m.transcript().is_empty());
        assert!(m.request_reconnect);
    }
    #[test]
    fn plans_follow_session_and_survive_serialization() {
        let mut m = isolated_model();
        let old = m.active_sid.clone();
        m.apply(Event::StateChange {
            sid: old.clone(),
            todo: vec![TodoItem {
                title: "验证".into(),
                state: TodoState::Running,
            }],
        });
        m.new_task();
        assert!(m.todo.is_empty());
        m.switch_session(&old);
        assert_eq!(m.todo[0].title, "验证");
        let data = serde_json::to_vec(&m.plans).unwrap();
        let restored: HashMap<String, Vec<TodoItem>> = serde_json::from_slice(&data).unwrap();
        assert_eq!(restored[&old][0].state, TodoState::Running);
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;
    #[test]
    fn sending_from_old_project_does_not_reuse_its_tool_context() {
        let mut m = Model::new(true);
        m.sessions.clear();
        m.transcripts.clear();
        m.active_sid.clear();
        m.new_task();
        let old = m.active_sid.clone();
        m.transcripts
            .get_mut(&old)
            .unwrap()
            .push(Item::Sys("old".into()));
        m.api_history.insert(
            old.clone(),
            vec![json!({"role":"user","content":"old-project"})],
        );
        m.runtime.workspace = "/different-project".into();
        m.demo = false;
        m.history_blocked = true;
        m.input = "continue".into();
        m.user_submit();
        assert_ne!(m.active_sid, old);
        assert!(m.transcripts.contains_key(&old));
        match &m.drain_outbox()[0] {
            InMsg::UserText { history, .. } => assert!(history.is_empty()),
            _ => panic!("expected user message"),
        }
        assert_eq!(m.api_history[&old][0]["content"], "old-project");
    }
    #[test]
    fn old_history_without_context_remains_readable() {
        let session: Session = serde_json::from_value(
            json!({"sid":"old","title":"old","workspace":"project","created_at":1,"done":true}),
        )
        .unwrap();
        assert!(session.context.is_none());
    }
}
