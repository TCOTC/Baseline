<script setup>
defineEmits(['nav'])
</script>

<template>
  <a class="back-link" href="/" @click.prevent="$emit('nav', '/')">← 返回主页</a>
</template>

<style scoped lang="scss">
.back-link {
  display: inline-block;
  margin-bottom: var(--space-3);
  font-size: var(--font-size-3);
  color: var(--text-faint);

  &:hover {
    color: var(--text);
    text-decoration: underline;
  }
}
</style>
