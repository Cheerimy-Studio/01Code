use super::{InMsg, Transport};
use crate::{
    events::{Event, Role},
    settings::{http_client, Preferences},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, SyncSender},
    Arc,
};
use std::time::Duration;

pub struct Direct {
    prefs: Preferences,
    tx: SyncSender<(u64, Event)>,
    rx: Receiver<(u64, Event)>,
    generation: u64,
    cancel: Arc<AtomicBool>,
    approval: Option<Sender<(String, bool)>>,
    done: Option<Receiver<()>>,
}
impl Direct {
    pub fn new(prefs: Preferences) -> Self {
        let (tx, rx) = mpsc::sync_channel(256);
        Self {
            prefs,
            tx,
            rx,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            approval: None,
            done: None,
        }
    }
}
impl Drop for Direct {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}
impl Transport for Direct {
    fn shutdown(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.approval = None;
        self.rx.try_iter().for_each(drop);
        if let Some(done) = self.done.take() {
            let _ = done.recv_timeout(Duration::from_secs(2));
        }
    }
    fn poll(&mut self) -> Vec<Event> {
        self.rx
            .try_iter()
            .filter(|(g, _)| *g == self.generation)
            .map(|(_, e)| e)
            .collect()
    }
    fn send(&mut self, msg: InMsg) {
        match msg {
            InMsg::Cancel => {
                self.cancel.store(true, Ordering::SeqCst);
                self.generation += 1;
                self.approval = None;
            }
            InMsg::Approval {
                request_id,
                approved,
                ..
            } => {
                if let Some(tx) = &self.approval {
                    let _ = tx.send((request_id, approved));
                }
            }
            InMsg::UserText {
                sid,
                text: _,
                history,
                content,
            } => {
                self.cancel.store(true, Ordering::SeqCst);
                self.generation += 1;
                let cancel = Arc::new(AtomicBool::new(false));
                self.cancel = cancel.clone();
                let prefs = self.prefs.clone();
                let tx = self.tx.clone();
                let generation = self.generation;
                let (atx, arx) = mpsc::channel();
                self.approval = Some(atx);
                let (done_tx, done_rx) = mpsc::channel();
                self.done = Some(done_rx);
                std::thread::spawn(move || {
                    let emit = |e| {
                        if !cancel.load(Ordering::SeqCst) {
                            let _ = tx.send((generation, e));
                        }
                    };
                    if let Err(e) = run(&prefs, &sid, content, history, &cancel, &arx, &emit) {
                        emit(Event::Sys {
                            sid: sid.clone(),
                            text: e,
                        });
                    }
                    emit(Event::SessionDone {
                        sid,
                        cost: String::new(),
                        ms: 0,
                    });
                    let _ = done_tx.send(());
                });
            }
        }
    }
}
#[derive(Default, Debug)]
struct ToolCall {
    id: String,
    name: String,
    args: String,
}
#[derive(Default)]
struct Completion {
    text: String,
    reasoning: String,
    calls: BTreeMap<usize, ToolCall>,
    finish: Option<String>,
}
impl Completion {
    fn delta(&mut self, v: &Value, sid: &str, emit: &impl Fn(Event)) {
        if let Some(usage) = v.get("usage") {
            if let Some(tokens) = usage.get("total_tokens").and_then(Value::as_f64) {
                emit(Event::CtxUsage {
                    sid: sid.into(),
                    used: tokens as f32,
                    limit: 0.0,
                });
            }
        }
        let Some(choice) = v.get("choices").and_then(|v| v.get(0)) else {
            return;
        };
        if let Some(f) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish = Some(f.into());
        }
        let d = choice
            .get("delta")
            .or_else(|| choice.get("message"))
            .unwrap_or(&Value::Null);
        if let Some(s) = d.get("content").and_then(Value::as_str) {
            self.text.push_str(s);
            emit(Event::MessageDelta {
                sid: sid.into(),
                text: s.into(),
            });
        }
        if let Some(s) = d
            .get("reasoning_content")
            .or_else(|| d.get("reasoning"))
            .and_then(Value::as_str)
        {
            self.reasoning.push_str(s);
            emit(Event::ReasoningDelta {
                sid: sid.into(),
                text: s.into(),
            });
        }
        if let Some(calls) = d.get("tool_calls").and_then(Value::as_array) {
            for (pos, c) in calls.iter().enumerate() {
                let idx = c.get("index").and_then(Value::as_u64).unwrap_or(pos as u64) as usize;
                let t = self.calls.entry(idx).or_default();
                if let Some(id) = c.get("id").and_then(Value::as_str) {
                    t.id.push_str(id);
                }
                if let Some(name) = c.pointer("/function/name").and_then(Value::as_str) {
                    t.name.push_str(name);
                }
                if let Some(args) = c.pointer("/function/arguments").and_then(Value::as_str) {
                    t.args.push_str(args);
                }
            }
        }
    }
}
fn run(
    p: &Preferences,
    sid: &str,
    content: Value,
    history: Vec<Value>,
    cancel: &AtomicBool,
    approval: &Receiver<(String, bool)>,
    emit: &impl Fn(Event),
) -> Result<(), String> {
    let upstream = p.active();
    upstream.validate()?;
    let key = upstream.key()?;
    let client = http_client()?;
    let mut messages = vec![json!({"role":"system","content":p.prompt})];
    messages.extend(history);
    messages.push(json!({"role":"user","content":content}));
    for message in &mut messages {
        crate::storage::materialize(message)?;
    }
    if serde_json::to_vec(&messages)
        .map_err(|e| e.to_string())?
        .len()
        > 8 * 1024 * 1024
    {
        return Err("会话内容超过 8 MiB，请新建会话或减少附件".into());
    }
    for round in 0..p.max_rounds {
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        if serde_json::to_vec(&messages)
            .map_err(|e| e.to_string())?
            .len()
            > 8 * 1024 * 1024
        {
            return Err("工具上下文超过 8 MiB，请新建会话继续".into());
        }
        let mut body = json!({"model":upstream.model.trim(),"messages":messages,"stream":true});
        if p.agent {
            body["tools"] = tool_schemas();
        }
        let mut req = client
            .post(upstream.endpoint("chat/completions"))
            .json(&body);
        if !key.is_empty() {
            req = req.bearer_auth(&key);
        }
        let response = req
            .send()
            .map_err(|_| "上游请求失败或超时，请检查网络、地址和 API Key".to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "上游返回 HTTP {}；请检查模型、权限与配额",
                response.status()
            ));
        }
        emit(Event::MessageBegin {
            sid: sid.into(),
            role: Role::Assistant,
        });
        let is_json = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|x| x.to_str().ok())
            .is_some_and(|s| s.contains("application/json"));
        let mut result = Completion::default();
        if is_json {
            let mut bytes = Vec::new();
            response
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "读取上游响应失败")?;
            if bytes.len() > 8 * 1024 * 1024 {
                return Err("上游响应超过 8 MiB".into());
            }
            let v: Value = serde_json::from_slice(&bytes).map_err(|_| "上游返回无效 JSON")?;
            result.delta(&v, sid, emit);
            if result.finish.is_none() {
                return Err("上游未返回完成状态".into());
            }
        } else {
            let mut reader = BufReader::new(response);
            let mut data = String::new();
            let mut count = 0;
            let mut complete = false;
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let mut line = String::new();
                let n = reader
                    .by_ref()
                    .take(1024 * 1024 + 1)
                    .read_line(&mut line)
                    .map_err(|_| "流式连接中断，请重试")?;
                if n > 1024 * 1024 {
                    return Err("流式事件过大".into());
                }
                count += n;
                if count > 8 * 1024 * 1024 {
                    return Err("本轮响应超过 8 MiB".into());
                }
                if n == 0 {
                    if !data.is_empty() {
                        process_data(&data, &mut result, sid, emit)?;
                    }
                    if result.finish.is_some() {
                        complete = true;
                    }
                    break;
                }
                let line = line.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    if data.trim() == "[DONE]" {
                        complete = true;
                        break;
                    }
                    if !data.is_empty() {
                        process_data(&data, &mut result, sid, emit)?;
                        data.clear();
                    }
                } else if let Some(value) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(value.strip_prefix(' ').unwrap_or(value));
                }
            }
            if !complete {
                return Err("流式响应提前结束，已保留收到的内容".into());
            }
        }
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        emit(Event::MessageEnd { sid: sid.into() });
        if result.calls.len() > 16 || result.calls.values().any(|c| c.args.len() > 512 * 1024) {
            return Err("工具请求数量或参数大小超过限制，已阻止执行".into());
        }
        if result.calls.is_empty() {
            if result.text.trim().is_empty() {
                return Err("模型没有返回正文，请尝试其他模型或重试".into());
            }
            if result.finish.as_deref() == Some("length") {
                return Err("输出达到上游长度限制，正文已保留".into());
            }
            messages.push(json!({"role":"assistant","content":result.text}));
            emit(Event::ContextSnapshot {
                sid: sid.into(),
                messages: messages.iter().skip(1).cloned().collect(),
            });
            return Ok(());
        }
        if !p.agent {
            return Err("对话模式不执行工具，请切换 Agent 模式".into());
        }
        if result.finish.as_deref() != Some("tool_calls") {
            return Err("工具请求未正常结束，已阻止执行，请重试".into());
        }
        let mut assistant = json!({"role":"assistant","content":result.text,"tool_calls":[]});
        if !result.reasoning.is_empty() {
            assistant["reasoning_content"] = json!(result.reasoning);
        }
        let mut calls = Vec::new();
        for (index, mut call) in result.calls {
            if call.id.is_empty() {
                call.id = format!("call-{round}-{index}");
            }
            calls.push(call);
        }
        assistant["tool_calls"]=Value::Array(calls.iter().map(|c|json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.args}})).collect());
        messages.push(assistant);
        for call in calls {
            if cancel.load(Ordering::SeqCst) {
                return Ok(());
            }
            emit(Event::ToolCall {
                sid: sid.into(),
                call: call.id.clone(),
                tool: call.name.clone(),
                meta: call.args.clone(),
            });
            let mut diff = None;
            let outcome = (|| {
                let args: Value = serde_json::from_str(&call.args)
                    .map_err(|_| "工具参数不是有效 JSON".to_string())?;
                if matches!(
                    call.name.as_str(),
                    "write_file" | "edit_file" | "run_command"
                ) {
                    let rid = format!("{}-{}", sid, call.id);
                    emit(Event::ApprovalRequest {
                        sid: sid.into(),
                        request_id: rid.clone(),
                        action: format!("{} {}", call.name, call.args),
                        reason: if call.name == "run_command" {
                            "命令将以当前用户权限执行，可访问工作区外资源。请检查后批准。".into()
                        } else {
                            "将覆盖目标文件，请确认路径与完整内容。".into()
                        },
                    });
                    loop {
                        if cancel.load(Ordering::SeqCst) {
                            return Err("已停止".into());
                        }
                        match approval.recv_timeout(Duration::from_millis(100)) {
                            Ok((id, yes)) if id == rid => {
                                if !yes {
                                    return Err("用户拒绝执行；不要重复申请相同操作".into());
                                }
                                break;
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                return Err("审批已取消".into())
                            }
                            _ => {}
                        }
                    }
                }
                let before = if matches!(call.name.as_str(), "write_file" | "edit_file") {
                    scoped(
                        &p.workspace,
                        args.get("path").and_then(Value::as_str).unwrap_or(""),
                        true,
                    )
                    .ok()
                    .and_then(|path| {
                        let file = std::fs::File::open(path).ok()?;
                        let mut text = String::new();
                        file.take(256 * 1024 + 1).read_to_string(&mut text).ok()?;
                        if text.len() > 256 * 1024 {
                            None
                        } else {
                            Some(text)
                        }
                    })
                    .unwrap_or_default()
                } else {
                    String::new()
                };
                let result = if call.name == "set_plan" {
                    let todo = parse_plan(&args)?;
                    emit(Event::StateChange {
                        sid: sid.into(),
                        todo,
                    });
                    Ok("计划已更新".into())
                } else {
                    execute(&p.workspace, &call.name, &args, cancel)
                };
                if result.is_ok() && matches!(call.name.as_str(), "write_file" | "edit_file") {
                    let after = if call.name == "edit_file" {
                        scoped(
                            &p.workspace,
                            args.get("path").and_then(Value::as_str).unwrap_or(""),
                            false,
                        )
                        .ok()
                        .and_then(|p| std::fs::read_to_string(p).ok())
                        .unwrap_or_default()
                    } else {
                        args.get("content")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .into()
                    };
                    diff = Some(change_diff(&before, &after));
                }
                result
            })();
            let ok = outcome.is_ok();
            let output = outcome.unwrap_or_else(|e| format!("错误：{e}"));
            emit(Event::ToolResult {
                sid: sid.into(),
                call: call.id.clone(),
                ok,
                meta: output.clone(),
                diff,
            });
            messages.push(json!({"role":"tool","tool_call_id":call.id,"content":output}));
        }
        emit(Event::ContextSnapshot {
            sid: sid.into(),
            messages: messages.iter().skip(1).cloned().collect(),
        });
    }
    Err(format!(
        "已达到 {} 轮工具调用上限，请检查轨迹后继续",
        p.max_rounds
    ))
}
fn parse_plan(args: &Value) -> Result<Vec<crate::events::TodoItem>, String> {
    use crate::events::{TodoItem, TodoState};
    let steps = args
        .get("steps")
        .and_then(Value::as_array)
        .ok_or("计划缺少步骤列表")?;
    if steps.len() > 16 {
        return Err("计划不能超过 16 步".into());
    }
    steps
        .iter()
        .map(|step| {
            let title = step
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty() && s.chars().count() <= 200)
                .ok_or("步骤标题须为 1–200 个字符")?;
            let state = match step.get("status").and_then(Value::as_str) {
                Some("pending") => TodoState::Pending,
                Some("in_progress") => TodoState::Running,
                Some("completed") => TodoState::Done,
                _ => return Err("计划状态无效".into()),
            };
            Ok(TodoItem {
                title: title.into(),
                state,
            })
        })
        .collect()
}
fn change_diff(before: &str, after: &str) -> crate::events::Diff {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let limit = |lines: &[&str]| {
        let mut out: Vec<String> = lines.iter().take(200).map(|s| s.to_string()).collect();
        if lines.len() > 200 {
            out.push("…变更预览已截断，请检查文件".into());
        }
        out
    };
    crate::events::Diff {
        removed: limit(&a[prefix..a.len() - suffix]),
        added: limit(&b[prefix..b.len() - suffix]),
    }
}
fn process_data(
    data: &str,
    result: &mut Completion,
    sid: &str,
    emit: &impl Fn(Event),
) -> Result<(), String> {
    if data.trim() == "[DONE]" {
        return Ok(());
    }
    let value: Value = serde_json::from_str(data).map_err(|_| "上游返回无效流式事件")?;
    if value.get("error").is_some() {
        return Err("上游在生成过程中返回错误".into());
    }
    result.delta(&value, sid, emit);
    Ok(())
}
fn tool_schemas() -> Value {
    json!([
        {"type":"function","function":{"name":"set_plan","description":"更新任务计划。最多 16 步；使用 pending/in_progress/completed 表示真实进度","parameters":{"type":"object","properties":{"steps":{"type":"array","maxItems":16,"items":{"type":"object","properties":{"title":{"type":"string"},"status":{"type":"string","enum":["pending","in_progress","completed"]}},"required":["title","status"]}}},"required":["steps"]}}},
        {"type":"function","function":{"name":"list_directory","description":"列出工作区目录，不递归","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}},
        {"type":"function","function":{"name":"read_file","description":"读取工作区 UTF-8 文件，最大 256 KiB","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}},
        {"type":"function","function":{"name":"write_file","description":"经批准写入工作区文件，提供完整内容；父目录必须存在","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}}},
        {"type":"function","function":{"name":"edit_file","description":"经批准精确替换文件中唯一一处文本；旧文本必须完整匹配","parameters":{"type":"object","properties":{"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"}},"required":["path","old_text","new_text"]}}},
        {"type":"function","function":{"name":"search_text","description":"在工作区搜索字面文本，跳过依赖与构建目录，结果最多 100 条","parameters":{"type":"object","properties":{"path":{"type":"string"},"query":{"type":"string"}},"required":["query"]}}},
        {"type":"function","function":{"name":"run_command","description":"经批准在工作区执行 shell 命令，最长 60 秒；不是沙箱","parameters":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}}}
    ])
}
fn scoped(root: &str, path: &str, write: bool) -> Result<std::path::PathBuf, String> {
    let root = std::fs::canonicalize(root).map_err(|_| "工作区不存在")?;
    let target = root.join(path);
    let resolved = if write && !target.exists() {
        let parent = target
            .parent()
            .ok_or("文件路径无效")?
            .canonicalize()
            .map_err(|_| "父目录不存在")?;
        parent.join(target.file_name().ok_or("文件名无效")?)
    } else {
        target.canonicalize().map_err(|_| "路径不存在")?
    };
    if !resolved.starts_with(&root) {
        return Err("路径超出工作区".into());
    }
    for part in resolved
        .strip_prefix(&root)
        .map_err(|_| "路径无效")?
        .components()
    {
        let name = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        if matches!(
            name.as_str(),
            ".git" | ".ssh" | ".aws" | ".gnupg" | ".nullclaw" | ".config"
        ) || name.starts_with(".env")
        {
            return Err("该路径包含受保护的配置或凭据".into());
        }
    }
    Ok(resolved)
}
fn execute(root: &str, name: &str, args: &Value, cancel: &AtomicBool) -> Result<String, String> {
    let field = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("缺少参数 {key}"))
    };
    if cancel.load(Ordering::SeqCst) {
        return Err("已停止".into());
    }
    match name {
        "list_directory" => {
            let path = scoped(root, field("path")?, false)?;
            let mut names = Vec::new();
            for item in std::fs::read_dir(path)
                .map_err(|e| e.to_string())?
                .take(201)
            {
                let entry = item.map_err(|e| e.to_string())?;
                names.push(format!(
                    "{}{}",
                    entry.file_name().to_string_lossy(),
                    if entry.path().is_dir() { "/" } else { "" }
                ));
            }
            names.sort();
            if names.len() > 200 {
                names.truncate(200);
                names.push("…目录已截断".into());
            }
            Ok(names.join("\n"))
        }
        "read_file" => {
            let path = scoped(root, field("path")?, false)?;
            let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            file.take(256 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 256 * 1024 {
                return Err("文件超过 256 KiB，请使用命令分段读取".into());
            }
            String::from_utf8(bytes).map_err(|_| "文件不是 UTF-8 文本".into())
        }
        "write_file" => {
            let path = scoped(root, field("path")?, true)?;
            if std::fs::metadata(&path)
                .map(|m| m.len() > 256 * 1024)
                .unwrap_or(false)
            {
                return Err("目标文件超过 256 KiB，请分段处理".into());
            }
            let content = field("content")?;
            if content.len() > 256 * 1024 {
                return Err("文件超过 256 KiB".into());
            }
            let permissions = std::fs::metadata(&path).ok().map(|m| m.permissions());
            use std::io::Write;
            let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or("路径无效")?)
                .map_err(|e| e.to_string())?;
            if let Some(perms) = permissions {
                temp.as_file()
                    .set_permissions(perms)
                    .map_err(|e| e.to_string())?;
            }
            temp.write_all(content.as_bytes())
                .map_err(|e| e.to_string())?;
            temp.as_file().sync_all().map_err(|e| e.to_string())?;
            if cancel.load(Ordering::SeqCst) {
                return Err("已停止".into());
            }
            temp.persist(path).map_err(|e| e.to_string())?;
            Ok(format!("已写入 {} 字节", content.len()))
        }
        "edit_file" => {
            let path = scoped(root, field("path")?, false)?;
            let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            file.take(256 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 256 * 1024 {
                return Err("目标文件超过 256 KiB".into());
            }
            let before = String::from_utf8(bytes).map_err(|_| "文件不是 UTF-8 文本")?;
            let old = field("old_text")?;
            let new = field("new_text")?;
            if old.is_empty() || before.matches(old).count() != 1 {
                return Err("旧文本必须在文件中恰好匹配一次，未修改文件".into());
            }
            let content = before.replacen(old, new, 1);
            execute(
                root,
                "write_file",
                &json!({"path":field("path")?,"content":content}),
                cancel,
            )
        }
        "search_text" => search_text(root, args, cancel),
        "run_command" => run_command(root, field("command")?, cancel),
        _ => Err(format!("不支持的工具：{name}")),
    }
}
fn search_text(root: &str, args: &Value, cancel: &AtomicBool) -> Result<String, String> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("搜索内容不能为空")?;
    let start = scoped(
        root,
        args.get("path").and_then(Value::as_str).unwrap_or("."),
        false,
    )?;
    let base = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let mut stack = vec![start];
    let mut hits = Vec::new();
    let mut scanned = 0usize;
    let mut bytes_read = 0u64;
    while let Some(path) = stack.pop() {
        if cancel.load(Ordering::SeqCst) {
            return Err("已停止".into());
        }
        if scanned >= 500 || hits.len() >= 100 || bytes_read >= 16 * 1024 * 1024 {
            break;
        }
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.is_symlink() {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path != base
            && (name.starts_with('.')
                || matches!(
                    name.as_ref(),
                    "node_modules" | "target" | "vendor" | "dist" | "build"
                ))
        {
            continue;
        }
        if meta.is_dir() {
            for item in std::fs::read_dir(path)
                .map_err(|e| e.to_string())?
                .take(500)
                .flatten()
            {
                stack.push(item.path());
            }
            continue;
        }
        scanned += 1;
        if !meta.is_file() || meta.len() > 256 * 1024 {
            continue;
        }
        bytes_read += meta.len();
        let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let mut text = String::new();
        if file.take(256 * 1024 + 1).read_to_string(&mut text).is_err() {
            continue;
        }
        for (i, line) in text.lines().enumerate() {
            if line.contains(query) {
                hits.push(format!(
                    "{}:{}: {}",
                    path.strip_prefix(&base)
                        .map_err(|_| "路径超出工作区")?
                        .display(),
                    i + 1,
                    line.chars().take(240).collect::<String>()
                ));
                if hits.len() >= 100 {
                    break;
                }
            }
        }
    }
    Ok(format!(
        "已搜索 {scanned} 个文件；有界搜索最多 500 个文件 / 16 MiB / 100 条结果。\n{}",
        if hits.is_empty() {
            "未找到匹配".into()
        } else {
            hits.join("\n")
        }
    ))
}

fn run_command(root: &str, command: &str, cancel: &AtomicBool) -> Result<String, String> {
    use std::process::Command;
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };
    cmd.current_dir(root);
    run_process(cmd, cancel, Duration::from_secs(60))
}
pub(crate) fn run_process(
    mut cmd: std::process::Command,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<String, String> {
    use std::process::Stdio;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = crate::process_tree::ProcessTree::spawn(&mut cmd).map_err(|e| e.to_string())?;
    let out = child.child.stdout.take().ok_or("无法获取命令输出")?;
    let err = child.child.stderr.take().ok_or("无法获取命令错误输出")?;
    let drain = |mut stream: Box<dyn Read + Send>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let room = (32 * 1024usize).saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
            let _ = tx.send(String::from_utf8_lossy(&kept).into_owned());
        });
        rx
    };
    let stdout = drain(Box::new(out));
    let stderr = drain(Box::new(err));
    let start = std::time::Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) || start.elapsed() > timeout {
            child.terminate();
            return Err(format!("命令已停止或超过 {} 秒", timeout.as_secs()));
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            child.terminate();
            // Descendants may retain pipes. Do not block the UI worker waiting for them.
            let a = stdout
                .recv_timeout(Duration::from_millis(300))
                .unwrap_or_else(|_| "输出仍被子进程占用".into());
            let b = stderr
                .recv_timeout(Duration::from_millis(300))
                .unwrap_or_default();
            let result = format!("exit={}\n{}\n{}", status, a, b);
            return if status.success() {
                Ok(result)
            } else {
                Err(result)
            };
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streamed_tool_arguments_and_unicode() {
        let mut c = Completion::default();
        c.delta(&json!({"choices":[{"delta":{"content":"你好","tool_calls":[{"index":0,"id":"c1","function":{"name":"read_file","arguments":"{\"path\":"}}]}}]}),"s",&|_|{});
        c.delta(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.rs\"}"}}]},"finish_reason":"tool_calls"}]}),"s",&|_|{});
        assert_eq!(c.text, "你好");
        assert_eq!(c.calls[&0].args, "{\"path\":\"a.rs\"}");
        assert_eq!(c.finish.as_deref(), Some("tool_calls"));
    }
    #[test]
    fn tools_cannot_escape_workspace() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&root).unwrap();
        assert!(scoped(root.to_str().unwrap(), "../escape", true).is_err());
        std::fs::write(root.join(".env"), "secret").unwrap();
        assert!(scoped(root.to_str().unwrap(), ".env", false).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", root.join("link")).unwrap();
            assert!(scoped(root.to_str().unwrap(), "link/passwd", false).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    fn read_request(stream: &mut std::net::TcpStream) -> Value {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.starts_with("POST /v1/chat/completions "));
        let mut len = 0;
        loop {
            line.clear();
            r.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                if k.eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse::<usize>().unwrap();
                }
            }
        }
        let mut data = vec![0; len];
        r.read_exact(&mut data).unwrap();
        serde_json::from_slice(&data).unwrap()
    }
    fn reply(stream: &mut std::net::TcpStream, value: Value) {
        let text = format!("data: {}\r\n\r\ndata: [DONE]\r\n\r\n", value);
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",text.len(),text).unwrap();
    }
    fn write_round_trip(approve: bool) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&root).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let body = read_request(&mut stream);
            assert!(body["tools"].is_array());
            reply(
                &mut stream,
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"write1","function":{"name":"write_file","arguments":"{\"path\":\"a.txt\",\"content\":\"你好\"}"}}]},"finish_reason":"tool_calls"}]}),
            );
            let (mut stream, _) = listener.accept().unwrap();
            let body = read_request(&mut stream);
            assert_eq!(body["messages"][3]["tool_call_id"], "write1");
            assert!(body["messages"][3]["content"]
                .as_str()
                .unwrap()
                .contains(if approve { "已写入" } else { "用户拒绝" }));
            reply(
                &mut stream,
                json!({"choices":[{"delta":{"content":"已完成"},"finish_reason":"stop"}]}),
            );
        });
        let mut p = Preferences {
            agent: true,
            workspace: root.to_string_lossy().into(),
            ..Preferences::default()
        };
        p.upstreams[0].base_url = format!("http://{address}/v1");
        let (tx, rx) = mpsc::channel();
        let approved = std::cell::Cell::new(false);
        let result = run(
            &p,
            "s",
            json!("写文件"),
            Vec::new(),
            &AtomicBool::new(false),
            &rx,
            &|e| {
                if let Event::ApprovalRequest { request_id, .. } = e {
                    approved.set(true);
                    tx.send((request_id, approve)).unwrap();
                }
            },
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(approved.get());
        if approve {
            assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "你好");
        } else {
            assert!(!root.join("a.txt").exists());
        }
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn agent_executes_approved_write_then_continues_with_tool_result() {
        write_round_trip(true);
    }
    #[test]
    fn denied_write_never_changes_file() {
        write_round_trip(false);
    }
    #[test]
    fn plan_tool_emits_progress_and_returns_result_to_model() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            reply(
                &mut stream,
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"plan1","function":{"name":"set_plan","arguments":json!({"steps":[{"title":"检查项目","status":"in_progress"}]}).to_string()}}]},"finish_reason":"tool_calls"}]}),
            );
            let (mut stream, _) = listener.accept().unwrap();
            let body = read_request(&mut stream);
            assert_eq!(body["messages"][3]["tool_call_id"], "plan1");
            assert_eq!(body["messages"][3]["content"], "计划已更新");
            reply(
                &mut stream,
                json!({"choices":[{"delta":{"content":"继续检查"},"finish_reason":"stop"}]}),
            );
        });
        let mut prefs = Preferences {
            agent: true,
            ..Preferences::default()
        };
        prefs.upstreams[0].base_url = format!("http://{address}/v1");
        let (_tx, rx) = mpsc::channel();
        let plan = std::cell::Cell::new(false);
        run(
            &prefs,
            "s",
            json!("检查"),
            Vec::new(),
            &AtomicBool::new(false),
            &rx,
            &|event| match event {
                Event::StateChange { todo, .. } => {
                    assert_eq!(todo[0].title, "检查项目");
                    plan.set(true);
                }
                Event::ApprovalRequest { .. } => {
                    panic!("planning must not request filesystem approval")
                }
                _ => {}
            },
        )
        .unwrap();
        assert!(plan.get());
        server.join().unwrap();
    }
    #[test]
    fn malformed_sse_does_not_execute_tools() {
        let mut completion = Completion::default();
        assert!(process_data("not-json", &mut completion, "s", &|_| {}).is_err());
        assert!(process_data("{\"error\":{}}", &mut completion, "s", &|_| {}).is_err());
    }
    #[test]
    fn changed_lines_exclude_common_prefix_and_suffix() {
        let d = change_diff("a\nb\nc\n", "a\n替换\nc\n");
        assert_eq!(d.removed, vec!["b"]);
        assert_eq!(d.added, vec!["替换"]);
    }
    #[test]
    fn shell_captures_output_and_nonzero_exit() {
        #[cfg(unix)]
        {
            let root = std::env::temp_dir();
            let out = run_command(
                root.to_str().unwrap(),
                "printf hello",
                &AtomicBool::new(false),
            )
            .unwrap();
            assert!(out.contains("hello"));
            assert!(
                run_command(root.to_str().unwrap(), "exit 7", &AtomicBool::new(false)).is_err()
            );
        }
    }
}

