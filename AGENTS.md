# AGENTS.md

在这个仓库里干活的约定。产品定位与设计取舍见 `README.md`，这里只写「怎么改、怎么验证、怎么提交」。

## 技术栈（2026-10-18 迁移后）

- **内核与 HTTP 服务**：Go 1.24+，SQLite 用 `modernc.org/sqlite`（纯 Go，无 cgo）。
- **界面**：Vue 3 + Vite + SCSS（`web/`），开发时 HMR；样式分全局层与组件 scoped 两层。
- **外壳**：Electron 44（无边框窗口 + 自绘顶栏 + 托盘）。
- 前端产物编进 Go 二进制（`internal/webui/dist`），所以**运行时只有一个 exe**。

## 常用命令

```powershell
go test ./...                                              # 判定 / 归属 / 快照语义（<1 秒）
go build -o bin/baseline.exe ./cmd/baseline                # 内核 + CLI + HTTP
go run ./cmd/baseline --db data/_demo.db serve --port 7717 --debug
cd web; pnpm dev                                           # Vite :5173（改 Vue/CSS 即时生效）
cd web; pnpm build                                         # 产物落进 internal/webui/dist
powershell -ExecutionPolicy Bypass -File scripts/demo.ps1  # 演示数据，写 data/_demo.db
powershell -ExecutionPolicy Bypass -File scripts/check.ps1 # 全量：构建 + 浏览器 + 外壳
node web/scripts/check-ui.mjs                              # 只跑浏览器端到端（服务要已在跑）
node web/scripts/check-electron.mjs                        # 只跑 Electron 外壳
```

默认库是 `%APPDATA%\Baseline\baseline.db`（`--db` 或 `BASELINE_DB` 可覆盖）。
**不要拿真实库跑自动化**，演示和测试一律指向 `data/` 下的临时库。
**测 AI 更要用一次性库**：`ai set` 会覆盖密钥，拿别人的库试一次就把人家配好的密钥冲掉了。

## 唯一的构建陷阱

**`pnpm build` 之后必须重新 `go build`。** 前端是编进二进制的，只 build 前端不 build Go，
浏览器里看到的还是旧页面——而且刷新、重启服务都没用，因为旧的那份在 exe 里。
这是这套结构里唯一一个「改了没生效」的坑。

## 验证：能用便宜的就别用贵的

**改完一批再验一次，不要改一行验一行。** 按代价从低到高：

| 想确认的事 | 用什么 | 代价 |
|---|---|---|
| 内核语义（归属、快照、拒绝） | `go test ./...` | <1 秒 |
| 一屏数据对不对（数字、待办数、AI 是否可用） | `GET /api/view` / `GET /__state` | 一次 HTTP |
| 文案、DOM 结构、曲线路径 | `node web/scripts/check-ui.mjs`（Playwright） | 几秒，不用开窗口 |
| 真实的排版和交互 | 上面那套的截图（`data/_ui/*.png`） | 读一张图 |
| 改样式要证明「视觉没动」 | `node web/scripts/shots.mjs <目录>` 改前/改后各一次，比 SHA256 | 两次截图 |
| 窗口按钮、托盘、关窗不退出 | `check-electron.mjs` | 几秒，会开一个窗口 |

两条踩过的坑：

- **页面里的数字一律来自服务端**。前端不算 delta、不算累计，连「+3 / 持平 / 还没有记录」
  都是 Go 算好的。改前端时别顺手在 `computed` 里补一次算术——两边各算各的，
  迟早显示成两个数，而且坏掉时不报错。
- **不要用截图判断「点击没生效」**。先看 `/__state` 或 `page.evaluate` 读到的 DOM，
  截图只能证明像素，不能证明状态。

## 调试：浏览器、`/__state`、Playwright

旧版（Tauri）那个「在页面里执行任意 JS」的调试桥**已经删了**，因为不再需要：

- 开发时浏览器直接开 `http://127.0.0.1:7717`，DevTools 原生可用；
- AI / 自动化用 Playwright 的 `page.evaluate`，能力等价，而且不用在服务里常开一个口子；
- `--debug` 只多一个 `GET /__state`：各目标的当前值、流水行数、没关联目标的条数、
  AI 能不能用。给「不想开浏览器」的场合用。

```powershell
.\bin\baseline.exe --db data/_demo.db serve --port 7717 --debug
Invoke-RestMethod http://127.0.0.1:7717/__state | ConvertTo-Json -Depth 4
```

安全边界：服务**只绑 127.0.0.1**，并校验 `Host`（防 DNS rebinding）。打包时额外用
随机端口 + 每次启动的 token（首访 `?t=` 换成 HttpOnly cookie）。
**别为了省事把这两条关掉，也别绑 0.0.0.0。**

## 硬约束

