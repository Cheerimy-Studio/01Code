//! UI 状态模型 — 事件归约
#![allow(dead_code)]

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolState {
    Pending,
    Running,
    Ok,
    Err,
}

#[derive(Clone, Debug)]
pub struct ToolNode {
    pub call: String,
    pub tool: String,
    pub meta: String,
    pub state: ToolState,
}

#[derive(Clone)]
pub struct Stream {
    pub text: String,
    pub done: bool,
    pub time: String,
    /// 打字机已展示的字符数(用于单次返回的流式呈现)
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

#[derive(Clone)]
pub struct UserMsg {
    pub text: String,
    pub time: String,
    pub images: Vec<ImageAttachment>,
}

#[derive(Clone, Debug)]
pub struct Approval {
    pub request_id: String,
    pub action: String,
    pub reason: String,
    pub responded: Option<bool>,
}

#[derive(Clone)]
pub enum Item {
    User(UserMsg),
    Ai(Stream),
    Tool(ToolNode),
    Diff(Diff),
    Approval(Approval),
    Sys(String),
}

#[derive(Clone, Debug)]
pub struct Session {
    pub sid: String,
    pub title: String,
    pub workspace: String,
    pub created_at: u64,
    pub done: bool,
}

pub struct Model {
    pub sessions: Vec<Session>,
    pub active_sid: String,
    pub transcripts: HashMap<String, Vec<Item>>,
    pub todo: Vec<TodoItem>,
    pub ctx_used: f32,
    pub ctx_limit: f32,
    pub model_name: String,
    pub input: String,
    pub search: String,
    /// 真实网关模式:一条连接绑定一个会话(default),新会话=重置当前会话
    pub gateway_mode: bool,
    /// 侧栏搜索框展开状态
    pub show_search: bool,
    /// 最近一次完成的执行用时(秒)
    pub last_elapsed: Option<u64>,
    /// 当前弹窗
    pub modal: Option<Modal>,
    pub settings: SettingsState,
    pub info_title: String,
    pub info_text: String,
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
        let settings = SettingsState::load();
        let model_name = if settings.model.is_empty() {
            "deepseek-chat".to_string()
        } else {
            settings.model.clone()
        };
        let mut m = Self {
            sessions: Vec::new(),
            active_sid: String::new(),
            transcripts: HashMap::new(),
            todo: Vec::new(),
            ctx_used: 0.0,
            ctx_limit: 200_000.0,
            model_name,
            input: String::new(),
            search: String::new(),
            gateway_mode: !seed_demo,
            show_search: false,
            last_elapsed: None,
            modal: None,
            settings,
            info_title: String::new(),
            info_text: String::new(),
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
                Session { sid: "a1f2".into(), title: "修复购物车金额精度 #482".into(), workspace: "demo-shop".into(), created_at: now - 240, done: false },
                Session { sid: "s-auth".into(), title: "重构鉴权中间件".into(), workspace: "demo-shop".into(), created_at: now - 5 * 3600, done: true },
                Session { sid: "s-doc".into(), title: "生成 API 文档".into(), workspace: "docs-site".into(), created_at: now - 3600, done: true },
                Session { sid: "s-week".into(), title: "周报数据汇总".into(), workspace: "ops".into(), created_at: now - 9 * 3600, done: true },
            ];
            m.transcripts.insert("a1f2".to_string(), Vec::new());
            m.transcripts.insert("s-auth".to_string(), static_transcript("src/auth/middleware.ts", "已重构为可组合的中间件链,全量回归通过。"));
            m.transcripts.insert("s-doc".to_string(), static_transcript("docs/api.md", "已按 OpenAPI 规范生成接口文档,覆盖 32 个端点。"));
            m.transcripts.insert("s-week".to_string(), static_transcript("data/weekly.xlsx", "已汇总本周提交与缺陷数据,报告已导出。"));
            m.active_sid = "a1f2".into();
        } else {
            // 网关模式:单会话,sid 与网关连接的会话一致
            m.sessions = vec![Session {
                sid: "default".into(),
                title: "新会话".into(),
                workspace: "默认工作区".into(),
                created_at: now,
                done: false,
            }];
            m.transcripts.insert("default".to_string(), Vec::new());
            m.active_sid = "default".into();
        }
        m
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

