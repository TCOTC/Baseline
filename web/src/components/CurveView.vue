<script setup>
// 阶梯线。**不做插值**——数据是离散累积的，平滑会伪造出一个从未发生过的渐进过程。
//
// path 由 Go 算好（和数字同一趟），这里只负责画：
// 曲线怎么画是「数字的另一种说法」，两边各算一次迟早对不上。
defineProps({
  curve: { type: Object, required: true },
})
</script>

<template>
  <svg
    v-if="curve.kind === 'line'"
    class="cv"
    viewBox="0 0 300 44"
    preserveAspectRatio="none"
    xmlns="http://www.w3.org/2000/svg"
  >
    <title>{{ curve.range }}</title>
    <line class="cv-base" x1="0" :y1="curve.baseY" x2="300" :y2="curve.baseY" />
    <path class="cv-line" :d="curve.path" />
  </svg>
  <div v-else-if="curve.kind === 'unwired'" class="cempty unwired">
    规则还没接线 —— 这条线现在算不出数
  </div>
  <div v-else class="cempty">这条线还没有任何记录</div>
</template>

<style scoped lang="scss">
.cv {
  width: 100%;
  height: 44px;
  display: block;
  margin-bottom: var(--space-3);
}

.cv-line {
  fill: none;
  stroke: var(--goal-color);
  stroke-width: 2;
  stroke-linejoin: round;
  vector-effect: non-scaling-stroke;
}

.cv-base {
  stroke: var(--goal-border);
  stroke-width: 1;
  vector-effect: non-scaling-stroke;
}

.cempty {
  height: 44px;
  margin-bottom: var(--space-3);
  display: flex;
  align-items: center;
  font-size: var(--font-size-2);
  color: var(--text-faint);
  border-bottom: 1px dashed var(--goal-border);

  // 「规则还没接线」和「还没有记录」用的是同一个位置，但说的不是一件事：
  // 前者那条线现在算不出数，后者只是还没动。所以颜色也不一样。
  &.unwired {
    color: var(--amber-text);
    border-bottom-color: var(--amber-border);
  }
}
</style>
