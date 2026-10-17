<script setup>
import { onBeforeUnmount, onMounted } from 'vue'
import { modal, resolveModal } from '../ui.js'

function close(v) {
  resolveModal(v)
}

function onKey(ev) {
  if (ev.key === 'Escape') close(false)
}

onMounted(() => document.addEventListener('keydown', onKey))
onBeforeUnmount(() => document.removeEventListener('keydown', onKey))
</script>

<template>
  <div v-if="modal.open" id="modal" class="modal" @click.self="close(false)">
    <div class="mbox" role="dialog" aria-modal="true" aria-labelledby="m-title">
      <h3 id="m-title">{{ modal.title }}</h3>
      <p id="m-text">{{ modal.text }}</p>
      <div v-if="modal.what" id="m-what" class="mwhat">{{ modal.what }}</div>
      <div class="macts">
        <button id="m-cancel" type="button" @click="close(false)">取消</button>
        <button id="m-ok" type="button" :class="{ danger: modal.danger }" @click="close(true)">
          {{ modal.ok }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped lang="scss">
// 危险动作（删规则、删目标）一律走这里：把「要删的是什么」摆在眼前，
// 代价是打断一次——删除值得这一次打断。
.modal {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(28, 26, 23, 0.32);
}

.mbox {
  width: min(430px, calc(100vw - var(--page-padding) * 2));
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-lg);
  padding: var(--space-5);

  h3 {
    font-size: var(--font-size-5);
    font-weight: bold;
    margin-bottom: 8px;
  }

  p {
    font-size: var(--font-size-3);
    color: var(--text-muted);
  }

  .mwhat {
    margin-top: 12px;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    background: var(--surface-2);
    font-size: var(--font-size-3);
    color: var(--text-muted);
    word-break: break-word;
  }
}

.macts {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  margin-top: var(--space-4);

  button {
    &:hover {
      border-color: var(--text-faint);
      color: var(--text);
    }

    &.danger {
      border-color: var(--rose);
      color: var(--rose-text);

      &:hover {
        background: var(--rose-background);
      }
    }
  }
}
</style>