- **判定口径只有一份实现。** 就是 `internal/metrics.SourceValue`；记录归属是
  `store.ResolveLinks`。同一个语义不要在两处各写一遍。
  归不下来的**一律不猜**，但有两种处置：`defer = false` 就地拒绝并列出候选；
  `defer = true` **先挂空着写下来**，等 AI 补判或者等人去详情页指认。
- **AI 不能成为判定的第二份实现。** 它只从调用方给的候选里挑一条，候选之外的 id
  一律作废（`ai.ParseReply` 核对），置信度不到门槛就什么都不选。
  **顺序也不许反过来：记录先落库，AI 只负责事后补判**——它只填空着的格子，
  包括「压根没挂目标」那种。
- **数字和曲线同源**：`current` 和 `deltaWeek` 都取自快照序列，前端只负责显示。
- **快照写了不再改写**（`INSERT OR IGNORE`），只有今天例外、每次重算。
- **界面由 Vue 渲染，样式分两层。** `web/src/styles/` 只放必须全局的东西：调色板
  （`.blue` 加在容器上、后代读 `var(--goal-color)`，scoped 会拆断这条链）、流水行
  （由 innerHTML 拼，scoped 匹配不到）、跨屏原语（`.detail`/`.hero`/`.dsec` 等）。
  组件自己的样式写在 `<style scoped lang="scss">`，避免复制共享规则。
  零硬编码颜色，一律走 CSS 变量；字号 / 圆角 / 间距走 `--font-size-*` / `--radius-*` / `--space-*`，
  **不要再写 12.5px、9px 这种一次性取值**；数字 `tabular-nums`。
  **`font-weight` 只有 `bold` / `normal` 两档，不写数字字重**——字重的细微差别看不出来，
  只会让「谁该强调」变得含糊。
  **`line-height` 只在真正参与定位的地方写**（大数字盒、图标字形），其余继承 body 的 1.55；
  逐处调行高是最容易失控的一种「微调」。
  **全局 `button` 不带圆角**：窗口按钮是方角（Windows 标题栏就是这样），圆角只给动作按钮。
  **没有响应式断点**：这是桌面工具，窗口最小 880；浏览器验证也在这个宽度以上。
- **mutation 之后重取整个 view model**（`GET /api/view`），不做乐观更新。
  卡片上的数字、曲线末端、流水行必须来自同一次计算。
- **密钥明文不出内核。** 落库是密文，页面拿到的永远是掩码；掩码在 Go 里定死。
- **`.ps1` 必须是 UTF-8 with BOM**（Windows PowerShell 5.1 会把无 BOM 的脚本按 ANSI 读，
  中文注释直接变语法错误）。编辑工具会把 BOM 抹掉，所以顺序是「先改内容，最后补 BOM」：
  ```powershell
  $p = (Resolve-Path scripts\x.ps1).Path
  $b = [System.IO.File]::ReadAllBytes($p)
  if (-not ($b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF)) {
    [System.IO.File]::WriteAllBytes($p, ([byte[]](0xEF,0xBB,0xBF) + $b))
  }
  ```
- **源码只用编辑工具改，绝不用 PowerShell 重写文件。** `Get-Content | Set-Content` 会按
  当前代码页来回转一次，中文注释当场变成乱码。要动字节（比如给脚本加 BOM）就只动字节。
- 许可为专有许可（见 `LICENSE`），不接受外部 PR；不要引入会改变许可义务的依赖。

## 提交前

1. `go test ./...` 与 `go vet ./...` 通过。
2. 改了前端：`cd web; pnpm build`，然后**重新 `go build`**。
3. 改了界面/交互：`scripts/check.ps1`（退出码当闸门）。只改文案可以用更便宜的方式。
4. `git status` 里只该有你打算提交的文件。`bin/`、`data/`、`web/node_modules/`、
   `dist-electron/`、`internal/webui/dist/`（除 `.gitkeep`）都是运行时产物，永不入库。

## 提交信息风格

中文。标题一行说清「这次变了什么」，需要时用「范围：内容」的形式；正文 2–4 条要点，
只写结论和原因，不写过程流水。

```
内核与界面：迁移到 Go + Vue 3 + Electron

- 判定、归属、快照语义原样搬进 Go，测试一条没少；SQLite 用纯 Go 驱动，不需要 cgo。
- 界面改成 Vue，数字和曲线仍由服务端算好，前端不再有第二份算术。
- 外壳换成 Electron，保留无边框窗口、自绘顶栏和托盘；关窗仍只是收进托盘。
```

不要写的：

- 对话口吻（「他指出」「按他说的改」）。
- 每一步的操作流水；测试结果收成一行即可，不必每次单独成段。
- 引用仓库里不存在的文档章节（设计文档未公开）。
- 中途的临时产物（例如 `build.log` 提交了又删）——本地先确认再提交。

一次提交一件事；同一处的连续微调先合并成一个提交。作者身份沿用仓库既有的 noreply 邮箱。
仓库只有 `main` 一个分支，**已推送的历史不要重写**。
