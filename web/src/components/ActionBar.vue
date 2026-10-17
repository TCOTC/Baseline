<script setup></script>

<template>
  <div class="action-bar"><slot /></div>
</template>

<style scoped lang="scss">
.action-bar {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  margin-top: var(--space-5);
}
</style>
