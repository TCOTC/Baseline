<script setup>
defineProps({
  title: { type: String, required: true },
  hint: { type: String, default: '' },
})
</script>

<template>
  <section class="app-section">
    <h2 class="app-section__title">
      {{ title }}<span v-if="hint" class="app-section__hint">{{ hint }}</span>
    </h2>
    <slot />
  </section>
</template>

<style scoped lang="scss">
.app-section__title {
  margin-bottom: var(--space-2);
  font-size: var(--font-size-3);
  font-weight: bold;
  color: var(--text-muted);
}

.app-section__hint {
  margin-left: var(--space-1);
  font-weight: normal;
  color: var(--text-faint);
}
</style>
