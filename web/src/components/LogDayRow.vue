<script setup>
defineProps({
  row: { type: Object, required: true },
})
</script>

<template>
  <div class="log-day">
    <div class="log-day__label" :title="row.title">
      <span class="log-day__date">{{ row.label }}</span>
      <span v-if="row.wd" class="log-day__weekday">{{ row.wd }}</span>
    </div>
  </div>
</template>

<style scoped lang="scss">
// 日期用和记录同一个 46px 时间列，右对齐：扫日期和扫时间是同一个动作。
.log-day {
  height: var(--row-height-day);
  display: grid;
  grid-template-columns: 46px 22px 1fr auto;
  gap: 10px;
  align-items: end;
  padding: 0 0 7px;
}

.log-day__label {
  grid-column: 1 / 3;
  width: max-content;
}

.log-day__date {
  display: inline-block;
  width: 46px;
  text-align: right;
  font-size: var(--font-size-3);
  font-weight: bold;
}

.log-day__weekday {
  margin-left: var(--space-2);
  font-size: var(--font-size-2);
  color: var(--text-faint);
}
</style>
