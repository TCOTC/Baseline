<script setup>
defineProps({
  tone: { type: String, default: 'muted' }, // muted | warning | danger
})
</script>

<template>
  <p class="notice" :class="`notice--${tone}`"><slot /></p>
</template>

<style scoped lang="scss">
.notice {
  font-size: var(--font-size-2);
  color: var(--text-faint);

  // 「有 N 条记录没归到规则」这类提醒：不报错、界面也不会坏，
  // 最容易变成「数字明明不动，却没人知道为什么」——用底色让它自己跳出来。
  &--warning {
    padding: var(--space-3) var(--space-4);
    background: var(--warning-background);
    color: var(--amber-text);
  }

  &--danger {
    padding: var(--space-2) var(--space-3);
    background: var(--rose-background);
    color: var(--rose-text);
  }
}
</style>
