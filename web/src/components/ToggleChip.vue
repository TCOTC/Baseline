<script setup>
defineProps({
  as: { type: String, default: 'span' },
  theme: { type: String, default: 'none' },
  shape: { type: String, default: 'pill' }, // pill | rect
  selected: { type: Boolean, default: false },
  dashed: { type: Boolean, default: false },
  dot: { type: Boolean, default: false },
  title: { type: String, default: '' },
})
</script>

<template>
  <component
    :is="as"
    class="toggle-chip"
    :class="[
      `toggle-chip--${shape}`,
      { 'toggle-chip--selected': selected, 'toggle-chip--dashed': dashed },
    ]"
    :data-theme="theme"
    :title="title || undefined"
  >
    <i v-if="dot" class="toggle-chip__dot" />
    <slot />
  </component>
</template>

<style scoped lang="scss">
.toggle-chip {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-2);
  border: 1px solid transparent;
  border-radius: var(--radius-pill);
  color: var(--text-faint);
  font-size: var(--font-size-2);
  white-space: nowrap;

  &:hover {
    border-color: var(--border-strong);
    color: var(--text-muted);
  }

  // 规则气泡是圆角矩形，不是胶囊：两种气泡长得一样就分不清哪排是哪排。
  &--rect {
    border-color: var(--border-strong);
    border-radius: var(--radius);
    color: var(--text-muted);
  }

  &--selected {
    border-color: var(--goal-border);
    background: var(--goal-background);
    color: var(--goal-text);
    font-weight: bold;
  }

  // 规则里没有手工打卡：选得中，但画成虚线，表示「记了也不动这条线」。
  &--dashed.toggle-chip--selected {
    border-style: dashed;
  }

  &__dot {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--border-strong);
  }

  &--selected &__dot {
    background: var(--goal-color);
    box-shadow: none;
  }
}
</style>
