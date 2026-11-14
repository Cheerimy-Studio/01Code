### 01Code

Lite AI Agent For 言灵中转.

连接言灵中转的桌面编码客户端，使用 Rust、egui 和本机 NullClaw。

## 开始使用

1. 启动客户端，首次运行自动从 `nullclaw/nullclaw` 官方 GitHub Release 下载当前平台的 NullClaw，并核对发布文件的 SHA-256。下载失败可在“设置 → 运行环境”重试。
2. 在“账户”选择“在浏览器中登录”，核对验证码后允许登录。网关启用 GitHub 登录时，也可以在浏览器中使用 GitHub 账户。
3. 登录成功后保存独立的设备 Key，自动同步账户、近七天用量和模型列表。模型或余额变化每 60 秒刷新，也可以手动同步。
4. 登录后在“模型”选择模型即可开始；模型接口兼容 OpenAI 格式。客户端**仅支持网页登录**，不提供 API Key 手动填写入口。

需要配套网关 1.7.61 或更新版本。设备登录必须由网关运营方先配置 HTTPS 和可信代理，见网关 README 的“01Code 客户端登录”。

## 功能

- 账户、模型、运行环境三个设置入口；内容随窗口宽度调整，长页面独立滚动。
- 自动安装、检查更新、启动和停止本客户端管理的 NullClaw；不会结束用户另行启动的同名程序。
- 各会话使用独立 WebSocket 绑定，断线自动重连；连接失败会显示真实错误。
- 对话记录保存在本机，保留最近 100 个会话，历史文件上限 16 MiB。图片缩略图不写入历史文件。
- 文件附件复制到受管工作区，每个附件上限 10 MiB；图片预览缩至 1024 像素。原始文件不会被移动。
- 支持查看能力、诊断、状态、Skill 列表，以及创建、查看和删除 Agent 定时任务。
- NullClaw 的工具、记忆、MCP 和其他渠道使用其原生配置；高级配置和日志可从运行环境打开。

远程凭据只保存到系统凭据管理器（由网页登录颁发，仅本机可用）。NullClaw 使用带随机令牌的本地转发接口，实际模型调用携带 `X-01Code-Client: 01Code/0.2.2`。默认限制工作区并使用 NullClaw 的 supervised 策略。

当前官方 WebChannel 提供消息、配对和流式回复；没有原生工具审批往返协议。客户端保留扩展事件解析，但不会宣称官方运行时已提供完整工具时间线或桌面逐项审批。工具权限以 NullClaw 原生安全策略为准。

## 构建与验证

```bash
cd apps/client
cargo build --release --locked
cargo test --locked
# 下载官方运行时并在本地模拟模型上验证完整消息链路
cargo test --locked official_runtime_roundtrip -- --ignored --nocapture
```

Windows 发布产物为 `target/release/coding01.exe`，Linux/macOS 为 `target/release/coding01`。Linux 需要桌面 Secret Service（例如 GNOME Keyring/KWallet）；凭据服务不可用时会明确报错，不会降级为明文保存。

配置与受管运行时默认位于 Windows `%APPDATA%/01Code`、macOS `~/Library/Application Support/01Code` 或 Linux `$XDG_DATA_HOME/01Code`（默认 `~/.local/share/01Code`）。不会修改已有的 `~/.nullclaw`。设置 `CODING01_HOME` 可以指定独立数据目录。调试模拟模式仅在显式设置 `CODING01_SIM=1` 时启用。

NullClaw 更新保留一份 `.previous` 旧文件；复制失败会回滚文件替换。更新之后仍需验证新版本与本地配置的兼容性。窗口退出会停止本客户端启动的进程。
