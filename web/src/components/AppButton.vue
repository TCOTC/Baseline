<script setup>
defineProps({
  variant: { type: String, default: 'default' }, // default | primary | danger | dashed | ghost
  type: { type: String, default: 'button' },
  disabled: { type: Boolean, default: false },
})
</script>

<template>
  <button :type="type" class="app-button" :class="`app-button--${variant}`" :disabled="disabled">
    <slot />
  </button>
</template>

<style scoped lang="scss">
// 动作按钮只有这一套。窗口按钮、标签、菜单行不用它——它们的形状是另一回事。
.app-button {
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  padding: var(--space-2) var(--space-4);
  font-size: var(--font-size-2);
  color: var(--text-muted);

  &:hover:not(:disabled) {
    border-color: var(--text-faint);
    color: var(--text);
  }

  &:disabled {
    opacity: 0.4;
  }

  &--primary {
    background: var(--surface-2);
    color: var(--text);
  }

  &--danger {
    border-color: var(--rose);
    color: var(--rose-text);

    &:hover:not(:disabled) {
      border-color: var(--rose);
      color: var(--rose-text);
      background: var(--rose-background);
    }
  }

  &--dashed {
    border-style: dashed;
    color: var(--text-faint);
  }

  &--ghost {
    border: 0;
    padding: 0;
    color: var(--text-faint);

    &:hover:not(:disabled) {
      color: var(--text);
    }
  }
}
</style>
