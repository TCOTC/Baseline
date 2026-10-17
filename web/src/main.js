import { createApp } from 'vue'
import App from './App.vue'
import './assets/view.css'
import './assets/shell.css'

// 界面的 JS 异常必须送出来：窗口没有地址栏，异常不主动上报就等于不存在。
function report(what, e) {
  const msg = what + ': ' + ((e && (e.stack || e.message)) || e)
  console.error(msg)
  try {
    fetch('/__jslog', { method: 'POST', body: msg, keepalive: true })
  } catch {
    /* 日志送不出去不该再抛一次 */
  }
}
window.addEventListener('error', (e) => report('window.onerror', e.message))
window.addEventListener('unhandledrejection', (e) => report('unhandled', e.reason))

createApp(App).mount('#app')
