<script setup>
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

// 流水用**虚拟滚动**：几条到几千条都只画可见的那几十行。
//
// 这是全项目唯一一段命令式 DOM：它要知道滚到哪儿了、总高多少、该画哪几行。
// 行的 class 仍然全部来自 view.css，所以样式还是只有一份——搬过来的只是拼装。
const props = defineProps({
  rows: { type: Array, default: () => [] },
  aiState: { type: Object, default: () => ({}) },
  variant: { type: String, default: 'log' }, // log | dlog
  visible: { type: Boolean, default: true },
  // 详情页的空状态自己写在 .dlog 外面（和旧版一样），这里就不再画一条。
  showEmpty: { type: Boolean, default: true },
})

const logEl = ref(null)
const canvasEl = ref(null)
const rowsEl = ref(null)

const ROW = { d: 34, e: 46 }
const OVERSCAN = 8
let offsets = []
let lastKey = ''
let pending = false

function readHeights() {
  const cs = getComputedStyle(document.documentElement)
  const d = parseFloat(cs.getPropertyValue('--row-day'))
  const e = parseFloat(cs.getPropertyValue('--row-entry'))
  // 解析不出来就退回默认值：宁可行高差几像素，也不要整条流水不显示。
  if (isFinite(d) && d > 0) ROW.d = d
  if (isFinite(e) && e > 0) ROW.e = e
}

const rowHeight = (r) => (r.k === 'd' ? ROW.d : ROW.e)

function esc(s) {
  return String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c])
}

function rowHtml(r) {
  if (r.k === 'd') {
    // 具体年月日在 title 上，悬停才出现。看得见的是「哪天」+「周几」。
    return (
      '<div class="day"><div class="dlabel" title="' + esc(r.title || '') + '">' +
      '<span class="dl">' + esc(r.label) + '</span>' +
      (r.wd ? '<span class="dw">' + esc(r.wd) + '</span>' : '') +
      '</div></div>'
    )
  }

  const st = props.aiState[r.id] || null
  const wait = !!(st && st.wait)
  const note = st && st.note ? st.note : ''

  // 一条记录可以挂多个目标。挂着的都列出来；`counts` 为假表示这条规则
  // 不接受手工记录——标签画成虚线并说明。
  let tags = (r.goals || [])
    .map((g) => {
      // 补判还没回来时不下结论：既不说「计入」，也不说「不计入」。
      const no = g.counts || wait ? '' : ' noscore'
      const pend = wait ? ' pending' : ''
      const tip = g.counts ? (g.byAi ? 'AI 挑的规则' : '') : '这条规则不接受手工记录，记了也不动这条线'
      return (
        '<span class="chip ' + esc(g.color) + no + pend + '"' +
        (tip ? ' title="' + esc(tip) + '"' : '') + '>' + esc(g.title) + '</span>'
      )
    })
    .join('')
  // **先判空，再补那句兜底标签。** 顺序反了的话 `none` 永远算不出来，
  // 没关联目标的行就不会被画成虚线（踩过一次）。
  const none = tags ? '' : ' unlinked'
  if (!tags) {
    tags = wait
      ? '<span class="chip none pending">还没关联目标</span>'
      : '<span class="chip none">没关联目标</span>'
  }

  let sub = r.sub || ''
  if (wait) sub += ' • AI 正在分类…'
  else if (note) sub += ' • ' + note

  const dot = (r.goals && r.goals[0] && r.goals[0].color) || 'none'
  return (
    '<div class="entry' + none + '">' +
    '<div class="time">' + esc(r.time) + '</div>' +
    '<div class="rail"><span class="dot ' + esc(dot) + '"></span></div>' +
    '<div class="body">' +
    '<div class="act" title="' + esc(r.text) + '">' + esc(r.text) + '</div>' +
    '<div class="sub" title="' + esc(sub) + '">' + esc(sub) + '</div>' +
    '</div>' +
    '<div class="tags">' + tags + '</div></div>'
  )
}

function build() {
  const list = props.rows
  offsets = new Array(list.length + 1)
  offsets[0] = 0
  for (let i = 0; i < list.length; i++) offsets[i + 1] = offsets[i] + rowHeight(list[i])
  if (canvasEl.value) canvasEl.value.style.height = offsets[list.length] + 'px'
}

function indexAt(y) {
  let lo = 0
  let hi = props.rows.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (offsets[mid + 1] <= y) lo = mid + 1
    else hi = mid
  }
  return Math.max(0, Math.min(lo, props.rows.length - 1))
}

function paint() {
  if (!rowsEl.value || !logEl.value) return
  if (!props.rows.length) return
  if (!offsets.length) build()
  const top = logEl.value.scrollTop
  const vh = logEl.value.clientHeight
  const start = Math.max(0, indexAt(top) - OVERSCAN)
  const end = Math.min(props.rows.length, indexAt(top + vh) + 1 + OVERSCAN)
  const key = start + ':' + end
  if (key === lastKey) return
  lastKey = key
  let html = ''
  for (let i = start; i < end; i++) html += rowHtml(props.rows[i])
  rowsEl.value.style.transform = 'translateY(' + offsets[start] + 'px)'
  rowsEl.value.innerHTML = html
}

function schedule() {
  if (pending) return
  pending = true
  requestAnimationFrame(() => {
    pending = false
    paint()
  })
}

function scrollToBottom() {
  if (!logEl.value) return
  logEl.value.scrollTop = logEl.value.scrollHeight
  paint()
}

function refresh() {
  build()
  lastKey = ''
  paint()
}

watch(
  () => props.rows,
  () => {
    nextTick(() => {
      refresh()
      scrollToBottom()
    })
  },
  { deep: false },
)

watch(
  () => props.aiState,
  () => {
    lastKey = ''
    paint()
  },
  { deep: true },
)

watch(
  () => props.visible,
  (on) => {
    if (!on) return
    nextTick(() => {
      refresh()
      scrollToBottom()
    })
  },
)

function onResize() {
  lastKey = ''
  schedule()
}

onMounted(() => {
  readHeights()
  nextTick(() => {
    refresh()
    scrollToBottom()
  })
  window.addEventListener('resize', onResize)
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', onResize)
})
</script>

<template>
  <div ref="logEl" id="log" :class="variant" @scroll="schedule">
    <div v-if="!rows.length && showEmpty" class="log-hint">
      <slot name="empty">还没有任何记录。<br />下面那个输入框里写一句，回车——记的是「刚做了什么」。</slot>
    </div>
    <div v-else-if="rows.length" ref="canvasEl" class="log-canvas">
      <div ref="rowsEl" class="log-rows"></div>
    </div>
  </div>
</template>
