<script setup>
import { onBeforeUnmount, onMounted } from 'vue'
import { modal, resolveModal } from '../ui.js'
import AppButton from './AppButton.vue'

function close(value) {
  resolveModal(value)
}

function onKey(event) {
  if (event.key === 'Escape') close(false)
}

onMounted(() => document.addEventListener('keydown', onKey))
onBeforeUnmount(() => document.removeEventListener('keydown', onKey))
</script>

<template>
  <div v-if="modal.open" id="modal" class="confirm-modal" @click.self="close(false)">
    <div
      class="confirm-modal__box"
      role="dialog"
      aria-modal="true"
      aria-labelledby="confirm-modal-title"
    >
      <h2 id="confirm-modal-title" class="confirm-modal__title">{{ modal.title }}</h2>
      <p class="confirm-modal__text">{{ modal.text }}</p>
      <p v-if="modal.what" class="confirm-modal__what">{{ modal.what }}</p>

      <div class="confirm-modal__actions">
        <AppButton @click="close(false)">取消</AppButton>
        <AppButton :variant="modal.danger ? 'danger' : 'primary'" @click="close(true)">
          {{ modal.ok }}
        </AppButton>
      </div>
    </div>
  </div>
</template>

<style scoped lang="scss">
.confirm-modal {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(28, 26, 23, 0.32);
}

.confirm-modal__box {
  width: min(430px, calc(100vw - var(--page-padding) * 2));
  padding: var(--space-5);
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-lg);
}

.confirm-modal__title {
  margin-bottom: var(--space-2);
  font-size: var(--font-size-5);
  font-weight: bold;
}

.confirm-modal__text {
  color: var(--text-muted);
  font-size: var(--font-size-3);
}

.confirm-modal__what {
  margin-top: var(--space-3);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius);
  background: var(--surface-2);
  color: var(--text-muted);
  font-size: var(--font-size-2);
  word-break: break-word;
}

.confirm-modal__actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  margin-top: var(--space-4);
}
</style>
