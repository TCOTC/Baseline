<script setup>
defineProps({
  title: { type: String, required: true },
  hint: { type: String, default: '' },
})
</script>

<template>
  <section class="settings-group">
    <h3 class="settings-group__title">
      {{ title }}<span v-if="hint" class="settings-group__hint">{{ hint }}</span>
    </h3>
    <div class="settings-group__body"><slot /></div>
  </section>
</template>

<style scoped lang="scss">
// 第二层：一组相关设置。卡片边界把「哪几项是一回事」画出来，
// 不然一串 FormField 从上排到下，看不出分隔。
.settings-group {
  padding: var(--space-4) var(--space-5);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
}

.settings-group__title {
  font-size: var(--font-size-3);
  font-weight: bold;
}

.settings-group__hint {
  margin-left: var(--space-2);
  font-size: var(--font-size-2);
  font-weight: normal;
  color: var(--text-faint);
}

.settings-group__body {
  margin-top: var(--space-3);
}
</style>
