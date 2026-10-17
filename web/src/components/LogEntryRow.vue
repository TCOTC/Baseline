<script setup>
import { computed } from 'vue'
import LogChip from './LogChip.vue'

const props = defineProps({
  row: { type: Object, required: true },
  state: { type: Object, default: null },
})

const wait = computed(() => !!(props.state && props.state.wait))
const note = computed(() => (props.state && props.state.note) || '')

const sub = computed(() => {
  let text = props.row.sub || ''
  if (wait.value) text += ' • AI 正在分类…'
  else if (note.value) text += ' • ' + note.value
  return text
})

const goals = computed(() => props.row.goals || [])
const dotTheme = computed(() => (goals.value[0] && goals.value[0].color) || 'none')
const unlinked = computed(() => !goals.value.length)
</script>

<template>
  <article class="log-entry" :class="{ 'log-entry--unlinked': unlinked }">
    <time class="log-entry__time">{{ row.time }}</time>

    <div class="log-entry__rail"><span class="log-entry__dot" :data-theme="dotTheme" /></div>

    <div class="log-entry__body">
      <p class="log-entry__text" :title="row.text">{{ row.text }}</p>
      <p class="log-entry__sub" :title="sub">{{ sub }}</p>
    </div>

    <div class="log-entry__tags">
      <LogChip
        v-for="(goal, index) in goals"
        :key="index"
        :label="goal.title"
        :theme="goal.color"
        :muted="!goal.counts && !wait"
        :pending="wait"
        :suffix="!goal.counts && !wait ? '不计入' : ''"
        :title="goal.counts ? (goal.byAi ? 'AI 挑的规则' : '') : '这条规则不接受手工记录，记了也不动这条线'"
      />
      <LogChip
        v-if="unlinked"
        label="没关联目标"
        theme="none"
        muted
        :pending="wait"
      />
    </div>
  </article>
</template>

<style scoped lang="scss">
// 行高固定——虚拟滚动靠它算偏移；所以备注不换行，超长省略，全文进 title。
.log-entry {
  height: var(--row-height-entry);
  display: grid;
  grid-template-columns: 46px 22px 1fr auto;
  gap: 10px;
  align-items: start;
  padding-top: 6px;
}

.log-entry__time {
  font-size: var(--font-size-1);
  color: var(--text-faint);
  font-variant-numeric: tabular-nums;
  font-family: var(--font-mono);
  text-align: right;
  padding-top: 2px;
}

// 竖线只在一组之内连着：每天一组。往上多伸 6px，接住前一行。
.log-entry__rail {
  position: relative;
  align-self: stretch;

  &::before {
    content: "";
    position: absolute;
    left: 50%;
    top: -6px;
    bottom: 0;
    width: 1px;
    background: var(--timeline-line);
    transform: translateX(-50%);
  }
}

.log-entry__dot {
  position: absolute;
  left: 50%;
  top: 6px;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: var(--goal-color);
  transform: translateX(-50%);
  z-index: 1;

  // 没关联目标的记录：空心点，安静但不隐藏。它确实发生过。
  &[data-theme="none"] {
    background: var(--background);
    box-shadow: inset 0 0 0 2px var(--border-strong);
  }
}

.log-entry__body {
  min-width: 0;
}

.log-entry__text {
  font-size: var(--font-size-3);
  font-weight: bold;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

// 没关联目标的记录：不抢注意力，但也不消失。
.log-entry--unlinked .log-entry__text {
  font-weight: normal;
  color: var(--text-muted);
}

.log-entry__sub {
  margin-top: 1px;
  font-size: var(--font-size-1);
  color: var(--text-faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.log-entry__tags {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding-top: 2px;
}
</style>
