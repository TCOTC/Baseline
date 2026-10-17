<script setup>
defineProps({
  curve: { type: Object, required: true },
})
</script>

<template>
  <svg
    v-if="curve.kind === 'line'"
    class="curve"
    viewBox="0 0 300 44"
    preserveAspectRatio="none"
    xmlns="http://www.w3.org/2000/svg"
  >
    <title>{{ curve.range }}</title>
    <line class="curve__baseline" x1="0" :y1="curve.baseY" x2="300" :y2="curve.baseY" />
    <path class="curve__line" :d="curve.path" />
  </svg>

  <p v-else class="curve curve--empty" :class="{ 'curve--unwired': curve.kind === 'unwired' }">
    {{ curve.kind === 'unwired' ? '规则还没接线 —— 这条线现在算不出数' : '这条线还没有任何记录' }}
  </p>
</template>

<style scoped lang="scss">
// 阶梯线，不插值：数据是离散累积的，平滑会伪造出一个从未发生过的渐进过程。
.curve {
  display: block;
  width: 100%;
  height: 44px;
}

.curve__line {
  fill: none;
  stroke: var(--goal-color);
  stroke-width: 2;
  stroke-linejoin: round;
  vector-effect: non-scaling-stroke;
}

.curve__baseline {
  stroke: var(--goal-border);
  stroke-width: 1;
  vector-effect: non-scaling-stroke;
}

.curve--empty {
  display: flex;
  align-items: center;
  font-size: var(--font-size-2);
  color: var(--text-faint);
  border-bottom: 1px dashed var(--goal-border);
}

// 「规则还没接线」和「还没有记录」不是一件事，颜色也不同。
.curve--unwired {
  color: var(--amber-text);
  border-bottom-color: var(--amber-border);
}
</style>
