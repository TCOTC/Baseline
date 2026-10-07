# AGENTS.md

在这个仓库里干活的约定。产品定位与设计取舍见 `README.md`，这里只写「怎么改、怎么验证、怎么提交」。

## 常用命令

```powershell
cargo check -p baseline                                           # 内核单独编译（约 6 秒）
cargo test                                                        # 判定规则 / 归属 / 快照语义（约 2 秒）
cargo build --workspace                                           # 内核 + 外壳（约 10 秒，最后再跑）
cargo run -- init                                                 # 建库（幂等）
cargo run -- render --window -o data/_v.html                      # 导出窗口那一屏的 HTML（不用开窗口）
cargo run -p baseline-desktop                                     # 桌面窗口（窗口里 F5 即刷新数据）
powershell -ExecutionPolicy Bypass -File scripts/demo.ps1         # 演示数据，写 data/_demo.db
powershell -ExecutionPolicy Bypass -File scripts/ui-check.ps1     # 端到端外壳检查（15 项，退出码 0/1）
```

`cargo test` 和 `ui-check.ps1` 的覆盖面**不重叠**，改哪边跑哪边：前者守内核语义
（判定规则、记录归属、快照冻结），后者守**外壳**（窗口按钮、拖动、两栏独立滚动）。
内核那一侧坏掉时界面完全正常，只是数字变成另一个——没有报错，也没有红框。
`ui-check.ps1` 退出码 2 是「拿不到前台窗口，这一轮没跑」，不是通过。

默认库是 `%APPDATA%\Baseline\baseline.db`（`--db <路径>` 或 `BASELINE_DB` 可覆盖）。
**不要拿真实库跑自动化**，演示和测试一律指向 `data/` 下的临时库。
**测 AI 更要用一次性库**：`ai set` 会覆盖密钥，拿别人的库试一次就把人家配好的密钥冲掉了。

## 验证：能用便宜的就别用贵的

**改完一批再验一次，不要改一行验一行。** 下面按「代价从低到高」排，能用上面那条就
不要用下面那条：

| 想确认的事 | 用什么 | 代价 |
|---|---|---|
| 内核语义（归属、快照、拒绝） | `cargo test` | 2 秒 |
| 文案、DOM 结构、渲染出的属性 | `render --window/--settings` + `Select-String` | 1 秒，**不用开窗口** |
| 页面里的真实状态（元素坐标、DOM、变量、交互） | 调试桥 `Invoke-Js`（见下） | 一次 HTTP |
| 窗口看起来对不对 | `Save-Shot` + 看图 | 一次启动 + 读一张 200KB 的图 |
| 外壳（顶栏、拖动、两栏滚动） | `ui-check.ps1` | 半分钟，且要抢前台 |

两条踩过的坑：

- **动了内容/文案不必跑 `ui-check`**：它 15 项里 14 项在量顶栏和滚动，改流水内容碰不到它们；
  唯一相关的「界面没有 JS 异常」在 `data/desktop.log` 里是免费的。
- **截图会骗人**：`PrintWindow` 可能返回缓存帧，两张 PNG 字节一致（`Test-SameRegion` 能识破）。
  据此得出的「点击没生效」是假结论。

## 调试桥：在活着的页面上执行 JS

窗口没有开发工具，界面元素只能靠截图目测坐标去点——一次「点错 → 再截图 → 再点」
要好几个来回。所以外壳带一个**只在 `--debug` 下开**的桥（`src-tauri/src/debug.rs`）：

```powershell
Import-Module scripts\dbg.psm1
Start-Baseline -Db data/_demo.db        # 起窗口，等到真的能应答（不睡固定秒数）
Invoke-Js "document.querySelectorAll('.chip.pending').length"
Invoke-ClickOn '.send'                  # 先问坐标，再按坐标点
Invoke-TypeInto '#composer-input' '读完了第 3 章'
Get-UiLog -Tail 20
Stop-Baseline
```

它**只绑 127.0.0.1、随机端口、默认关闭**。能在页面里执行任意 JS 等于把窗口的完全
控制权交给本机进程，所以：**别默认开，别绑 0.0.0.0，别指望它上线**。

