<script setup>
import { onMounted, onBeforeUnmount, ref } from 'vue'

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
  <div class="bar">
    <div class="drag"></div>
    <button id="w-min" type="button" class="wbtn" tabindex="-1" aria-label="最小化" @click="bl.minimize()">
      &#xE921;
    </button>
    <button id="w-max" type="button" class="wbtn" tabindex="-1" aria-label="最大化" @click="bl.toggleMaximize()">
      {{ maxed ? '\uE923' : '\uE922' }}
    </button>
    <button id="w-close" type="button" class="wbtn close" tabindex="-1" aria-label="关闭" @click="bl.close()">
      &#xE8BB;
    </button>
  </div>
</template>

<style scoped lang="scss">
// 顶栏就是窗口的边框：横跨整个宽度，里面不放产品名、不放日期。
// 尺寸和颜色是照 Windows 自己的标题栏量出来的：按钮 46×40、图标 10px。
// 拖动交给 Chromium 的 app-region，双击最大化、Aero Snap 跟着系统走。
.bar {
  flex: none;
  height: 40px;
  display: flex;
  align-items: stretch;
  user-select: none;
  -webkit-app-region: drag;
}

.bar .drag {
  flex: 1;
  -webkit-app-region: drag;
}

.wbtn {
  flex: none;
  width: 46px;
  display: grid;
  place-items: center;
  border: 0;
  padding: 0 0 3px;
  font-family: var(--font-icon);
  font-size: var(--font-size-1);
  line-height: 1;
  color: var(--caption-ink);
  outline: none;
  -webkit-app-region: no-drag;
  // 关掉次像素抗锯齿。Windows 上的 Chromium 默认给文字上 LCD 次像素，
  // 图标边缘会泛蓝泛橙；系统标题栏的图标是灰度的。提到独立合成层才真的关掉
  // （透明背底的合成层不允许 LCD 文字）——-webkit-font-smoothing 在这里不起作用。
  transform: translateZ(0);

  &:hover {
    background: var(--caption-hover);
  }

  &:active {
    background: var(--caption-press);
  }

  &.close:hover {
    background: var(--caption-close);
    color: var(--caption-close-ink);
  }

  &.close:active {
    background: var(--caption-close-press);
    color: var(--caption-close-ink);
  }
}
</style>
