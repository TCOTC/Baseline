<script setup>
import { SOURCE_KINDS } from '../kinds.js'

defineProps({
  modelValue: { type: String, default: '' },
})
defineEmits(['update:modelValue'])
</script>

<template>
  <div class="source-kind-picker">
    <button
      v-for="kind in SOURCE_KINDS"
      :key="kind.value"
      type="button"
      class="source-kind-picker__option"
      :class="{ 'source-kind-picker__option--selected': modelValue === kind.value }"
      @click="$emit('update:modelValue', kind.value)"
    >
      <strong class="source-kind-picker__label">{{ kind.label }}</strong>
      <span class="source-kind-picker__description">{{ kind.description }}</span>
    </button>
  </div>
</template>

<style scoped lang="scss">
.source-kind-picker__option {
  display: block;
  width: 100%;
  margin-bottom: var(--space-2);
  padding: var(--space-3);
  text-align: left;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);

  &:hover {
    border-color: var(--text-faint);
  }

  &--selected {
    border-color: var(--text-faint);
    background: var(--surface-2);
  }
}

.source-kind-picker__label {
  display: block;
  margin-bottom: 2px;
  font-size: var(--font-size-3);
  font-weight: bold;
  color: var(--text);
}

.source-kind-picker__description {
  font-size: var(--font-size-2);
  color: var(--text-faint);
}
</style>
