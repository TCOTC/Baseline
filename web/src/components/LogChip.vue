<script setup>
defineProps({
  label: { type: String, required: true },
  theme: { type: String, default: 'none' },
  muted: { type: Boolean, default: false },
  pending: { type: Boolean, default: false },
  suffix: { type: String, default: '' },
  title: { type: String, default: '' },
})
</script>

<template>
  <span
    class="log-chip"
    :class="{ 'log-chip--muted': muted, 'log-chip--pending': pending }"
    :data-theme="theme"
    :title="title || undefined"
  >
    {{ label }}<span v-if="pending || suffix" class="log-chip__suffix">·{{ pending ? '分类中' : suffix }}</span>
  </span>
</template>

<style scoped lang="scss">
.log-chip {
  display: inline-flex;
  align-items: center;
  padding: var(--space-1) var(--space-2);
  border: 1px solid var(--goal-border);
  border-radius: var(--radius-pill);
  background: var(--goal-background);
  color: var(--goal-text);
  font-size: var(--font-size-1);
  font-weight: bold;
  white-space: nowrap;

  // 挂着但不算数 / 补判还没回来：虚线、透明底，后面接一句原因。
  &--muted,
  &--pending {
    border-style: dashed;
    background: transparent;
    color: var(--text-faint);
  }
}
</style>
