<script setup>
import { onBeforeUnmount, onMounted, ref } from 'vue'

// 顶栏图标是 Segoe Fluent Icons 的字符，不是画的 SVG：
// E921 最小化 / E922 最大化 / E923 还原 / E8BB 关闭，就是 Windows 标题栏自己用的字形。
const bl = window.bl
const maxed = ref(false)
let off = null

function sync() {
  bl.isMaximized().then((v) => (maxed.value = v))
}

onMounted(() => {
  sync()
  off = bl.onMaximized(sync)
})
onBeforeUnmount(() => {
  if (off) off()
})
</script>

<template>
  <header class="title-bar">
    <div class="title-bar__drag"></div>
    <button id="w-min" type="button" class="title-bar__button" tabindex="-1" aria-label="最小化" @click="bl.minimize()">
      &#xE921;
    </button>
    <button id="w-max" type="button" class="title-bar__button" tabindex="-1" aria-label="最大化" @click="bl.toggleMaximize()">
      {{ maxed ? '\uE923' : '\uE922' }}
    </button>
    <button
      id="w-close"
      type="button"
      class="title-bar__button title-bar__button--close"
      tabindex="-1"
      aria-label="关闭"
      @click="bl.close()"
    >
      &#xE8BB;
    </button>
  </header>
</template>

<style scoped lang="scss">
// 顶栏就是窗口的边框：横跨整个宽度，里面不放产品名、不放日期。
// 尺寸和颜色照 Windows 自己的标题栏量出来：按钮 46×40、图标 10px。
.title-bar {
  flex: none;
  height: 40px;
  display: flex;
  align-items: stretch;
  user-select: none;
  -webkit-app-region: drag;
}

.title-bar__drag {
  flex: 1;
}

.title-bar__button {
  flex: none;
  width: 46px;
  display: grid;
  place-items: center;
  padding: 0 0 3px;
  font-family: var(--font-icon);
  font-size: var(--font-size-1);
  line-height: 1;
  color: var(--caption-ink);
  outline: none;
  -webkit-app-region: no-drag;
  // 关掉次像素抗锯齿：图标边缘泛蓝泛橙，和系统标题栏摆一起一眼就不像一家的。
  // 提到独立合成层才真的关掉（透明背底的合成层不允许 LCD 文字）。
  transform: translateZ(0);

  &:hover {
    background: var(--caption-hover);
  }

  &:active {
    background: var(--caption-press);
  }

  &--close:hover {
    background: var(--caption-close);
    color: var(--caption-close-ink);
  }

  &--close:active {
    background: var(--caption-close-press);
    color: var(--caption-close-ink);
  }
}
</style>