#[cfg(test)]
mod editing_tests {
    use super::*;
    #[test]
    fn exact_edit_rejects_ambiguous_match_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "one one").unwrap();
        let root = dir.path().to_str().unwrap();
        let cancel = AtomicBool::new(false);
        assert!(execute(
            root,
            "edit_file",
            &json!({"path":"a.txt","old_text":"one","new_text":"two"}),
            &cancel
        )
        .is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "one one"
        );
        execute(
            root,
            "edit_file",
            &json!({"path":"a.txt","old_text":"one one","new_text":"唯一替换"}),
            &cancel,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "唯一替换"
        );
    }
    #[test]
    fn search_skips_dependencies_and_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(dir.path().join("node_modules/dep.txt"), "needle").unwrap();
        std::fs::write(dir.path().join("a.rs"), "needle").unwrap();
        let result = search_text(
            dir.path().to_str().unwrap(),
            &json!({"query":"needle"}),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(result.contains("a.rs:1"));
        assert!(!result.contains("dep.txt"));
    }
    #[test]
    fn subprocess_timeout_returns_promptly() {
        #[cfg(unix)]
        {
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "sleep 10"]);
            let start = std::time::Instant::now();
            assert!(
                run_process(command, &AtomicBool::new(false), Duration::from_millis(80)).is_err()
            );
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;
    #[test]
    fn plan_limits_and_states_are_validated() {
        let steps=parse_plan(&json!({"steps":[{"title":"检查","status":"completed"},{"title":"修复","status":"in_progress"}]})).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].state, crate::events::TodoState::Running);
        for args in [
            json!({"steps":[{"title":" ","status":"pending"}]}),
            json!({"steps":[{"title":"a","status":"invented"}]}),
            json!({"steps":vec![json!({"title":"a","status":"pending"});17]}),
        ] {
            assert!(parse_plan(&args).is_err());
        }
        assert!(parse_plan(&json!({"steps":[]})).unwrap().is_empty());
    }
    #[test]
    #[cfg(unix)]
    fn completed_shell_does_not_leave_background_writer() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new("sh");
        command
            .current_dir(dir.path())
            .args(["-c", "(sleep 0.2; printf orphan > marker) & printf done"]);
        let out = run_process(command, &AtomicBool::new(false), Duration::from_secs(2)).unwrap();
        assert!(out.contains("done"));
        std::thread::sleep(Duration::from_millis(300));
        assert!(!dir.path().join("marker").exists());
    }
}
