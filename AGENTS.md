# AGENTS.md

在这个仓库里干活的约定。产品定位与设计取舍见 `README.md`，这里只写「怎么改、怎么验证、怎么提交」。

## 技术栈

- **内核与 HTTP 服务**：Go 1.24+，SQLite 用 `modernc.org/sqlite`（纯 Go，无 cgo）。
- **界面**：Vue 3 + Vite + SCSS（`web/`），开发时 HMR；样式分全局层与组件 scoped 两层。
- **外壳**：Electron 44（无边框窗口 + 自绘顶栏 + 托盘）。
- 前端产物编进 Go 二进制（`internal/webui/dist`），所以**运行时只有一个 exe**。

## 常用命令

```powershell
.\dev.ps1                                                  # 开发环境：Electron 窗口 + Vite 热重载，Ctrl+C 收工
.\dev.ps1 -Browser                                         # 同上，不起外壳，改开 http://127.0.0.1:7717
.\dev.ps1 -Reset                                           # 同上，先重建演示库
go test ./...                                              # 判定 / 归属 / 快照语义（<1 秒）
go build -o bin/baseline.exe ./cmd/baseline                # 内核 + CLI + HTTP
go run ./cmd/baseline --db data/_demo.db serve --port 7717 --dev --debug  # 手动起（--dev 才会代理到 Vite）
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

几条容易犯的错：

- **有头浏览器验证默认用 Chrome**（Playwright `channel: 'chrome'`）；无头的 `check-ui` 用 Edge 即可。
- **只有明确要求时才打安装包**：`pnpm run dist` 不随每次改动执行，日常验收到 `check.ps1` 为止。
- **页面里的数字一律来自服务端**。前端不算 delta、不算累计，连「+3 / 持平 / 还没有记录」
  都是 Go 算好的。改前端时别顺手在 `computed` 里补一次算术——两边各算各的，
  迟早显示成两个数，而且坏掉时不报错。
- **不要用截图判断「点击没生效」**。先看 `/__state` 或 `page.evaluate` 读到的 DOM，
  截图只能证明像素，不能证明状态。

## 调试：浏览器、`/__state`、Playwright

页面里没有执行任意 JS 的调试桥：

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

## 界面与样式

这一节是硬要求。原则：**样式跟着组件走，值走令牌，装饰能少就少。**

### 命名与 DOM

- 类名一律 BEM：`block__element--modifier`，全小写 kebab-case。
  block 用「东西」命名（`goal-card` / `settings-tabs` / `log-entry`），
  不用 `left` / `box` / `wrap` 这种位置名。
- 元素和状态用 `__element` / `--modifier`；不要 `on`、`active`、`now` 这种裸状态类。
- 颜色不写类名：容器上写 `data-theme="blue"`，后代读 `--goal-*`。
- DOM 用语义标签：`main` / `header` / `section` / `article` / `nav` / `time` / `h1`–`h3`；
  可点整块用 `<a>`，操作用 `<button>`。不用 `div` 堆出本来能语义化的结构。
- 不加只为了挂样式的包装层；能写在现有元素上的，不再包一层。
- 虚拟列表只渲染可见行，用真正的行组件（`LogList` + `LogDayRow` / `LogEntryRow`），
  不拼 `innerHTML` 字符串。
- 不用内联 `style`；只有动态几何（虚拟滚动偏移等）才允许 `:style`。
- ARIA 只在原生语义不够时加；`tablist` / `tab` / `tabpanel` 要成对，方向键可用。

### 样式归属

- 组件样式写在 `<style scoped lang="scss">`，不往全局层加类。
- 全局层只放：`_tokens.scss`（令牌 + `[data-theme]`）、`_base.scss`（重置 + `body` / `#app` + 控件重置）、
  `index.scss`（入口）。
- 跨屏重复的东西做成组件（`AppButton` / `AppInput` / `AppTextarea` / `FormField` /
  `AppSection` / `Notice` / `ActionBar` / `PageShell` / `ToggleChip` / `SourceKindPicker` / `BackLink`），
  不靠全局类名共享。
- 同一个视觉规则只写一份；两处出现就抽组件或合并选择器，不复制。
- `:deep()` 只用于子组件自己表达不了的布局穿透；能加 prop / class 就不 deep。

### 取值

- 颜色、字号、圆角、间距一律走令牌；不写 `#hex`、`12.5px`、`9px` 这种一次性值。
- 字号 8 档（`--font-size-1..8`）；圆角 3 档（`--radius` / `--radius-lg` / `--radius-pill`）；
  间距 4 的倍数（`--space-1..6`）。要新值先问「能不能用现有档」。
- 布局常数（列宽、内容最大宽）可以用字面量，但只写一处：组件内用局部 CSS 变量命名，
  不让 `720` / `224` 散落在两条规则里。
- `font-weight` 只有 `bold` / `normal`，不写数字。
- `line-height` 只在真正参与定位的地方写（大数字盒、图标字形），其余继承 `body` 的 1.55。
- 数字统一 `font-variant-numeric: tabular-nums`。
- 1–2px 的描边 / 偏移可以字面量；其余尺寸走 `var(--space-*)` / `var(--font-size-*)`。

### 重置与继承

- `* { margin: 0; padding: 0; box-sizing: border-box }` 已覆盖所有元素；
  组件里不再写 `padding: 0` / `margin: 0`，除非要覆盖组件自己的默认值。
- 全局 `button` 只重置字体、光标、背景、边框；不规定 padding / 圆角 / 颜色。
- 表单控件默认不继承文字色：`input, textarea` 的 `color` 在全局写一次，组件里不重复。
- 子元素能继承到的 `color` / `font-family` 不重复写，只写与父级不同的那一项。

### 装饰

- 默认不加：阴影、过渡、`letter-spacing`、装饰性 `opacity`、无意义的边框 / 圆角。
- 圆角只给面（卡片 / 对话框）、输入框、动作按钮、标签；图标按钮和窗口按钮是方角。
- 禁止「方角 + 左侧一条竖线」的选中 / 提醒样式；选中态用圆角背景或颜色，不画左边条。
- 窗口按钮必须方角；全局 `button` 不带圆角，避免漏给标题栏。
- 滚动条：8px 轨道、6px 胶囊滑块、透明轨道、无两端箭头。
  主内容（流水 / 设置页）的滚动条贴窗口右边缘，内容留白留在滚动容器上；
  侧栏（目标列 / 设置分类）的滚动条留在侧栏自己的边缘。
- 不做响应式断点：这是桌面工具，窗口最小 880；窄了让内容自己挤，不改布局。

### 注释

- 只写「现在为什么这样」，不写「以前怎样 / 这次改了什么 / 旧版如何」。
- 不写「不再是 innerHTML」「全局只有这三样」这类对照历史或数数的句子。
- 能删的注释就删；保留的只解释不明显的取舍（为什么方角、为什么固定行高、为什么先落库）。

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
- **界面改动的硬要求见《界面与样式》**：命名、DOM、样式归属、取值、重置、装饰、注释都按那一节执行。
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
