# 01Code 原生客户端

入口为 `src/main.rs`，具体使用、构建、存储和兼容范围见 [项目说明](../../README.md)。

开发检查：

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

开发钩子（仅用于隔离的测试配置目录）：

| 环境变量 | 用途 |
|---|---|
| `CODING01_CONFIG_DIR` | 指定独立配置及会话目录 |
| `CODING01_SIM=1` | 显式演示模式；不会连接实际上游 |
| `CODING01_MODAL=settings` | 启动时打开设置 |
| `CODING01_SEND` | 启动两秒后发送指定文本 |
| `CODING01_ATTACH` | 启动时附加指定本地文件 |
| `CODING01_TEST_SAVE=1` | 启动后执行设置保存；仅限测试 |
| `NULLCLAW_BIN` | 指定 NullClaw 可执行文件 |

不设置演示变量时，所有生成和工具结果来自真实接口；连接错误在会话中显示。
