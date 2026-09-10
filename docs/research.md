# Research notes

验证日期：2026-09-10（本机 macOS 26.5.1 arm64；Codex CLI 0.153.4；Claude Code 2.1.220；tmux 3.6b）

## 结论

第一版使用 Tauri 2 + React/TypeScript + Rust + SQLite。Codex 采用官方 app-server 的 Unix-domain WebSocket 作为托管控制通道，并让 Terminal.app 中的 `codex --remote unix://…` 连接同一服务；这样应用和终端各自只有一个控制端，thread/run/turn 事件仍由服务端归一化。Claude Code 采用受管 tmux 会话，启动时固定 `--session-id`，通过官方 hooks 写入受限的本地事件通道；没有 hook 或无法确认 pane/进程身份时只进入观察/未知状态，不开启自动续跑。

## 来源与采用方式

| 来源 | 观察（在验证日期打开） | 实际采用 | 限制 |
| --- | --- | --- | --- |
| [Codex App Server 官方文档](https://developers.openai.com/codex/app-server/) | `initialize`/`initialized` 握手；`thread/start`、`thread/resume`；`turn/start`、`turn/steer`；`turn/started`、`turn/completed`、`turn/plan/updated`、`item/*`；审批请求需要客户端响应，`serverRequest/resolved` 表示清除；`thread.sessionId` 必须从服务端读取。文档同时给出 `codex app-server --listen` 与 `codex --remote`。 | Rust `CodexAdapter` 通过 Unix WebSocket 发送 JSON-RPC，持久化 thread/session/turn/item 事件；TerminalAdapter 用同一 socket 启动远程 TUI。 | App Server 仍是实验性接口，版本升级可能改变事件字段；终端 tab 与 thread 并非天然一一对应，绑定必须额外保存 socket、PID、tmux/TTY 证据。 |
| [Dimillian/CodexMonitor](https://github.com/Dimillian/CodexMonitor) | 当前 README 说明其为 Tauri 应用，按 workspace 启动/恢复 Codex app-server，使用 stdio，按 cwd 过滤线程；设置写入 app data；仓库有 400+ stars、活跃 release，许可证需以仓库 LICENSE 为准。 | 借鉴按项目恢复线程、把 app-server 事件作为唯一实时来源、持久化设置和重连。 | 其会话是 Codex thread；不能直接解决本产品要求的 Claude 会话或精确外部终端定位，因此不引入整个项目。 |
| [superset-sh/superset](https://github.com/superset-sh/superset) | 当前仓库是 Electron/TypeScript 的本地 agent 工作区，ELv2；围绕 worktree、PTY、终端和多 CLI agent 组织。 | 借鉴每个 Agent 都有稳定工作区/终端身份，以及异常后按 session id 恢复的原则。 | ELv2 不是本项目许可；其较大的 IDE/worktree 范围超出第一版。 |
| [BloopAI/vibe-kanban](https://github.com/BloopAI/vibe-kanban) | 文档说明每次 task attempt 用独立 Git worktree，并把执行状态和审阅/合并分离。 | 借鉴 Task、Run、人工验收分离；本产品默认成功 Run 进入“待验收”。 | 不复制其看板、云集成或工作区编排。 |
| [router-for-me/CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) / [官方说明](https://help.router-for.me/) | 代理同时暴露 Chat Completions、Responses、Gemini、Claude 等协议；兼容性依赖具体端点和模型。 | PromptService 明确区分 Responses 与 Chat Completions，单独处理流式 SSE、错误分类和 URL 拼接。 | 没有用户提供代理地址/密钥，线上代理联调未完成；不内置代理服务。 |
| [Claude Code hooks 文档](https://code.claude.com/docs/en/hooks) | Hooks 适合发送结构化生命周期事件；hook 必须快速返回，停止、工具调用、通知和会话结束并不等于任务成功。 | 受管 Claude 会话生成最小 hooks 配置，事件只用于归约证据；最终完成仍需 Stop/退出/结果字段组合判断。 | 外部手工启动的 Claude 只能发现/观察；无可信 hook/session/pane 绑定时不宣称可控。 |
| [tmux wiki](https://github.com/tmux/tmux/wiki) | tmux session/window/pane 有稳定目标格式；`list-panes`、`capture-pane`、`send-keys` 可按 pane 目标操作。 | 保存 `tmux_session`、`tmux_pane`、启动 PID、工作目录和命令指纹；发送 continue 前再次核对这些证据。 | tmux 只管理 PTY，不证明 Agent 协议状态；不能把 shell 提示符当作 Agent 输入状态。 |
| [iTerm2 Python API](https://iterm2.com/python-api/) / Terminal.app | iTerm2 可按 session/tab 精确操作但本机未安装；Terminal.app 可用 AppleScript，能力较弱且不能可靠读取 Agent 协议状态。 | 当前机器仅把 Terminal.app 作为可选 attach 目标；优先用受管 tmux pane，找不到目标时显示“终端未绑定”。 | iTerm2 未实测；不能声称支持所有终端，也不盲目向焦点窗口发输入。 |

## 最小真实链路验证记录

- 已核对本机 CLI：`codex-cli 0.153.4`、`Claude Code 2.1.220`、`tmux 3.6b`；Terminal.app 已安装，iTerm2 未安装。
- Codex App Server 的官方文档明确提供 Unix/WebSocket transport 与 `codex --remote unix://…`，实现将以此为托管链路。
- 受限环境没有把用户任务提交到真实模型；因此“启动 → 实时事件 → 人工输入 → 同会话终端定位 → 结束”的线上模型联调仍标记为未实测，开发测试使用协议 fixture 和本机 CLI smoke 命令，不能视为供应商联调成功。

### 2026-09-10 针对本机 CLI 的协议核对

用 `codex app-server generate-json-schema --experimental` 生成本机 0.153.4 的 schema，逐条比对代码里实际发送和读取的字段：

| 代码位置 | 断言 | 结果 |
| --- | --- | --- |
| `codex app-server --listen unix://PATH` | `--listen` 接受 `unix://PATH` | 通过 |
| `codex --remote <ADDR>` | 终端可连同一 app-server | 通过（flag 存在） |
| `initialize` / `initialized` | 均在 `ClientRequest` / `ClientNotification` 中 | 通过 |
| `thread/start` → `/thread/id` | `ThreadStartResponse.thread` 必填，`Thread.id` 必填 | 通过 |
| `turn/start` params `{threadId,input}` | 两者均为必填；`input` 为 `UserInput[]`，`{type:"text",text}` 合法 | 通过 |
| `turn/start` → `/turn/id` | `TurnStartResponse.turn`，`Turn.id` 必填 | 通过 |
| `thread/read` → `/thread/status/type == "idle"` | `ThreadStatus` 是带 `type` 的 oneOf，含 `idle` | 通过 |
| `/turn/error/codexErrorInfo == "serverOverloaded"` | `CodexErrorInfo` 含该字符串变体 | 通过 |
| 归约用到的事件名 | 全部出现在 `ServerNotification` / `ServerRequest` | 通过 |
| `claude --session-id/--name/--settings/--permission-mode manual` | 均见于 `claude --help` | 通过 |
| `claude -- <prompt>` | `--` 停止选项解析，负号开头的 prompt 也能送达 | 通过（对照 `claude --未知flag` 报错） |
| tmux `new-session -d -s … -e K=V -- argv…` | 多行 argv 原样传给子进程 | 通过（实测写文件回读） |

此外 `src-tauri/tests/codex_handshake.rs` 会真实启动本机 `codex app-server --listen unix://…`，完成 WebSocket 升级，跑通 `initialize` → `initialized` → `thread/start`，并断言响应里存在适配器实际读取的 `/thread/id`。它在 `turn/start` 之前停止，因此不会向模型提交任何内容。默认 `#[ignore]`，用 `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored` 运行；2026-09-10 在本机通过。

未覆盖：真实模型调用、真实 overload 重试、审批往返。这些需要联网凭据，仍标记未实测。

## 版本与兼容性策略

启动时记录 CLI 版本和能力探测结果。Codex 版本不支持 app-server/Unix transport 时降级为只读发现；Claude 版本不支持所需 hook 字段时降级为 tmux 观察。未知事件保留原始 JSON 并归约为 `unknown`/`disconnected`，不猜测为完成。 
