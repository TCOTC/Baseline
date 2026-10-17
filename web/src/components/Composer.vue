<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { api } from '../api.js'

const props = defineProps({
  composer: { type: Object, default: null },
  visible: { type: Boolean, default: true },
})
const emit = defineEmits(['created'])

const text = ref('')
const chosen = reactive({})
const picked = reactive({})
const menuOpen = ref(false)
const busy = ref(false)
const broke = ref(false)
const rulesLit = ref(false)
const overflow = ref(false)

const inputEl = ref(null)
const chipsEl = ref(null)
const menuEl = ref(null)
const formEl = ref(null)

const goals = computed(() => (props.composer ? props.composer.goals : []))
const ruleMap = computed(() => (props.composer ? props.composer.rules : {}))
const aiOn = computed(() => !!(props.composer && props.composer.aiOn))
const selected = computed(() => Object.keys(chosen).filter((k) => chosen[k]).map(Number))

const groups = computed(() =>
  selected.value
    .map((gid) => {
      const r = ruleMap.value[gid]
      if (!r || !r.sources || !r.sources.length) return null
      return { goal: gid, title: r.title, color: r.color, auto: r.auto, list: r.sources }
    })
    .filter(Boolean),
)

const many = computed(() => groups.value.length > 1)

function missingPick() {
  for (const gid of selected.value) {
    const r = ruleMap.value[gid]
    if (r && r.sources.length > 1 && !picked[gid]) return gid
  }
  return null
}

// 选中目标之后，每个目标都会先按内核算出的 auto 预置一条——
// **能唯一确定的那条直接就是选中的状态**，于是它在按下回车前就看得见。
function refreshPicks() {
  for (const k of Object.keys(picked)) {
    if (!chosen[k]) delete picked[k]
  }
  for (const g of groups.value) {
    if (g.auto && !picked[g.goal]) picked[g.goal] = g.auto
  }
  if (!missingPick()) rulesLit.value = false
}

function toggle(id) {
  if (!id) return
  if (chosen[id]) delete chosen[id]
  else chosen[id] = true
  refreshPicks()
  nextTick(fitChips)
  focus()
}

function togglePick(goalId, sourceId) {
  // 一个目标下只能算一条规则，所以这里不是多选：点另一条就换过去。
  // 再点当前这条 = 取消（「这条不算它」），留一条退路。
  if (picked[goalId] === sourceId) delete picked[goalId]
  else picked[goalId] = sourceId
  if (!missingPick()) rulesLit.value = false
  focus()
}

// 一行放不下就整排收起来，只留一个「…」。用真实的布局宽度判断，
// 不靠猜字符数——目标名有长有短，猜不准。
function fitChips() {
  if (!chipsEl.value) return
  overflow.value = false
  nextTick(() => {
    const el = chipsEl.value
    if (el && el.scrollWidth > el.clientWidth + 1) overflow.value = true
  })
}

function grow() {
  const el = inputEl.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = Math.min(el.scrollHeight, 160) + 'px'
}

let hintTimer = 0
function hint(msg) {
  if (!inputEl.value) return
  broke.value = true
  inputEl.value.placeholder = msg
  clearTimeout(hintTimer)
  hintTimer = setTimeout(() => {
    broke.value = false
    if (inputEl.value) inputEl.value.placeholder = '刚做了什么？'
    grow()
  }, 4000)
}

function focus() {
  const el = inputEl.value
  if (!el || el.disabled) return
  try {
    el.focus()
  } catch {
    /* 忽略 */
  }
}

function onKey(ev) {
  // 中文输入法选词时的回车不能当成提交：`isComposing` 在候选框里是真的。
  if (ev.key === 'Enter' && !ev.shiftKey) {
    if (ev.isComposing || ev.keyCode === 229) return
    ev.preventDefault()
    send()
  }
}

async function send() {
  const note = text.value.trim()
  if (!note || busy.value) return
  // 两条规则都能收这条记录时必须先指认，但**只在没人接手的时候**才拦：
  // AI 能用就放它过去——内核会先落库，界面回头再叫它补判。
  if (missingPick() && !aiOn.value) {
    rulesLit.value = true
    hint('选一条规则再记')
    return
  }

  const picks = Object.keys(picked)
    .filter((k) => chosen[k])
    .map((k) => [Number(k), picked[k]])

  busy.value = true
  try {
    const res = await api.checkin(selected.value, note, picks)
    text.value = ''
    grow()
    emit('created', res)
  } catch (e) {
    hint('没记上：' + e.message)
  } finally {
    busy.value = false
    focus()
  }
}