    pub fn busy_secs(&self) -> u64 {
        self.busy_since
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0)
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
                    workspace: "默认工作区".into(),
                    created_at: chrono::Local::now().timestamp().max(0) as u64,
                    done: false,
                },
            );
        }
        self.transcripts.entry(sid.to_string()).or_default();
    }

    pub fn new_task(&mut self) {
        // 网关模式:连接绑定单会话,重置当前会话而非新建 sid
        if self.gateway_mode {
            if let Some(list) = self.transcripts.get_mut("default") {
                list.clear();
            }
            if let Some(s) = self.sessions.iter_mut().find(|s| s.sid == "default") {
                s.done = false;
                s.created_at = chrono::Local::now().timestamp().max(0) as u64;
            }
            self.active_sid = "default".into();
            self.todo.clear();
            self.input.clear();
            return;
        }
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let sid = format!("ui-{:x}", ms);
        let title = "新会话".to_string();
        self.sessions.insert(
            0,
            Session {
                sid: sid.clone(),
                title,
                workspace: "默认工作区".into(),
                created_at: chrono::Local::now().timestamp().max(0) as u64,
                done: false,
            },
        );
        self.transcripts.insert(sid.clone(), Vec::new());
        self.active_sid = sid;
        self.todo.clear();
    }

    pub fn apply(&mut self, ev: Event) {
        self.last_event_at = Some(std::time::Instant::now());
        match ev {
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
                        }));
                    }
                }
                self.streaming = true;
                self.touch_busy();
            }
            Event::MessageDelta { sid, text } => {
                self.ensure_session(&sid);
                if let Some(list) = self.transcripts.get_mut(&sid) {
                    for item in list.iter_mut().rev() {
                        match item {
                            Item::Ai(st) if !st.done => {
                                st.text.push_str(&text);
                                st.typed = st.text.chars().count();
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
                self.capture_elapsed();
                self.streaming = false;
                self.touch_busy();
            }
            Event::MessageFinal { sid, text } => {
                self.ensure_session(&sid);
                if let Some(list) = self.transcripts.get_mut(&sid) {
                    let mut replaced = false;
                    for item in list.iter_mut().rev() {
                        if let Item::Ai(st) = item {
                            if !st.done {
                                // 单次返回(无 chunk)时用打字机呈现
                                let was_empty = st.text.trim().is_empty();
                                st.text = text.clone();
                                st.done = true;
                                st.typed = if was_empty {
                                    0
                                } else {
                                    st.text.chars().count()
                                };
                                replaced = true;
                                break;
                            }
                        }
                    }
                    if !replaced {
                        list.push(Item::Ai(Stream {
                            text,
                            done: true,
                            time: now_hhmm(),
                            typed: 0,
                        }));
                    }
                }
                self.capture_elapsed();
                self.streaming = false;
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
                        meta,
                        state: ToolState::Running,
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
                                    hit = true;
                                    break;
                                }
                            }
                        }
                    }
                    if !hit {
                        // 协议未提供关联 id:回填最近一个运行中的工具
                        for item in list.iter_mut().rev() {
                            if let Item::Tool(node) = item {
                                if node.state == ToolState::Running {
                                    node.state = if ok { ToolState::Ok } else { ToolState::Err };
                                    node.meta = meta.clone();
                                    break;
                                }
                            }
                        }
                    }
                    if let Some(d) = diff {
                        list.push(Item::Diff(d));
                    }
                }
                self.running = self.running.saturating_sub(1);
                self.touch_busy();
            }
            Event::StateChange { sid: _, todo } => self.todo = todo,
            Event::CtxUsage { used, limit, .. } => {
                self.ctx_used = used;
                self.ctx_limit = limit;
            }
            Event::SessionDone { sid, .. } => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.sid == sid) {
                    s.done = true;
                }
                self.streaming = false;
                self.running = 0;
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
        if last.elapsed().as_secs() > 90 {
            self.streaming = false;
            self.running = 0;
            self.touch_busy();
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

    /// 打字机是否仍在推进
    pub fn is_typing(&self) -> bool {
        self.transcripts.values().any(|list| {
            list.iter().any(|item| {
                matches!(item, Item::Ai(s) if s.done && s.typed < s.text.chars().count())
            })
        })
    }

    /// 推进打字机(约 6 字符/次,配合 100ms 刷新)
    pub fn tick(&mut self) {
        for list in self.transcripts.values_mut() {
            for item in list.iter_mut() {
                if let Item::Ai(st) = item {
                    let len = st.text.chars().count();
                    if st.done && st.typed < len {
                        st.typed = (st.typed + 6).min(len);
                    }
                }
            }
        }
    }

    pub fn user_submit(&mut self) {
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
        if display.is_empty() && images.is_empty() {
            return;
        }
        if self.active_sid.is_empty() {
            self.new_task();
        }
        // 网关模式固定走连接绑定的会话
        if self.gateway_mode {
            self.active_sid = "default".into();
        }
        let sid0 = self.active_sid.clone();
        self.ensure_session(&sid0);
        let sid = self.active_sid.clone();
        let time = now_hhmm();
        self.transcripts
            .entry(sid.clone())
            .or_default()
            .push(Item::User(UserMsg {
                text: display,
                time,
                images,
            }));
        self.outbox.push(InMsg::UserText { sid, text: sent });
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
        let mut texture = None;
        if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif") {
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    if w > 0 && h > 0 {
                        let color = egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            rgba.as_raw(),
                        );
                        texture = Some(ctx.load_texture(
                            format!("att_{}_{}x{}", name, w, h),
                            color,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
            }
        }
        self.pending.push(PendingAttach {
            path,
            name,
            texture,
        });
    }

    pub fn remove_attachment(&mut self, idx: usize) {
        if idx < self.pending.len() {
            self.pending.remove(idx);
        }
    }

    pub fn respond_approval(&mut self, request_id: &str, approved: bool) {
        let sid = self.active_sid.clone();
        if let Some(list) = self.transcripts.get_mut(&sid) {
            for item in list.iter_mut() {
                if let Item::Approval(a) = item {
                    if a.request_id == request_id && a.responded.is_none() {
                        a.responded = Some(approved);
                        break;
                    }
                }
            }
        }
        self.outbox.push(InMsg::Approval {
            sid,
            request_id: request_id.to_string(),
            approved,
        });
    }

    pub fn drain_outbox(&mut self) -> Vec<InMsg> {
        std::mem::take(&mut self.outbox)
    }

    pub fn switch_session(&mut self, sid: &str) {
        self.active_sid = sid.to_string();
        self.todo.clear();
    }
}

fn static_transcript(file: &str, reply: &str) -> Vec<Item> {
    let text = format!("{} ✓", reply);
    vec![
        Item::User(UserMsg {
            text: format!("请处理 {}", file),
            time: "10:20".into(),
            images: Vec::new(),
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
        }),
    ]
}
