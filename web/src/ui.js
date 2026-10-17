import { reactive } from 'vue'

// 危险动作的确认框。不用 window.confirm：原生弹窗在这扇无边框窗口里很突兀，
// 而且它挡住 JS 线程，连我们自己的日志都送不出去。
export const modal = reactive({
  open: false,
  title: '',
  text: '',
  what: '',
  ok: '确定',
  danger: true,
})

let resolver = null

export function askConfirm(opts) {
  Object.assign(modal, { open: true, danger: true, what: '', ok: '确定' }, opts)
  return new Promise((resolve) => {
    resolver = resolve
  })
}

export function resolveModal(value) {
  modal.open = false
  const r = resolver
  resolver = null
  if (r) r(value)
}
