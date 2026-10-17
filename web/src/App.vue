<script setup>
import { onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { api } from './api.js'
import TitleBar from './components/TitleBar.vue'
import MainView from './components/MainView.vue'
import GoalView from './components/GoalView.vue'
import SettingsView from './components/SettingsView.vue'
import ConfirmModal from './components/ConfirmModal.vue'

const page = ref(null)
const failMsg = ref('')
const aiState = reactive({})
const shell = !!(window.bl && window.bl.shell)

// 路由还是查询串：?goal=N 是详情页，?settings=1 是设置页。
// 和旧版一样，刷新、直接粘链接进来都落在同一屏。
function parseRoute() {
  const q = new URLSearchParams(location.search)
  if (q.get('settings') === '1') return { settings: true }
  const g = Number(q.get('goal'))
  return g > 0 ? { goal: g } : {}
}
const route = ref(parseRoute())

async function load() {
  try {
    page.value = await api.view(route.value)
    if (page.value.title) document.title = page.value.title
    failMsg.value = ''
  } catch (e) {
    failMsg.value = e.message
    console.error('load', e)
  }
}

function nav(url) {
  history.pushState(null, '', url)
  route.value = parseRoute()
  load()
}

function onPop() {
  route.value = parseRoute()
  load()
}

// 记下一条之后：先整体重取（这一刻就看得见），再让 AI 补判。
//
// **顺序不许反过来**：先落库、再判。补判的这几秒里人可能已经在写下一条，
// 所以补判的状态只写在那一条记录上，输入框一概不碰。
async function onCreated(res) {
  await load()
  if (!res || !res.needsAi) return
  const id = res.id
  aiState[id] = { wait: true }
  try {
    const r = await api.classify(id)
    await load()
    if (r && String(r.message || '').indexOf('归了') > -1) {
      delete aiState[id]
    } else {
      // 没判出来是正常结果，不是错误：那面标签本来就写着「·不计入」。
      aiState[id] = { wait: false, note: 'AI 没判出来' }
    }
  } catch (e) {
    aiState[id] = { wait: false, note: 'AI 没判成' }
    console.error('ai-classify', e)
  }
}

onMounted(() => {
  load()
  window.addEventListener('popstate', onPop)
})
onBeforeUnmount(() => window.removeEventListener('popstate', onPop))
</script>

<template>
  <TitleBar v-if="shell" />
  <div class="wrap">
    <template v-if="page">
      <MainView
        v-if="page.view === 'main'"
        :page="page"
        :ai-state="aiState"
        @nav="nav"
        @reload="load"
        @created="onCreated"
      />
      <GoalView
        v-else-if="page.view === 'goal'"
        :page="page"
        :ai-state="aiState"
        @nav="nav"
        @reload="load"
      />
      <SettingsView v-else :page="page" @nav="nav" @reload="load" />
    </template>
    <div v-else class="boot">
      <div v-if="failMsg" class="bad">
        读取失败：{{ failMsg }}<br />
        <button type="button" @click="load">重试</button>
      </div>
      <div v-else>正在读取…</div>
    </div>
  </div>
  <ConfirmModal />
</template>
