// Electron 外壳的检查：窗口、顶栏按钮、关窗进托盘、preload 桥。
//
// 和浏览器那套分开：那边验证的是界面和接口，这边验证的是「打包后的壳」——
// 它有没有把 Go 拉起来、有没有把 URL 读到、自绘顶栏的按钮接没接上。
import { _electron as electron } from 'playwright'
import { mkdirSync } from 'node:fs'
import path from 'node:path'

const OUT = process.env.OUT || path.resolve('../data/_ui')
const DB = process.env.BASELINE_DB || path.resolve('../data/_demo.db')
mkdirSync(OUT, { recursive: true })

const fails = []
function ok(name, cond, extra = '') {
  console.log((cond ? 'ok   ' : 'FAIL ') + name + (extra ? ' · ' + extra : ''))
  if (!cond) fails.push(name + (extra ? ' · ' + extra : ''))
}

const env = { ...process.env, BASELINE_DB: DB }
// 有些终端（包括这套 AI 的宿主）会带 ELECTRON_RUN_AS_NODE=1；
// 那样 Electron 会退化成 Node，require('electron') 拿不到 app。
delete env.ELECTRON_RUN_AS_NODE

const app = await electron.launch({
  args: ['.'],
  cwd: process.cwd(),
  env,
})

try {
  const win = await app.firstWindow({ timeout: 20000 })
  await win.waitForSelector('.card', { timeout: 20000 })
  ok('窗口起来了，页面有卡片', (await win.locator('.card').count()) === 3)
  ok('preload 桥在', await win.evaluate(() => !!(window.bl && window.bl.shell === 'electron')))
  ok('自绘顶栏在', (await win.locator('.bar').count()) === 1)
  ok('三个窗口按钮', (await win.locator('.bar .wbtn').count()) === 3)
  // Windows 的标题栏按钮是方角；全局 button 底座不能把圆角漏过来。
  const capRadius = await win.locator('#w-close').evaluate((el) => getComputedStyle(el).borderRadius)
  ok('窗口按钮是方角', capRadius === '0px', capRadius)

  const [w] = app.windows()
  ok('窗口可缩放（无边框不等于不能拖）', await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].isResizable()))

  await win.screenshot({ path: path.join(OUT, 'electron.png') })

  await win.locator('.card').first().click()
  await win.waitForSelector('.detail[data-goal]')
  ok('点卡片能进详情页', (await win.locator('.dtitle').innerText()).includes('计算机基础'))

  // 关窗 → 隐藏，不是退出。托盘是「随手记一条」的入口，关一次就退出它就没意义了。
  await win.locator('#w-close').click()
  await new Promise((r) => setTimeout(r, 500))
  const visible = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].isVisible())
  const alive = await app.evaluate(() => process.versions.electron)
  ok('关窗只是收进托盘', visible === false && !!alive)

  const count = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)
  ok('窗口没有销毁（还能从托盘叫回来）', count === 1)

  const errs = await win.evaluate(() => window.__blErrors || [])
  ok('页面没有 JS 异常', errs.length === 0, errs.join(' | '))
} catch (e) {
  fails.push('异常：' + e.message)
  console.error(e)
} finally {
  await app.close().catch(() => {})
}

if (fails.length) {
  console.error('\n失败 ' + fails.length + ' 项：')
  for (const f of fails) console.error(' - ' + f)
  process.exit(1)
}
console.log('\nElectron 外壳检查通过。截图在 ' + OUT)
