<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import LogDayRow from './LogDayRow.vue'
import LogEntryRow from './LogEntryRow.vue'

// 流水用**虚拟滚动**：只渲染可见的几十行，几千条也不卡。
//
// 可见行由 LogDayRow / LogEntryRow 渲染，行的样式也在各自组件里；
// 这里只管滚动位置、可见区切片和 translateY。
const props = defineProps({
  rows: { type: Array, default: () => [] },
  aiState: { type: Object, default: () => ({}) },
  variant: { type: String, default: 'main' }, // main | detail
  visible: { type: Boolean, default: true },
  showEmpty: { type: Boolean, default: true },
})

const viewport = ref(null)
const scrollTop = ref(0)
const viewportHeight = ref(0)
const dayHeight = ref(34)
const entryHeight = ref(46)
const OVERSCAN = 8

const offsets = computed(() => {
  const out = new Array(props.rows.length + 1)
  out[0] = 0
  for (let i = 0; i < props.rows.length; i++) {
    out[i + 1] = out[i] + (props.rows[i].k === 'd' ? dayHeight.value : entryHeight.value)
  }
  return out
})

const totalHeight = computed(() => offsets.value[props.rows.length] || 0)

function indexAt(y) {
  const table = offsets.value
  let lo = 0
  let hi = props.rows.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (table[mid + 1] <= y) lo = mid + 1
    else hi = mid
  }
  return Math.max(0, Math.min(lo, props.rows.length - 1))
}

const start = computed(() => Math.max(0, indexAt(scrollTop.value) - OVERSCAN))
const end = computed(() =>
  Math.min(props.rows.length, indexAt(scrollTop.value + viewportHeight.value) + 1 + OVERSCAN),
)
const visibleRows = computed(() => props.rows.slice(start.value, end.value))
const offsetY = computed(() => offsets.value[start.value] || 0)

// 行高只有 CSS 一处定义。解析不出来就退回默认值：
// 宁可行高差几像素，也不要整条流水不显示。
function readHeights() {
  const styles = getComputedStyle(document.documentElement)
  const day = parseFloat(styles.getPropertyValue('--row-height-day'))
  const entry = parseFloat(styles.getPropertyValue('--row-height-entry'))
  if (isFinite(day) && day > 0) dayHeight.value = day
  if (isFinite(entry) && entry > 0) entryHeight.value = entry
}

function measure() {
  if (!viewport.value) return
  viewportHeight.value = viewport.value.clientHeight
  scrollTop.value = viewport.value.scrollTop
}

let frame = 0
function onScroll() {
  if (frame) return
  frame = requestAnimationFrame(() => {
    frame = 0
    measure()
  })
}

function scrollToBottom() {
  if (!viewport.value) return
  viewport.value.scrollTop = viewport.value.scrollHeight
  scrollTop.value = viewport.value.scrollTop
}

watch(
  () => props.rows,
  () => {
    nextTick(() => {
      measure()
      scrollToBottom()
    })
  },
  { deep: false },
)

watch(
  () => props.visible,
  (on) => {
    if (!on) return
    nextTick(() => {
      measure()
      scrollToBottom()
    })
  },
)

onMounted(() => {
  readHeights()
  nextTick(() => {
    measure()
    scrollToBottom()
  })
  window.addEventListener('resize', measure)
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', measure)
  if (frame) cancelAnimationFrame(frame)
})
</script>

<template>
  <div
    ref="viewport"
    id="log"
    class="log"
    :class="{ 'log--detail': variant === 'detail' }"
    @scroll="onScroll"
  >
    <p v-if="!rows.length && showEmpty" class="log__empty">
      <slot name="empty">还没有任何记录。<br />下面那个输入框里写一句，回车——记的是「刚做了什么」。</slot>
    </p>

    <div v-else-if="rows.length" class="log__canvas" :style="{ height: totalHeight + 'px' }">
      <div class="log__rows" :style="{ transform: `translateY(${offsetY}px)` }">
        <template v-for="row in visibleRows" :key="row.k === 'd' ? 'day:' + row.title : 'entry:' + row.id">
          <LogDayRow v-if="row.k === 'd'" :row="row" />
          <LogEntryRow v-else :row="row" :state="aiState[row.id]" />
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped lang="scss">
.log {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-gutter: stable;
  // 右内边距留在滚动容器上：内容仍落在页面内容区，滚动条贴住窗口右边缘。
  padding: 0 calc(var(--page-padding) + var(--page-margin)) var(--space-4) 0;
}

// 详情页的记录区就是这一栏本身。
.log--detail {
  padding-right: var(--page-padding);
}

// 虚拟滚动的画布：整条流水的高度撑在这里，可见的那几十行绝对定位在里面。
.log__canvas {
  position: relative;
}

.log__rows {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
}

.log__empty {
  padding: var(--space-5) var(--space-1);
  color: var(--text-faint);
  font-size: var(--font-size-3);
}
</style>