## 提交前

1. `cargo build --workspace` 通过（迭代时用 `cargo check -p baseline`）；改了 manifest 顺手
   `cargo metadata` 看一眼有没有警告。
2. 动了**外壳**就跑 `ui-check.ps1`，退出码当闸门；只动了内容/文案，用上面那张表里更便宜的办法。
3. `git status` 里只该有你打算提交的文件。`target/`、`data/`、`build.log`、`*.db` 都是运行时产物，永不入库。

## 硬约束

- **判定口径只有一份实现。** 内核是 lib（`src/lib.rs`），CLI 与桌面外壳都调它；同一个语义不要在两处各写一遍。
  判定就是 `metrics::source_value`，记录归属就是 `db::resolve_links`。归不下来的**一律不猜**，
  但有两种处置：`defer = false` 就地拒绝并列出候选（说不清就让人现在选）；
  `defer = true` **先挂空着写下来**，等 AI 补判或者等人去详情页指认。
  窗口里那排气泡只负责问，不负责判。
- **AI 不能成为判定的第二份实现。** 它只从调用方给的候选里挑一条，候选之外的 id 一律作废
  （`ai::parse_reply` 核对），置信度不到门槛就什么都不选。
  **顺序也不许反过来：记录先落库（`add_checkin` 里一次网络请求都没有），AI 只负责事后
  补判（`ai::classify_one`）——它只填空着的格子，包括「压根没挂目标」那种。**
  把模型放到写库之前，界面就得等它，而一条已经发生的事不该因为别人的服务器慢而记不下来。
  跟着这条走：**新加的判定逻辑进 `ai.rs` 之外的那一层**，别在提示词里再实现一遍规则。
- **命令的返回值也要管大小写。** Tauri 只对**参数**做 camelCase → snake_case，
  返回值走 serde 原样序列化——`needs_ai` 在界面上就是 `needs_ai`，不是 `needsAi`。
  漏了 `#[serde(rename_all = "camelCase")]`，表现是「记录写进去了，但再也没人叫 AI 补判」，
  而两边都不报错。**CLI 试不出来**：那边没有序列化这一层，只有真窗口能暴露。
- **密钥明文不出内核。** 落库是密文（`ai::seal`），页面拿到的永远是掩码；
  渲染时就把掩码定死，别把明文塞进 HTML 再让前端遮。
- **界面由 Rust 渲染**（`src/render.rs`）。唯一例外是流水行：它要虚拟滚动，所以由 `assets/view.js` 拼装，
  但行的 class 仍然只能来自 `assets/view.css`。
- **零硬编码颜色**，一律走 CSS 变量；SVG 内部类名必须带 `cv-` 前缀；数字必须 `tabular-nums`。
- **快照写了不再改写**（`INSERT OR IGNORE`），只有今天例外、每次重算；`current` 与曲线必须同源。
- **`.ps1` / `.psm1` 必须是 UTF-8 with BOM**（Windows PowerShell 5.1 会把无 BOM 的脚本按 ANSI 读，
  中文注释直接变语法错误）。入库统一 LF，脚本 CRLF，由 `.gitattributes` 保证。
  **编辑工具会把 BOM 抹掉**，所以脚本的顺序是「先改内容，最后补 BOM」，补完再跑一次：
  ```powershell
  $p = (Resolve-Path scripts\x.psm1).Path
  $b = [System.IO.File]::ReadAllBytes($p)
  if (-not ($b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF)) {
    [System.IO.File]::WriteAllBytes($p, ([byte[]](0xEF,0xBB,0xBF) + $b))
  }
  ```
  忘了补的表现很好认：报一堆互相矛盾的语法错误（「缺右 }」「字符串缺少终止符」），
  而照着行号去看，那几行明明是对的。
- **源码只用编辑工具改，绝不用 PowerShell 重写文件。** `Get-Content | Set-Content` 会按当前
  代码页来回转一次，中文注释当场变成乱码（踩过：整个 `db.rs` 被写坏，只能从 HEAD 恢复、
  把改动重做一遍）。要动字节（比如给脚本加 BOM）就只动字节，别把文本搬来搬去。
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
