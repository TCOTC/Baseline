<script setup>
import CurveView from './CurveView.vue'

defineProps({
  card: { type: Object, required: true },
})
defineEmits(['nav'])
</script>

<template>
  <a
    class="goal-card"
    :data-theme="card.color"
    :href="'?goal=' + card.id"
    @click.prevent="$emit('nav', '?goal=' + card.id)"
  >
    <header class="goal-card__header">
      <h2 class="goal-card__title">{{ card.title }}</h2>
      <span class="goal-card__delta" :class="{ 'goal-card__delta--zero': card.zero }">
        {{ card.deltaText }}
      </span>
    </header>

    <p class="goal-card__value-row">
      <strong class="goal-card__value" :class="{ 'goal-card__value--zero': card.zero }">
        {{ card.currentText }}
      </strong>
      <span class="goal-card__unit">次</span>
    </p>

    <CurveView class="goal-card__curve" :curve="card.curve" />

    <div class="goal-card__strip" aria-hidden="true">
      <span
        v-for="(on, index) in card.strip"
        :key="index"
        class="goal-card__tick"
        :class="{ 'goal-card__tick--on': on }"
      />
    </div>

    <p class="goal-card__rule" :class="{ 'goal-card__rule--missing': card.rule.empty }">
      <template v-if="card.rule.empty">
        <b>没有判定规则</b> —— 这个目标给不出「什么算推进它」
      </template>
      <template v-else>{{ card.rule.text }}</template>
    </p>
  </a>
</template>

<style scoped lang="scss">
// 卡片整张是个链接，点开进这个目标的详情页。
.goal-card {
  display: block;
  padding: var(--space-4);
  background: var(--goal-background);
  border: 1px solid var(--goal-border);
  border-radius: var(--radius-lg);

  &:hover {
    border-color: var(--goal-color);
  }

  &:focus-visible {
    outline: 2px solid var(--goal-color);
    outline-offset: 2px;
  }
}

.goal-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--space-2);
}

.goal-card__title {
  font-size: var(--font-size-3);
  font-weight: bold;
  color: var(--goal-text);
}

.goal-card__delta {
  font-size: var(--font-size-2);
  font-weight: bold;
  color: var(--goal-color);
  font-variant-numeric: tabular-nums;

  &--zero {
    color: var(--text-faint);
    font-weight: normal;
  }
}

.goal-card__value-row {
  margin-bottom: var(--space-3);
}

.goal-card__value {
  font-size: var(--font-size-7);
  font-weight: bold;
  line-height: 1;
  font-variant-numeric: tabular-nums;

  &--zero {
    color: var(--text-faint);
    font-weight: normal;
  }
}

.goal-card__unit {
  margin-left: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-faint);
}

.goal-card__curve {
  margin-bottom: var(--space-3);
}

// 30 格条纹：每天有没有比前一天多。方块，不加圆角。
.goal-card__strip {
  display: flex;
  gap: 2px;
}

.goal-card__tick {
  flex: 1;
  height: 12px;
  background: var(--surface);

  &--on {
    background: var(--goal-color);
  }
}

.goal-card__rule {
  margin-top: var(--space-2);
  font-size: var(--font-size-1);
  color: var(--goal-text);

  b {
    font-weight: bold;
  }
}
</style>
