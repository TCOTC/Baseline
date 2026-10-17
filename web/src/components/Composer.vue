<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { api } from '../api.js'
import AppButton from './AppButton.vue'
import ToggleChip from './ToggleChip.vue'

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

const goals = computed(() => (props.composer ? props.composer.goals : []))
const ruleMap = computed(() => (props.composer ? props.composer.rules : {}))
const aiOn = computed(() => !!(props.composer && props.composer.aiOn))
const selected = computed(() => Object.keys(chosen).filter((k) => chosen[k]).map(Number))

const groups = computed(() =>
  selected.value
    .map((gid) => {
      const rule = ruleMap.value[gid]
      if (!rule || !rule.sources || !rule.sources.length) return null
      return { goal: gid, title: rule.title, color: rule.color, auto: rule.auto, list: rule.sources }
    })
    .filter(Boolean),
)

const many = computed(() => groups.value.length > 1)

function missingPick() {
  for (const gid of selected.value) {
    const rule = ruleMap.value[gid]
    if (rule && rule.sources.length > 1 && !picked[gid]) return gid
  }
  return null
}

// 选中目标之后，每个目标都会先按内核算出的 auto 预置一条——
// 能唯一确定的那条直接就是选中的状态，于是它在按下回车前就看得见。
function refreshPicks() {
  for (const key of Object.keys(picked)) {
    if (!chosen[key]) delete picked[key]
  }
  for (const group of groups.value) {
    if (group.auto && !picked[group.goal]) picked[group.goal] = group.auto
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
    if (inputEl.value) inputEl.value.placeholder = '记录刚完成的事'
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
  // 中文输入法选词时的回车不能当成提交：isComposing 在候选框里是真的。
  if (ev.key === 'Enter' && !ev.shiftKey) {
    if (ev.isComposing || ev.keyCode === 229) return
    ev.preventDefault()
    send()
  }
}

async function send() {
  const note = text.value.trim()
  if (!note || busy.value) return
  // 两条规则都能收这条记录时必须先指认，但只在没人接手的时候才拦：
  // AI 能用就放它过去——内核会先落库，界面回头再叫它补判。
  if (missingPick() && !aiOn.value) {
    rulesLit.value = true
    hint('请选择一条判定规则')
    return
  }

  const picks = Object.keys(picked)
    .filter((key) => chosen[key])
    .map((key) => [Number(key), picked[key]])

  busy.value = true
  try {
    const res = await api.checkin(selected.value, note, picks)
    text.value = ''
    grow()
    emit('created', res)
  } catch (e) {
    hint('记录失败：' + e.message)
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
    id="composer"
    v-show="visible"
    class="composer"
    :class="{ 'composer--busy': busy, 'composer--invalid': broke }"
    autocomplete="off"
    @submit.prevent="send"
  >
    <div v-show="menuOpen" id="gmenu" ref="menuEl" class="composer__menu">
      <p class="composer__menu-title">
        选择这条记录推进的目标<span>可多选，也可不选</span>
      </p>
      <button
        v-for="goal in goals"
        :key="goal.id"
        type="button"
        class="composer__option"
        :class="{
          'composer__option--selected': chosen[goal.id],
          'composer__option--noscore': !goal.counts,
        }"
        :data-theme="goal.color"
        @click="toggle(goal.id)"
      >
        <i class="composer__option-dot" />
        <span class="composer__option-label">{{ goal.title }}</span>
        <em v-if="!goal.counts" class="composer__option-note">无手工打卡规则，不计入曲线</em>
      </button>
    </div>

    <!-- 规则气泡在输入框外面、它的上面，左对齐：
         上面是「这条记录算在哪条规则上」，下面是「它推进了哪些目标」。
         不弹浮层——浮层会盖住流水，而这件事需要在打字的时候一直看得见。 -->
    <div
      v-show="groups.length"
      id="rules"
      class="composer__rules"
      :class="{ 'composer__rules--many': many, 'composer__rules--invalid': rulesLit }"
    >
      <div id="rules-list" class="composer__rule-list">
        <template v-for="group in groups" :key="group.goal">
          <span v-if="many" class="composer__rule-group" :data-theme="group.color">{{ group.title }}</span>
          <ToggleChip
            v-for="source in group.list"
            :key="source.id"
            as="button"
            type="button"
            shape="rect"
            :theme="group.color"
            :selected="picked[group.goal] === source.id"
            :title="'规则 #' + source.id + '：' + source.what"
            @click="togglePick(group.goal, source.id)"
          >
            {{ source.what }}
          </ToggleChip>
        </template>
      </div>
    </div>

    <div class="composer__box">
      <textarea
        ref="inputEl"
        id="composer-input"
        v-model="text"
        class="composer__input"
        rows="2"
        maxlength="500"
        placeholder="记录刚完成的事"
        aria-label="记录内容"
        :disabled="busy"
        @input="grow"
        @keydown="onKey"
      />

      <div class="composer__row">
        <div ref="chipsEl" id="gchips" v-show="!overflow" class="composer__chips">
          <ToggleChip
            v-for="goal in goals"
            :key="goal.id"
            as="button"
            type="button"
            dot
            :theme="goal.color"
            :selected="!!chosen[goal.id]"
            :dashed="!goal.counts"
            :title="goal.title + (goal.counts ? '' : '（无手工打卡规则，不计入曲线）')"
            @click="toggle(goal.id)"
          >
            {{ goal.title }}
          </ToggleChip>
        </div>

        <button
          v-show="overflow"
          id="gmore"
          type="button"
          class="composer__more"
          :class="{ 'composer__more--open': menuOpen }"
          @click="menuOpen = !menuOpen"
        >
          …
        </button>

        <AppButton type="submit" class="composer__submit" tabindex="-1">记录</AppButton>
      </div>
    </div>
  </form>
</template>

<style scoped lang="scss">
// 上下两层：上面写文本，下面一行左边是「记到哪个目标」、右边是「记录」。
// 整个框是一个圆角矩形，里面全是无边框元素——边框和焦点态由框负责。
.composer {
  flex: none;
  position: relative;
  // 与流水的文字区对齐；流水自己要多留一条滚动条的位置。
  padding: var(--space-3) calc(var(--page-padding) + var(--page-margin)) var(--space-4) 0;
}

.composer__box {
  display: flex;
  flex-direction: column;
  padding: var(--space-2) var(--space-2) var(--space-2) var(--space-3);
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);

  &:focus-within {
    border-color: var(--blue);
  }
}

.composer__input {
  display: block;
  width: 100%;
  max-height: 160px;
  padding: 2px 0 var(--space-2);
  background: none;
  border: 0;
  outline: none;
  resize: none;
  font-size: var(--font-size-3);
  overflow-y: auto;

  &::placeholder {
    color: var(--text-faint);
  }
}

.composer__row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

// 目标标签：列全所有目标，点一下选中、再点一下取消，可以多选。
// 一行放不下就整排收起来，只留「…」——由 JS 量完宽度决定。
.composer__chips {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: var(--space-1);
  overflow: hidden;

  // 标签在 flex 里不许被压扁。
  :deep(.toggle-chip) {
    flex: none;
    gap: var(--space-2);
  }
}

.composer__more {
  flex: none;
  padding: var(--space-1) var(--space-2);
  border: 1px solid transparent;
  border-radius: var(--radius-pill);
  color: var(--text-faint);

  &:hover {
    border-color: var(--border-strong);
    color: var(--text-muted);
  }

  &--open {
    border-color: var(--goal-border);
    background: var(--surface-2);
    color: var(--text);
  }
}

// 往上弹，不往下：下面是窗口边缘。
.composer__menu {
  position: absolute;
  left: 0;
  bottom: 100%;
  z-index: 20;
  min-width: 230px;
  max-width: 420px;
  margin-bottom: var(--space-2);
  padding: var(--space-2);
  display: flex;
  flex-direction: column;
  gap: 1px;
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
}

.composer__menu-title {
  padding: var(--space-1) var(--space-2);
  font-size: var(--font-size-2);
  color: var(--text-faint);

  span {
    display: block;
  }
}

.composer__option {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 7px 9px;
  text-align: left;

  &:hover {
    background: var(--surface-2);
  }

  &--selected {
    background: var(--surface-2);
    color: var(--text);
    font-weight: bold;
  }
}

.composer__option-dot {
  width: 8px;
  height: 8px;
  flex: none;
  border-radius: 50%;
  background: transparent;
  box-shadow: inset 0 0 0 1.5px var(--border-strong);
}

.composer__option--selected .composer__option-dot {
  background: var(--goal-color);
  box-shadow: none;
}

.composer__option--noscore.composer__option--selected .composer__option-dot {
  background: transparent;
  box-shadow: inset 0 0 0 2px var(--goal-color);
}

.composer__option-label {
  flex: none;
}

.composer__option-note {
  margin-left: auto;
  padding-left: 10px;
  font-style: normal;
  font-size: var(--font-size-1);
  color: var(--text-faint);
  font-weight: normal;
}

// 规则气泡：圆角矩形而不是胶囊：两种气泡长得一样的话，扫一眼分不清哪排是哪排。
.composer__rules {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
  padding: 0 0 var(--space-2);
}

.composer__rule-list {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
}

// 有多个目标时才写目标名，否则它就是噪音。
.composer__rule-group {
  margin-left: 4px;
  font-size: var(--font-size-1);
  font-weight: bold;
  color: var(--goal-text);
}

.composer__rules--invalid :deep(.toggle-chip) {
  border-color: var(--rose);
  color: var(--rose-text);
}

.composer__submit {
  flex: none;
}

.composer--busy .composer__box,
.composer--busy .composer__submit {
  opacity: 0.5;
}

// 提交失败时把框描红——只把按钮变灰会让人以为「点了没反应」。
.composer--invalid .composer__box {
  border-color: var(--rose);
}

.composer--invalid .composer__input::placeholder {
  color: var(--rose-text);
}
</style>

