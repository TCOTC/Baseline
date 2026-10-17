<script setup>
import CurveView from './CurveView.vue'

defineProps({
  card: { type: Object, required: true },
})
defineEmits(['nav'])
</script>

<template>
  <a class="card" :class="card.color" :href="'?goal=' + card.id" @click.prevent="$emit('nav', '?goal=' + card.id)">
    <div class="chead">
      <span class="ct"><i></i>{{ card.title }}</span>
      <span :class="card.zero ? 'cd zero' : 'cd'">{{ card.deltaText }}</span>
    </div>
    <div class="vrow">
      <span :class="card.zero ? 'v none' : 'v'">{{ card.currentText }}</span><span class="u">次</span>
    </div>
    <CurveView :curve="card.curve" />
    <div class="strip">
      <span v-for="(on, i) in card.strip" :key="i" class="tick" :class="{ on }"></span>
    </div>
    <div v-if="card.rule.empty" class="gmeta">
      <b>没有判定规则</b> —— 这个目标给不出「什么算推进它」
    </div>
    <div v-else class="gmeta">{{ card.rule.text }}</div>
  </a>
</template>
