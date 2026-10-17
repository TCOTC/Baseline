<script setup>
defineProps({
  theme: { type: String, default: 'none' },
})
</script>

<template>
  <div class="page" :data-theme="theme">
    <header class="page__hero"><slot name="hero" /></header>
    <div class="page__body"><slot /></div>
  </div>
</template>

<style scoped lang="scss">
.page {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.page__hero {
  flex: none;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  padding: var(--space-4) var(--page-padding);
}

.page__body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
</style>
