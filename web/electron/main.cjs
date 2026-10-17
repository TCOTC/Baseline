// 基线桌面外壳。
//
// 这一层只做四件事：建窗口、托盘、拉起 Go 子进程、等它就绪后 loadURL。
// 界面和接口都在 Go 那边，所以这里没有 preload 之外的一行业务逻辑。
//
// **关掉窗口不等于退出**，只是收进托盘——托盘是「随手记一条」的入口。
// 真要退出走托盘右键菜单。
//
// 用 CJS（.cjs）而不是 ESM：Electron 的 ESM 主进程在 44 上对 `import { app }`
// 解析不到内建模块（会落到 npm 包里那个「返回 exe 路径」的默认导出上）。
// require('electron') 才是那条稳定的老路。
const { app, BrowserWindow, Menu, Tray, ipcMain, nativeImage, shell, dialog } = require('electron')
const { spawn } = require('node:child_process')
const path = require('node:path')
const crypto = require('node:crypto')

const repo = path.join(__dirname, '..', '..')
const DEV = process.env.BASELINE_DEV === '1'

let win = null
let tray = null
let child = null
let quitting = false

const iconPath = () =>
  app.isPackaged
    ? path.join(process.resourcesPath, 'icons', 'icon.ico')
    : path.join(repo, 'assets', 'icons', 'icon.ico')

function goBinary() {
  // 打包后 Go 二进制在 asar 外面（extraResources）；开发时用 bin/ 下刚构建的那份。
  return app.isPackaged ? path.join(process.resourcesPath, 'baseline.exe') : path.join(repo, 'bin', 'baseline.exe')
}

function startBackend() {
  const token = crypto.randomBytes(16).toString('hex')
  const args = ['serve', '--watch-stdin', '--port', '0']
  if (DEV) {
    // 开发时才开 /__state 和 Vite 代理；打包后的正式程序不暴露调试口。
    args.push('--dev', '--debug')
  } else {
    args.push('--token', token)
  }
  const dbArg = process.argv.find((a) => a.startsWith('--db='))
  if (dbArg) args.push(dbArg)

  child = spawn(goBinary(), args, { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true })
  child.stderr.on('data', (d) => console.error('[go]', String(d).trim()))
  child.on('exit', (code) => {
    console.error('[go] 退出', code)
    if (!quitting) {
      // 后端没了，窗口留着只会是一片「读取失败」。宁可退出，让人看得见。
      quitting = true
      app.quit()
    }
  })

  return new Promise((resolve, reject) => {
    let buf = ''
    const timer = setTimeout(() => reject(new Error('后端 10 秒内没有就绪')), 10000)
    child.stdout.on('data', (d) => {
      buf += String(d)
      let i
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i).trim()
        buf = buf.slice(i + 1)
        const m = line.match(/^BASELINE_URL=(\S+)$/)
        if (m) {
          clearTimeout(timer)
          resolve(m[1])
        }
        console.log('[go]', line)
      }
    })
    child.on('error', (e) => {
      clearTimeout(timer)
      reject(e)
    })
  })
}

function createWindow(url) {
  win = new BrowserWindow({
    width: 1280,
    height: 880,
    minWidth: 880,
    minHeight: 600,
    center: true,
    // 系统边框关掉，顶栏由页面自绘（46×40，照 Windows 量过的那组数）。
    frame: false,
    show: false,
    backgroundColor: '#fafaf8',
    title: '基线',
    icon: iconPath(),
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  })

  win.once('ready-to-show', () => win.show())
  win.loadURL(url)

  // 关掉窗口不等于退出：拦下来，收进托盘。
  win.on('close', (ev) => {
    if (!quitting) {
      ev.preventDefault()
      win.hide()
    }
  })
  win.on('maximize', () => win.webContents.send('win:maximized', true))
  win.on('unmaximize', () => win.webContents.send('win:maximized', false))
  // 外链一律交给系统浏览器，不要在应用窗口里打开。
  win.webContents.setWindowOpenHandler(({ url: u }) => {
    shell.openExternal(u)
    return { action: 'deny' }
  })
}

function showAndFocus() {
  if (!win) return
  win.show()
  if (win.isMinimized()) win.restore()
  win.focus()
  // 托盘的意义就是「两步记一条」：点图标、打字、回车。
  win.webContents.executeJavaScript('window.__blFocusComposer && window.__blFocusComposer()').catch(() => {})
}

function buildTray() {
  const icon = nativeImage.createFromPath(iconPath())
  tray = new Tray(icon.isEmpty() ? nativeImage.createEmpty() : icon)
  tray.setToolTip('基线')
  tray.setContextMenu(
    Menu.buildFromTemplate([
      { label: '打开基线', click: showAndFocus },
      { type: 'separator' },
      {
        label: '退出',
        click: () => {
          quitting = true
          app.quit()
        },
      },
    ]),
  )
  tray.on('click', showAndFocus)
}

function wireWindowControls() {
  ipcMain.on('win:minimize', () => win && win.minimize())
  ipcMain.on('win:toggle-maximize', () => {
    if (!win) return
    if (win.isMaximized()) win.unmaximize()
    else win.maximize()
  })
  ipcMain.on('win:close', () => win && win.close())
  ipcMain.handle('win:is-maximized', () => !!(win && win.isMaximized()))
}

const gotLock = app.requestSingleInstanceLock()
if (!gotLock) {
  app.quit()
} else {
  app.on('second-instance', showAndFocus)

  app.whenReady().then(async () => {
    app.setAppUserModelId('dev.baseline.desktop')
    wireWindowControls()
    buildTray()
    try {
      const url = await startBackend()
      createWindow(url)
    } catch (e) {
      console.error(e)
      dialog.showErrorBox('基线启动失败', String(e && e.message ? e.message : e))
      app.quit()
    }
  })

  app.on('before-quit', () => {
    quitting = true
    if (child) {
      // stdin 关闭 → Go 侧看到 EOF 主动退出；再兜一个 kill，防止它卡在写盘上。
      try {
        child.stdin.end()
      } catch {
        /* 已经关了 */
      }
      const c = child
      setTimeout(() => {
        try {
          c.kill()
        } catch {
          /* 已经退了 */
        }
      }, 1500)
    }
  })

  app.on('window-all-closed', () => {
    // 托盘常驻：不在这里退出。
  })
}
