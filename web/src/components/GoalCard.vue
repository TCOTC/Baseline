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
      <span class="ct">{{ card.title }}</span>
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

<style scoped lang="scss">
.card {
  background: var(--goal-background);
  border: 1px solid var(--goal-border);
  border-radius: var(--radius-lg);
  padding: var(--space-4);
}

// 卡片整张是个链接，点开进这个目标的详情页。
a.card {
  display: block;

  &:hover {
    border-color: var(--goal-color);
  }

  &:focus-visible {
    outline: 2px solid var(--goal-color);
    outline-offset: 2px;
  }
}

.chead {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--space-2);
}

.ct {
  font-size: var(--font-size-3);
  font-weight: bold;
  color: var(--goal-text);
}

.cd {
  font-size: var(--font-size-2);
  font-weight: bold;
  color: var(--goal-color);
  font-variant-numeric: tabular-nums;

  &.zero {
    color: var(--text-faint);
    font-weight: normal;
  }
}

.vrow {
  margin-bottom: var(--space-3);
}

.v {
  font-size: var(--font-size-7);
  font-weight: bold;
  line-height: 1;
  font-variant-numeric: tabular-nums;

  &.none {
    color: var(--text-faint);
    font-weight: normal;
  }
}

.u {
  margin-left: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-faint);
}

.strip {
  display: flex;
  gap: 2px;
}

.tick {
  flex: 1;
  height: 12px;
  background: var(--surface);

  &.on {
    background: var(--goal-color);
  }
}

.gmeta {
  font-size: var(--font-size-1);
  color: var(--goal-text);
  margin-top: var(--space-2);

  b {
    font-weight: bold;
  }
}
</style>
