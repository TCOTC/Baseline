<script setup>
import { nextTick, ref } from 'vue'

const props = defineProps({
  tabs: { type: Array, required: true },
  active: { type: String, required: true },
})
const emit = defineEmits(['select'])

const buttons = ref([])

function select(id) {
  if (id !== props.active) emit('select', id)
}

// 方向键在分类之间走动时，焦点也得跟着走；否则看起来选中了，键盘还停在原处。
function moveTo(index) {
  const tab = props.tabs[index]
  if (!tab) return
  emit('select', tab.id)
  nextTick(() => buttons.value[index] && buttons.value[index].focus())
}

function onKeydown(event, index) {
  let next = index
  if (event.key === 'ArrowDown') next = (index + 1) % props.tabs.length
  else if (event.key === 'ArrowUp') next = (index - 1 + props.tabs.length) % props.tabs.length
  else if (event.key === 'Home') next = 0
  else if (event.key === 'End') next = props.tabs.length - 1
  else return
  event.preventDefault()
  moveTo(next)
}
</script>

<template>
  <nav class="settings-tabs" aria-label="设置分类">
    <div class="settings-tabs__list" role="tablist" aria-orientation="vertical">
      <button
        v-for="(tab, index) in tabs"
        :id="`settings-tab-${tab.id}`"
        :key="tab.id"
        :ref="(el) => (buttons[index] = el)"
        type="button"
        role="tab"
        class="settings-tabs__tab"
        :class="{ 'settings-tabs__tab--active': tab.id === active }"
        :aria-selected="tab.id === active"
        :aria-controls="`settings-panel-${tab.id}`"
        :tabindex="tab.id === active ? 0 : -1"
        @click="select(tab.id)"
        @keydown="onKeydown($event, index)"
      >
        <span class="settings-tabs__label">{{ tab.label }}</span>
        <span class="settings-tabs__hint">{{ tab.hint }}</span>
      </button>
    </div>
  </nav>
</template>

<style scoped lang="scss">
// 第一层：分类。窄、固定、自己滚；右边内容再宽也不会挤到它。
.settings-tabs {
  flex: none;
  width: var(--settings-tabs-width);
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-4) var(--space-3) var(--space-5);
  border-right: 1px solid var(--border);
  background: var(--surface-2);
  scrollbar-gutter: stable;
}

.settings-tabs__list {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.settings-tabs__tab {
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius);
  color: var(--text-muted);
  text-align: left;

  &:hover {
    background: var(--surface);
    color: var(--text);
  }

  &--active {
    background: var(--surface);
    color: var(--text);
  }
}

.settings-tabs__label {
  font-size: var(--font-size-3);

  .settings-tabs__tab--active & {
    font-weight: bold;
  }
}

.settings-tabs__hint {
  font-size: var(--font-size-1);
  color: var(--text-faint);

  .settings-tabs__tab--active & {
    color: var(--text-muted);
  }
}
</style>
