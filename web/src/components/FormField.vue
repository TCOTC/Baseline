<script setup>
defineProps({
  label: { type: String, required: true },
  forId: { type: String, default: '' },
  hint: { type: String, default: '' },
  last: { type: Boolean, default: false },
})
</script>

<template>
  <div class="form-field" :class="{ 'form-field--last': last }">
    <label class="form-field__label" :for="forId || undefined">{{ label }}</label>
    <slot />
    <p v-if="hint" class="form-field__hint">{{ hint }}</p>
  </div>
</template>

<style scoped lang="scss">
.form-field {
  margin-bottom: var(--space-3);

  &--last {
    margin-bottom: 0;
  }
}

.form-field__label {
  display: block;
  margin-bottom: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-faint);
}

.form-field__hint {
  margin-top: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-faint);
}
</style>
