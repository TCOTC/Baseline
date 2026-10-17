// 只截图、不交互：给样式改动做「改前 / 改后」的像素基准。
// 三屏都必须是确定性的（演示数据的时间都是 00:00，页面不显示当前时间）。
import { chromium } from 'playwright'
import { mkdirSync } from 'node:fs'
import path from 'node:path'

const BASE = process.env.BASE || 'http://127.0.0.1:7717'
const OUT = process.argv[2] || path.resolve('../data/_ui/base')
mkdirSync(OUT, { recursive: true })

const browser = await chromium.launch({ channel: 'msedge', headless: true })
const page = await browser.newPage({ viewport: { width: 1280, height: 880 } })
const errors = []
page.on('pageerror', (e) => errors.push(e.message))

await page.goto(BASE, { waitUntil: 'networkidle' })
await page.waitForSelector('.goal-card')
await page.waitForTimeout(150)
await page.screenshot({ path: path.join(OUT, 'main.png') })

await page.locator('.goal-card').first().click()
await page.waitForSelector('.page[data-goal]')
await page.waitForTimeout(150)
await page.screenshot({ path: path.join(OUT, 'goal.png') })

await page.click('.back-link')
await page.waitForSelector('.goal-card')
await page.click('#gear')
await page.waitForSelector('.settings-tabs')
await page.waitForTimeout(150)
await page.screenshot({ path: path.join(OUT, 'settings.png') })

await page.click('#settings-tab-connection')
await page.waitForSelector('#ai-base')
await page.waitForTimeout(150)
await page.screenshot({ path: path.join(OUT, 'settings-connection.png') })

await browser.close()
if (errors.length) {
  console.error('JS 异常：', errors.join(' | '))
  process.exit(1)
}
console.log('截图完成：' + OUT)

