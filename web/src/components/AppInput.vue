<script setup>
import { ref } from 'vue'

defineProps({
  modelValue: { type: [String, Number], default: '' },
  type: { type: String, default: 'text' },
})
defineEmits(['update:modelValue'])

const input = ref(null)
defineExpose({ focus: () => input.value && input.value.focus() })
</script>

<template>
  <input
    ref="input"
    class="app-input"
    :type="type"
    :value="modelValue"
    @input="$emit('update:modelValue', $event.target.value)"
  />
</template>

<style scoped lang="scss">
.app-input {
  width: 100%;
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  padding: var(--space-2) var(--space-3);
  font-size: var(--font-size-3);
  outline: none;

  &:focus {
    border-color: var(--goal-color, var(--blue));
  }
}
</style>
