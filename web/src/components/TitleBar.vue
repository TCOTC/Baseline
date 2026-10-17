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
