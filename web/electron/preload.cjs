// 渲染进程能拿到的全部能力：三个窗口按钮 + 一个「是不是最大化了」。
//
// 页面自己走 HTTP 拿数据，所以这里没有 invoke 转发、没有业务 API——
// 那正是这套结构比旧版少一层的地方。
const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('bl', {
  shell: 'electron',
  minimize: () => ipcRenderer.send('win:minimize'),
  toggleMaximize: () => ipcRenderer.send('win:toggle-maximize'),
  close: () => ipcRenderer.send('win:close'),
  isMaximized: () => ipcRenderer.invoke('win:is-maximized'),
  onMaximized: (cb) => {
    const handler = (_ev, v) => cb(v)
    ipcRenderer.on('win:maximized', handler)
    return () => ipcRenderer.removeListener('win:maximized', handler)
  },
})
