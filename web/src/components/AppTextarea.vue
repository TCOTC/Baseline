<script setup>
import { ref } from 'vue'

defineProps({
  modelValue: { type: String, default: '' },
  rows: { type: Number, default: 2 },
})
defineEmits(['update:modelValue'])

const textarea = ref(null)
defineExpose({ focus: () => textarea.value && textarea.value.focus() })
</script>

<template>
  <textarea
    ref="textarea"
    class="app-textarea"
    :rows="rows"
    :value="modelValue"
    @input="$emit('update:modelValue', $event.target.value)"
  />
</template>

<style scoped lang="scss">
.app-textarea {
  width: 100%;
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  padding: var(--space-2) var(--space-3);
  font-size: var(--font-size-3);
  outline: none;
  resize: none;

  &:focus {
    border-color: var(--goal-color, var(--blue));
  }
}
</style>
