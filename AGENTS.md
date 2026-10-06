# AGENTS.md

在这个仓库里干活的约定。产品定位与设计取舍见 `README.md`，这里只写「怎么改、怎么验证、怎么提交」。

## 常用命令

```powershell
cargo build                                                       # 内核 + CLI
cargo run -- init                                                 # 建库（幂等）
cargo run -- render --open                                        # 导出单文件 HTML 并打开
cargo run -p baseline-desktop                                     # 桌面窗口（窗口里 F5 即刷新数据）
powershell -ExecutionPolicy Bypass -File scripts/demo.ps1         # 演示数据，写 data/_demo.db
powershell -ExecutionPolicy Bypass -File scripts/ui-check.ps1     # 端到端 UI 检查（15 项，退出码 0/1）
```

默认库是 `%APPDATA%\Baseline\baseline.db`（`--db <路径>` 或 `BASELINE_DB` 可覆盖）。
**不要拿真实库跑自动化**，演示和测试一律指向 `data/` 下的临时库。窗口没有开发工具，
界面里的 JS 异常会 POST 到 `/__jslog` 写进 `data/desktop.log`。

## 提交前

1. `cargo build` 通过；改了 manifest 顺手 `cargo metadata` 看一眼有没有警告。
2. 动了界面就跑 `ui-check.ps1`，退出码当闸门。它需要窗口能抢到前台（滚轮 / 点击类检查依赖前台；
   截图已改用 PrintWindow，不抢前台也能出图）。
3. `git status` 里只该有你打算提交的文件。`target/`、`data/`、`build.log`、`*.db` 都是运行时产物，永不入库。

## 硬约束

- **判定口径只有一份实现。** 内核是 lib（`src/lib.rs`），CLI 与桌面外壳都调它；同一个语义不要在两处各写一遍。
- **界面由 Rust 渲染**（`src/render.rs`）。唯一例外是流水行：它要虚拟滚动，所以由 `assets/view.js` 拼装，
  但行的 class 仍然只能来自 `assets/view.css`。
- **零硬编码颜色**，一律走 CSS 变量；SVG 内部类名必须带 `cv-` 前缀；数字必须 `tabular-nums`。
- **快照写了不再改写**（`INSERT OR IGNORE`），只有今天例外、每次重算；`current` 与曲线必须同源。
- **`.ps1` / `.psm1` 必须是 UTF-8 with BOM**（Windows PowerShell 5.1 会把无 BOM 的脚本按 ANSI 读，
  中文注释直接变语法错误）。入库统一 LF，脚本 CRLF，由 `.gitattributes` 保证。
- 许可为专有许可（见 `LICENSE`），不接受外部 PR；不要引入会改变许可义务的依赖。

## 提交信息风格

中文。标题一行说清「这次变了什么」，需要时用「范围：内容」的形式；正文 2–4 条要点，
只写结论和原因，不写过程流水。

```
内核与 CLI：数据层、判定规则与每日快照

- Goal / Source / Checkin / Snapshot 四个实体（rusqlite bundled）；判定规则即计分来源，
  没有来源的目标拒绝打卡，归档必须写原因。
- 快照写入后不改写（INSERT OR IGNORE），历史不会因判定规则改动被重画。
```

不要写的：

- 对话口吻（「他指出」「按他说的改」）。
- 每一步的操作流水；测试结果收成一行即可，不必每次单独成段。
- 引用仓库里不存在的文档章节（设计文档未公开）。
- 中途的临时产物（例如 `build.log` 提交了又删）——本地先确认再提交。

一次提交一件事；同一处的连续微调先合并成一个提交。作者身份沿用仓库既有的 noreply 邮箱。
仓库只有 `main` 一个分支，**已推送的历史不要重写**。
