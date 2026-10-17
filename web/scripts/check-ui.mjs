// 端到端检查：真的开浏览器、真的点、真的打字，然后断言语义。
//
// 这是旧版 ui-check.ps1 的替代：那边靠截屏量像素，这边直接读 DOM 和接口，
// 快、稳，而且 AI 读得懂失败在哪一步。
//
// 前提：服务已经在跑（默认 http://127.0.0.1:7717），库是 demo 数据。
import { chromium } from 'playwright'
import { mkdirSync } from 'node:fs'
import path from 'node:path'

const BASE = process.env.BASE || 'http://127.0.0.1:7717'
const OUT = process.env.OUT || path.resolve('../data/_ui')
mkdirSync(OUT, { recursive: true })

const fails = []
function ok(name, cond, extra = '') {
  console.log((cond ? 'ok   ' : 'FAIL ') + name + (extra ? ' · ' + extra : ''))
  if (!cond) fails.push(name + (extra ? ' · ' + extra : ''))
}

const browser = await chromium.launch({ channel: 'msedge', headless: true })
const page = await browser.newPage({ viewport: { width: 1280, height: 880 } })

const errors = []
page.on('pageerror', (e) => errors.push('pageerror: ' + e.message))
page.on('console', (m) => {
  if (m.type() === 'error') errors.push('console: ' + m.text())
})

try {
  await page.goto(BASE, { waitUntil: 'networkidle' })
  await page.waitForSelector('.card', { timeout: 10000 })

  // ---- 主视图 ----
  ok('三张卡片', (await page.locator('.card').count()) === 3)
  const firstTitle = await page.locator('.card .ct').first().innerText()
  ok('第一张卡是「计算机基础」', firstTitle.includes('计算机基础'), firstTitle)
  const firstValue = await page.locator('.card .v').first().innerText()
  ok('当前值是 23', firstValue.trim() === '23', firstValue)
  const firstDelta = await page.locator('.card .cd').first().innerText()
  ok('本周位移带正号', firstDelta.trim().startsWith('+'), firstDelta)
  ok('第一张卡有 30 格条纹', (await page.locator('.card').first().locator('.tick').count()) === 30)
  const d = await page.locator('.card .cv-line').first().getAttribute('d')
  ok('曲线是阶梯路径', !!d && d.includes('H') && d.includes('V'), d && d.slice(0, 40))
  await page.screenshot({ path: path.join(OUT, 'main.png') })

  const rowsInDom = await page.locator('.log-rows .entry').count()
  ok('流水只画可见的几十行', rowsInDom > 0 && rowsInDom < 80, String(rowsInDom))
  ok('输入框在', (await page.locator('#composer-input').count()) === 1)
  ok('三个目标标签', (await page.locator('#gchips .gchip').count()) === 3)

  // ---- 记一条（不选目标）----
  await page.fill('#composer-input', '无目标的一条')
  await page.press('#composer-input', 'Enter')
  await page.waitForFunction(() => document.body.innerText.includes('无目标的一条'), null, { timeout: 8000 })
  ok('无目标的记录进了流水', true)
  ok('这条被标成没关联目标', (await page.locator('.entry.unlinked').count()) >= 1)
  ok('主视图出现「没关联目标」的提醒', (await page.locator('#unlinked-warn').count()) === 1)

  // ---- 记一条（带目标）----
  await page.click('#gchips .gchip')
  await page.waitForSelector('#rules .rchip')
  ok('选了目标之后出现规则气泡', (await page.locator('#rules .rchip').count()) >= 1)
  await page.fill('#composer-input', '带目标的一条')
  await page.press('#composer-input', 'Enter')
  await page.waitForFunction(() => document.body.innerText.includes('带目标的一条'), null, { timeout: 8000 })
  const lastEntry = page.locator('.log-rows .entry').last()
  ok('带目标的记录有一个目标标签', (await lastEntry.locator('.chip').count()) === 1)
  ok('输入框提交后被清空', (await page.inputValue('#composer-input')) === '')

  // 这一条让「计算机基础」从 23 变成 24；后面的断言都对着这个数。
  const cardNow = (await page.locator('.card .v').first().innerText()).trim()
  ok('带目标的记录让数字涨了一格', cardNow === '24', cardNow)

  // ---- 详情页 ----
  await page.locator('.card').first().click()
  await page.waitForSelector('.detail[data-goal]')
  const detailTitle = await page.locator('.dtitle').innerText()
  ok('进入详情页', detailTitle.includes('计算机基础'), detailTitle)
  ok('详情页有判定规则', (await page.locator('.rule').count()) >= 1)
  ok('详情页曲线更大', (await page.locator('.dcurve .cv').count()) === 1)
  ok('详情页有记录', (await page.locator('.dlog .entry').count()) > 0)
  ok('当前值和卡片一致', (await page.locator('.dval').innerText()).trim() === cardNow)
  await page.screenshot({ path: path.join(OUT, 'goal.png') })

  // ---- 设置页 ----
  await page.click('.back')
  await page.waitForSelector('.card')
  await page.click('#gear')
  await page.waitForSelector('#ai-base')
  ok('设置页有接口地址', (await page.inputValue('#ai-base')).length > 0)
  ok('设置页有模型名', (await page.inputValue('#ai-model')).length > 0)
  ok('密钥框是空的（明文不回显）', (await page.inputValue('#ai-key')) === '')
  const ph = await page.getAttribute('#ai-key', 'placeholder')
  ok('密钥框只在 placeholder 里给掩码', ph === '还没填' || ph.startsWith('现在是'), ph)
  await page.screenshot({ path: path.join(OUT, 'settings.png') })

  // ---- 调试口 ----
  const state = await page.evaluate(async () => (await fetch('/__state')).json())
  ok('/__state 能读到', !!state && state.goals.length === 3)
  ok('/__state 的数字和界面一致', state.goals[0].current === cardNow, state.goals[0].current)

  ok('页面没有 JS 异常', errors.length === 0, errors.join(' | '))
} catch (e) {
  fails.push('异常：' + e.message)
  console.error(e)
  await page.screenshot({ path: path.join(OUT, 'failure.png') }).catch(() => {})
} finally {
  await browser.close()
}

if (fails.length) {
  console.error('\n失败 ' + fails.length + ' 项：')
  for (const f of fails) console.error(' - ' + f)
  process.exit(1)
}
console.log('\n全部通过。截图在 ' + OUT)
