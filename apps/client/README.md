# 01 编码 · 原生客户端(Rust + egui)

轻量、低内存、**GPU 原生绘制**的 AI 编码客户端 —— 无浏览器 / 无 WebView / 无图片资源,
Logo、图标、气泡、渐变、光标动画全部代码矢量绘制;中文注入系统字体(msyh)。

## 构建与运行

```bash
cd apps/client
cargo run                  # 开发运行
cargo build --release      # 发布构建(单文件约 3MB,opt-level=z + LTO + strip)
```

发布产物:`target/release/coding01.exe`

## 内置默认 API(言灵中转)

- **API 地址**:`https://ai.gzmath.cn/v1`
- **默认模型**:`deepseek-v4-pro`(可用 `GET /v1/models` 查看全部:deepseek/glm/qwen/minimax 等)
- **API Key**:用户自备,在客户端「设置」中填写,或:

```bash
nullclaw onboard --api-key sk-... --provider custom:https://ai.gzmath.cn/v1 --model deepseek-v4-pro
```

## 接入 NullClaw 网关

1. 配置 `~/.nullclaw/config.json` 的 `channels.web`(token 模式):

```json
"channels": {
  "web": { "accounts": { "default": {
    "transport": "local",
    "listen": "127.0.0.1",
    "port": 32123,
    "path": "/ws",
    "auth_token": "<32+ 位随机令牌>",
    "message_auth_mode": "token"
  } } }
}
```

2. 启动网关:`nullclaw gateway --port 32100`
3. 启动客户端:自动从 `~/.nullclaw/config.json` 发现 ws 地址与令牌并连接;
   连接失败自动回退内置演示(`CODING01_SIM=1` 可强制演示)。

协议:WebChannel v1(`pairing_request` / `user_message` / `assistant_chunk|final` /
`tool_call|result` / `approval_request|response` / `error`);
**token 模式下消息仅携带 `auth_token`(附带 `access_token` 会被网关静默丢弃)**;
会话绑定单连接(`session=default`)。

## 设置(客户端内)

「设置」弹窗读写 `~/.nullclaw/config.json` 与客户端连接配置:

- **模型 API**:地址 / Key(加密交由 nullclaw onboard 处理)/ 模型
- **Web 渠道**:端口 / 通道令牌
- **保存设置**:写配置(网关重启后生效)
- **保存并重启网关**:写配置 → `taskkill nullclaw.exe` → 后台拉起 `gateway --port <网关端口>` → 自动重连

「扩展」显示运行时能力(渠道/工具状态),「定时任务」显示 cron 列表,均为真实 CLI 输出。

## 测试钩子(开发用环境变量)

| 变量 | 作用 |
|---|---|
| `CODING01_SIM=1` | 强制本地演示模式 |
| `CODING01_SEND="text"` | 启动 2 秒后自动提交一条消息(端到端自检) |
| `CODING01_MODAL=settings\|info` | 启动即打开设置/能力弹窗 |
| `CODING01_TEST_SAVE=1` | 启动后自动执行一次设置保存并输出结果 |

## UI(对齐 ZCode 对话风格 + Harness 形态)

- Logo:01 字形去除终端下划线,`0`/`1` 间距放宽;标题栏图标为白底版,任务栏图标为深色版(WM_SETICON)
- 品牌行:logo / 01 编码 / **Code** 徽标同一中线对齐(徽标整体下移 1px)
- 对话正文使用略粗无衬线字体(microsoft yahei),标题/强调用粗体;`selectable_labels=false`,
  界面标签(含 Logo、品牌)不可拖选,复制统一走按钮(用户气泡 / 助手消息 / 代码复制)
- 选中文本为校准蓝底(#BFDBFE)黑字,不改变正文颜色
- 助手输出支持轻量 **Markdown**:标题、编号/圆点列表、行内代码、粗体、代码块(浅底圆角);
  单次返回以低帧率(10fps)打字机呈现,不干扰输入法组字
- 用户气泡右对齐、时间 + 复制;工具调用行(✦ 名称/摘要)、diff 红绿卡、审批行
- **附件**:+ 按钮选择文件/图片;图片解码为缩略图进入输入区(可 ✕ 移除),发送后随气泡
  展示(仿图三);文件路径以 `@path` 附带发送给 agent
- 「设置」为**居中固定弹窗**(不可拖动)+ 半透明遮罩;左导航(模型 API / Web 渠道)+ 右卡片,
  顶部「打开配置文件」直接调起记事本;保存并重启网关一键完成
- 侧栏菜单:「扩展」运行时能力、「定时任务」cron 列表、「模型」模型目录,均为真实 CLI 输出

## 已知取舍

- 网关一条 WS 连接绑定一个会话(`default`);多会话历史接入 `/history` 属下一里程碑
- 网关按消息拉起 agent,首条回复约 20–40 秒(之后同会话更快);推理型模型偶发
  `NoResponseContent`(仅返回 reasoning),重发即可
- **工具调用**:WebChannel 的 `tool_call/tool_result/approval_request` 已完整支持并在模拟器
  验证渲染;但言灵中转的 deepseek 模型在 nullclaw 中按 Anthropic 风格 XML 输出工具调用,
  nullclaw 未解析为原生 tool_calls(会以文本形式显示,工具请求可能挂起)。原生支持
  tool calling 协议的供应商(OpenRouter/Anthropic/OpenAI 官方)可直接获得完整工具循环
- 分片 UTF-8 帧会被客户端忽略而不断线;忙碌 90 秒无事件触发看门狗复位并提示超时
- `stick_to_bottom` 在 egui 0.27 存在偏移/裁剪缺陷,聊天区未启用
- 150% DPI:启动时 SetProcessDPIAware + 首帧 SetWindowPos 对齐画布与窗口;截图脚本同样需 DPI 感知
- 程序化点击受 Windows 前台锁限制(自动化测试用上述环境变量钩子)