function onDocClick(ev) {
  if (!menuOpen.value) return
  // 点「…」本身是切换，别在这一轮里又被关掉。
  if (ev.target && ev.target.closest && ev.target.closest('#gmore')) return
  if (menuEl.value && menuEl.value.contains(ev.target)) return
  if (chipsEl.value && chipsEl.value.contains(ev.target)) return
  menuOpen.value = false
}

function onDocKey(ev) {
  if (ev.key === 'Escape') menuOpen.value = false
}

function onGlobalFocus() {
  focus()
}

onMounted(() => {
  window.__blFocusComposer = onGlobalFocus
  focus()
  grow()
  nextTick(fitChips)
  document.addEventListener('click', onDocClick)
  document.addEventListener('keydown', onDocKey)
  window.addEventListener('resize', fitChips)
})

onBeforeUnmount(() => {
  clearTimeout(hintTimer)
  if (window.__blFocusComposer === onGlobalFocus) delete window.__blFocusComposer
  document.removeEventListener('click', onDocClick)
  document.removeEventListener('keydown', onDocKey)
  window.removeEventListener('resize', fitChips)
})

watch(
  () => props.composer,
  () => {
    refreshPicks()
    nextTick(fitChips)
  },
)
</script>

<template>
  <form
    v-if="composer"
    ref="formEl"
    id="composer"
    v-show="visible"
    class="composer"
    :class="{ busy, broke }"
    autocomplete="off"
    @submit.prevent="send"
  >
    <div v-show="menuOpen" id="gmenu" ref="menuEl" class="gmenu">
      <div class="gmenu-h">这条记录推进了哪些目标？<span>可以多选，也可以一个都不选</span></div>
      <button
        v-for="g in goals"
        :key="g.id"
        type="button"
        class="gopt"
        :class="[g.color, { on: chosen[g.id], noscore: !g.counts }]"
        @click="toggle(g.id)"
      >
        <i></i><span class="gn">{{ g.title }}</span>
        <em v-if="!g.counts">规则里没有手工打卡，记了也不动这条线</em>
      </button>
    </div>

    <!-- 规则气泡在输入框**外面**、它的上面，左对齐，和下面那排目标气泡分成两层：
         上面是「这条记录算在哪条规则上」，下面是「它推进了哪些目标」。
         不弹浮层——浮层会盖住流水，而这件事需要在打字的时候一直看得见。 -->
    <div v-show="groups.length" id="rules" class="rules" :class="{ many, broke: rulesLit }">
      <div id="rules-list" class="rules-list">
        <template v-for="g in groups" :key="g.goal">
          <span v-if="many" class="rgroup" :class="g.color">{{ g.title }}</span>
          <button
            v-for="s in g.list"
            :key="s.id"
            type="button"
            class="rchip"
            :class="[g.color, { on: picked[g.goal] === s.id }]"
            :title="'规则 #' + s.id + '：' + s.what"
            @click="togglePick(g.goal, s.id)"
          >
            {{ s.what }}
          </button>
        </template>
      </div>
    </div>

    <div class="box">
      <textarea
        ref="inputEl"
        id="composer-input"
        v-model="text"
        rows="2"
        maxlength="500"
        placeholder="刚做了什么？"
        aria-label="记一条"
        :disabled="busy"
        @input="grow"
        @keydown="onKey"
      ></textarea>
      <div class="crow">
        <div ref="chipsEl" id="gchips" v-show="!overflow" class="gchips">
          <button
            v-for="g in goals"
            :key="g.id"
            type="button"
            class="gchip"
            :class="[g.color, { on: chosen[g.id], noscore: !g.counts }]"
            :title="g.title + (g.counts ? '' : '（规则里没有手工打卡，记了也不动这条线）')"
            @click="toggle(g.id)"
          >
            <i></i>{{ g.title }}
          </button>
        </div>
        <button v-show="overflow" id="gmore" type="button" class="gmore" :class="{ on: menuOpen }" @click="menuOpen = !menuOpen">
          …
        </button>
        <button type="submit" class="send" tabindex="-1">记下</button>
      </div>
    </div>
  </form>
</template>
