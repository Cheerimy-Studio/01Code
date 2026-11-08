### 01Code

Lite AI Agent For 言灵中转.

轻量 Rust / egui 编码客户端，支持 OpenAI 兼容上游直连和 NullClaw WebChannel。界面参考 DeepSeek Harness 的浅色工作台布局，保留 01 品牌。

## 开始使用

```bash
cd apps/client
cargo run --locked
```

1. 打开「模型」，填写上游基础地址、API Key 和模型 ID，点击「保存并应用」。默认地址为 `https://ai.gzmath.cn/v1`，模型是否可用以你的上游返回为准。
2. 「获取上游模型列表」请求该上游的 `GET /models`，支持多个上游分别保存目录，输入区可切换上游和模型。也可以手动填写模型 ID。
3. 普通对话直接发送消息。需要处理代码时，在「Agent 预设」选择项目目录，再启用输入区的 Agent。
4. Agent 提供 `list_directory`、`read_file`、`search_text`、`write_file`、`edit_file`、`run_command`、`set_plan`。写文件和执行命令均逐次审批；拒绝结果会回传模型。

API Key 可通过「Key 环境变量」指定，例如 `OPENAI_API_KEY`。环境变量优先；直接填写的 Key 保存在本机配置文件中，并非加密保险库。重定向不会自动携带凭据跳转到其他地址，远程上游须使用 HTTPS。

## 已实现

- 多上游地址、凭据、模型目录、手动模型 ID；异步获取模型列表。
- OpenAI Chat Completions SSE / JSON 响应；Unicode 流式增量、思考折叠、原生工具调用增量拼接。
- 本地 Agent 多轮工具循环，参数、结果、耗时和失败状态可展开查看；写入成功后显示变更预览。
- 发送即进入等待状态，阻止重复发送；停止、超时、断线和异常响应均有明确结果。
- 对话 / 轨迹页签、代码复制、正文选择、文件和图片附件、会话搜索、右键重命名及复制 Markdown。
- 会话与原生工具上下文、任务计划保存；新会话使用独立 ID。项目、运行模式或网关地址改变时隔离上下文并保留草稿，回看旧会话后继续发送也会检查项目关系。
- Windows / Linux / macOS 配置路径及系统中文字体回退；窗口尺寸由 eframe 处理 DPI。
- 演示模式仅在显式设置 `CODING01_SIM=1` 时启用，连接失败不会生成演示回复。

## NullClaw

可在「更新与存储」安装官方 NullClaw，或选择已有程序。在「运行模式」选择 NullClaw，并在「网关管理」同步上游、当前模型、工作区和本机连接参数，然后启动网关。客户端会从`NULLCLAW_HOME/config.json`（设置该变量时）或用户目录下的 `.nullclaw/config.json` 发现 WebChannel，兼容直接 `channels.web` 和 `channels.web.accounts.default` 两种配置位置；也可手动填写 WS 地址与令牌。

- token 模式：WebSocket 升级使用 Bearer，消息使用 `auth_token`。
- 本机配对模式：先发送 `pairing_request`，消息使用返回的 `access_token`；不把两类凭据混用。
- 每次连接通过 `session_id` 查询参数绑定当前会话；切换会话会重连，新建会话会生成新 ID。
- 远程连接须使用 WSS；仅本机 WS 允许明文。
- 设置可同步多上游及当前模型；同步 Key 需要单独勾选，并会明确提示存储方式，默认保留已有加密凭据。未知配置、其他渠道与权限设置会保留。
- 客户端管理自己启动的网关，提供启动、就绪后连接、重启、停止和有界内存日志；退出客户端会停止该自有进程。外部网关继续由原启动方管理。
- 「任务与扩展」提供诊断、能力、技能、会话历史与 Cron 创建、暂停、恢复、立即执行和删除。

**兼容边界：**核对的 NullClaw WebChannel 实现提供 `assistant_chunk` / `assistant_final`，并未保证输出工具事件或支持取消。客户端兼容适配器发送的 `tool_call` / `tool_result` / `approval_request`，但不会把模型正文中的 XML 当成已执行工具。停止 NullClaw 仅断开接收，网关任务可能继续运行。直连上游配置不会自动覆盖网关配置，也不会结束用户已有的网关进程。

## 设置内更新

「更新与存储」分别检查官方 `nullclaw/nullclaw` 与 `Cheerimy-Studio/01Code` 的 GitHub Release，选择匹配系统与架构的原始二进制，并校验大小、SHA-256 和文件格式。下载不会携带模型凭据。NullClaw 安装到客户端管理目录，不覆盖用户自定义的外部程序。

检查与下载可以取消；进入安装阶段后锁定提交，取消后的迟到结果不会安装。01Code 下载完成后点击「安装并重启」，交接进程等待原程序退出，再原子替换并启动新程序。安装保留一份旧版本，失败原因在下次启动后的更新页显示。没有 Release、版本标签不受支持或缺少摘要时不会自动安装。

`.github/workflows/release.yml` 在维护者推送与 Cargo 版本相符的 `v*` tag 后发布 Windows / Linux 二进制，资源命名与更新器一致。当前源码没有替维护者创建发布或推送 tag。

## 资源与历史

图片按内容去重保存在 `attachments/`，会话记录只保留引用；只加载当前会话的缩略图，切换后释放旧纹理。空闲与审批等待低频刷新，消息队列有背压，屏幕外稳定消息跳过布局。

会话保存成功后才裁剪内存，失败时保留现有会话；恢复备份中的图片也受清理保护。可设置保留 5–200 个最近会话（默认 50），当前会话始终保留。历史文件上限 20 MiB；损坏或过大时暂停自动保存，可在设置中先备份再重建。存储页显示分类占用，并清理未引用图片、失败下载与更新缓存。清理不会删除程序备份或正在使用的附件。

## 附件与本地执行

- 直连文本附件发送实际 UTF-8 内容；PNG / JPEG 发送图片数据，需上游模型支持视觉。最多 4 个附件，总大小 4 MiB，单个文本 128 KiB。
- NullClaw 模式发送本地文件路径，需要客户端与网关共享该文件系统；不是远程上传协议。
- 文件工具限制在选定工作区，拒绝路径穿越、指向工作区外的符号链接以及常见凭据目录；读取和写入最大 256 KiB。
- 写文件使用临时文件原子替换；变更预览最多展示各 200 行，是同一文件变更区域预览。
- 命令以当前用户身份运行，**不是系统沙箱**，可以访问工作区之外。审批前需检查完整命令；单条命令最长 60 秒，输出每路最多 32 KiB。
- 停止生成会保留已完成的文件修改；没有自动回滚。

## 构建与检查

```bash
cd apps/client
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

Windows 产物为 `target/release/coding01.exe`；Linux 为 `target/release/coding01`。GitHub Actions 包含 Windows / Linux 检查和手动发布构建工作流；本次交付的实际验证范围见 [修改与验证说明](docs/RELEASE_NOTES.md)。

配置位于 Windows `%APPDATA%/coding01`、Linux `$XDG_CONFIG_HOME/coding01` 或 `~/.config/coding01`。`CODING01_CONFIG_DIR` 可指定隔离目录。新设置与连接参数合并保存在 `preferences.json`，仍兼容旧连接文件。会话保存在 `history.json`，图片保存在 `attachments/`；这些文件包含对话与工具结果，请按项目数据保护。

[当前设计与兼容范围](01编码-项目设计方案.md)
