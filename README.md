### 01Code

Lite AI Agent For 言灵中转.

轻量原生(Rust + egui,GPU 原生绘制)AI 编码 Agent 客户端,面向 NullClaw 网关,默认接入言灵中转 `https://ai.gzmath.cn/v1`。

## 功能

- 原生渲染,无 WebView;单文件约 3MB,低内存占用
- 流式消息(打字机)、Markdown 排版、工具调用时间线、diff 红绿卡、审批
- 文件 / 图片附件,聊天内缩略图预览
- 设置:模型 API(默认言灵中转)、Web 渠道端口与令牌、保存并重启网关
- 侧栏:会话、扩展能力(capabilities)、定时任务(cron)、模型目录,均为真实 nullclaw 输出

## 构建

```bash
cd apps/client
cargo run              # 开发运行
cargo build --release  # 发布构建 → target/release/coding01.exe
```

## 运行

1. NullClaw 配置 `channels.web`(token 模式,默认端口 32123)
2. `nullclaw gateway --port 32100`
3. 启动 `coding01.exe`(自动发现 ws 地址与令牌;网关不可用时自动回退内置演示)

## 设计文档

[01编码-项目设计方案.md](01编码-项目设计方案.md)
