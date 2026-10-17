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
