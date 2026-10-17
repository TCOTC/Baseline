<script setup>
import { computed, nextTick, ref, watch } from 'vue'
import { api } from '../api.js'
import { findKind } from '../kinds.js'
import AppButton from './AppButton.vue'
import AppInput from './AppInput.vue'
import Notice from './Notice.vue'
import SourceKindPicker from './SourceKindPicker.vue'

// 新建目标是四步固定骨架 + 一步收口，不是自由聊天。
// 固定骨架解决的是「自由聊天只能靠 prompt 求它自觉」：
// 步数就是步数，而「什么不算」这一问永远不会被跳过。
const emit = defineEmits(['close', 'created'])

const STEP_NAMES = ['想推进什么', '为什么是现在', '什么算推进它', '什么不算', '落到哪种来源']
const FIELDS = ['title', 'why', 'rule', 'notrule']

const d = ref({
  step: 0,
  title: '',
  why: '',
  rule: '',
  notrule: '',
  kind: '',
  target: '',
  problem: '',
  value: 0,
  valueText: '0',
})
const inputVal = ref('')
const askingTarget = ref(false)
const busy = ref(false)
const inputEl = ref(null)

const kindSpec = computed(() => findKind(d.value.kind))

function focusInput() {
  const go = () => {
    if (inputEl.value && document.activeElement !== inputEl.value) inputEl.value.focus()
  }
  // 写完 DOM 立刻 focus 会被静默忽略——不报错，只是没生效。
  nextTick(() => {
    requestAnimationFrame(go)
    setTimeout(go, 80)
  })
}

watch(
  () => [d.value.step, askingTarget.value],
  () => {
    if (askingTarget.value) {
      inputVal.value = d.value.target
    } else if (d.value.step < 4) {
      inputVal.value = d.value[FIELDS[d.value.step]] || ''
    }
    if (d.value.step <= 3 || askingTarget.value) focusInput()
  },
  { immediate: true },
)

function nav() {
  d.value.problem = ''
  d.value.step += 1
}

function next() {
  const value = inputVal.value.trim()
  if (askingTarget.value) {
    takeTarget()
    return
  }
  switch (d.value.step) {
    case 0:
      if (!value) {
        d.value.problem = '总得先有个名字。'
        return
      }
      d.value.title = value
      break
    case 1:
      d.value.why = value // 可以不填：不是每个目标都需要问目的
      break
    case 2:
      if (!value) {
        // 这是「必须再问」的落点：收不出一条可计算的规则，就不该往下走。
        d.value.problem =
          '这条填不了。没有它，这个目标给不出「什么算推进它」——曲线永远不会动，它也就不是目标，只是一句愿望。'
        return
      }
      d.value.rule = value
      break
    case 3:
      d.value.notrule = value
      break
    default:
      return
  }
  nav()
}

function pickKind(value) {
  const spec = findKind(value)
  d.value.kind = value
  d.value.problem = ''
  if (spec.needsTarget) {
    // 需要 target 的来源：先问清对象。空着就等于「什么都算」。
    askingTarget.value = true
    return
  }
  d.value.target = ''
  d.value.step = 5
}

function takeTarget() {
  const value = inputVal.value.trim()
  if (!value) {
    d.value.problem =
      '空着就等于「什么都算」——那条规则会把所有活动都装进来，曲线永远在涨，也就不告诉你偏了。'
    askingTarget.value = true
    focusInput()
    return
  }
  d.value.target = value
  askingTarget.value = false
  d.value.problem = ''
  d.value.step = 5
}

function back() {
  if (askingTarget.value) {
    askingTarget.value = false
    d.value.problem = ''
    return
  }
  if (d.value.step > 0) {
    d.value.problem = ''
    d.value.step -= 1
  }
}

function skipBoundary() {
  d.value.notrule = ''
  d.value.step = 4
}

async function create() {
  if (busy.value) return
  busy.value = true
  try {
    const res = await api.addGoal({
      title: d.value.title,
      why: d.value.why,
      rationale: d.value.rule,
      kind: d.value.kind,
      target: d.value.target,
    })
    d.value.value = res.value
    d.value.valueText = res.valueText
    d.value.problem = ''
    d.value.step = 6
  } catch (e) {
    d.value.problem = '没建成：' + e.message
  } finally {
    busy.value = false
  }
}

function onKey(event) {
  if (event.key !== 'Enter' || event.isComposing || event.keyCode === 229) return
  const tag = event.target && event.target.tagName
  if (tag !== 'INPUT') return
  event.preventDefault()
  if (d.value.step === 5) create()
  else next()
}
</script>

<template>
  <section id="dlg" class="add-goal-dialog" @keydown="onKey">
    <div class="add-goal-dialog__steps">
      <div
        v-for="(name, index) in STEP_NAMES"
        :key="name"
        class="add-goal-dialog__step"
        :class="{
          'add-goal-dialog__step--done': index < d.step,
          'add-goal-dialog__step--current': index === d.step,
        }"
      >
        {{ index + 1 }} · {{ name }}
      </div>
    </div>

    <template v-if="askingTarget">
      <h2 class="add-goal-dialog__title">{{ kindSpec.label }} · 具体是哪个？</h2>
      <p class="add-goal-dialog__note">{{ kindSpec.targetPrompt }}</p>
      <AppInput ref="inputEl" v-model="inputVal" type="text" maxlength="120" />
      <div class="add-goal-dialog__actions">
        <AppButton variant="primary" @click="takeTarget">下一步</AppButton>
        <AppButton @click="back">上一步</AppButton>
        <AppButton @click="emit('close')">取消</AppButton>
      </div>
      <Notice v-if="d.problem" tone="danger" class="add-goal-dialog__problem">{{ d.problem }}</Notice>
    </template>

    <template v-else-if="d.step === 0">
      <h2 class="add-goal-dialog__title">你想推进什么？</h2>
      <p class="add-goal-dialog__note">一句话就行。它会是左栏那张卡的标题。</p>
      <AppInput
        ref="inputEl"
        v-model="inputVal"
        type="text"
        maxlength="40"
        placeholder="比如：打牢计算机基础"
      />
      <div class="add-goal-dialog__actions">
        <AppButton variant="primary" @click="next">下一步</AppButton>
        <AppButton @click="emit('close')">取消</AppButton>
      </div>
      <Notice v-if="d.problem" tone="danger" class="add-goal-dialog__problem">{{ d.problem }}</Notice>
    </template>

    <template v-else-if="d.step === 1">
      <h2 class="add-goal-dialog__title">为什么是现在？</h2>
      <p class="add-goal-dialog__note">
        遇到什么事了，还是在准备什么？<br />
        这一问是为了推出「什么算推进它」，<b>不是问你感受</b>——所以只有在你上一句太宽、推不出规则的时候才问。
      </p>
      <AppInput
        ref="inputEl"
        v-model="inputVal"
        type="text"
        maxlength="120"
        placeholder="比如：想做出自己的产品，但基础不够"
      />
      <div class="add-goal-dialog__actions">
        <AppButton variant="primary" @click="next">下一步</AppButton>
        <AppButton @click="back">上一步</AppButton>
        <AppButton @click="emit('close')">取消</AppButton>
      </div>
      <Notice v-if="d.problem" tone="danger" class="add-goal-dialog__problem">{{ d.problem }}</Notice>
    </template>

    <template v-else-if="d.step === 2">
      <h2 class="add-goal-dialog__title">什么算推进它？</h2>
      <p class="add-goal-dialog__note">
        越具体越好。一句能装下任何活动的规则等于没有规则——那样的曲线会一直涨，也就永远不告诉你偏了。
      </p>
      <AppInput
        ref="inputEl"
        v-model="inputVal"
        type="text"
        maxlength="120"
        placeholder="比如：读完一章，或做完一章题，算一次"
      />
      <div class="add-goal-dialog__actions">
        <AppButton variant="primary" @click="next">下一步</AppButton>
        <AppButton @click="back">上一步</AppButton>
        <AppButton @click="emit('close')">取消</AppButton>
      </div>
      <Notice v-if="d.problem" tone="danger" class="add-goal-dialog__problem">{{ d.problem }}</Notice>
    </template>

    <template v-else-if="d.step === 3">
      <h2 class="add-goal-dialog__title">什么不算？</h2>
      <p class="add-goal-dialog__note">
        最容易跳过、也最值钱的一问。<br />
        同一件事看起来像在推进它、其实不算的，是什么？<br />
        （例：在 Learn-English 上写代码不算推进「英语」——只算复习记录。<br />
        不划这条边界，那条曲线会假装在涨。）
      </p>
      <AppInput
        ref="inputEl"
        v-model="inputVal"
        type="text"
        maxlength="120"
        placeholder="比如：在它上面写代码、改功能，不算"
      />
      <div class="add-goal-dialog__skip">
        <AppButton id="dlg-skip" variant="dashed" @click="skipBoundary">想不出来，先跳过</AppButton>
      </div>
      <div class="add-goal-dialog__actions">
        <AppButton variant="primary" @click="next">下一步</AppButton>
        <AppButton @click="back">上一步</AppButton>
        <AppButton @click="emit('close')">取消</AppButton>
      </div>
      <Notice v-if="d.problem" tone="danger" class="add-goal-dialog__problem">{{ d.problem }}</Notice>
    </template>

    <template v-else-if="d.step === 4">
      <h2 class="add-goal-dialog__title">这条规则落到哪种来源？</h2>
      <p class="add-goal-dialog__note">四种，是上限。落到别的上去，它就开始变成另一件东西了。</p>
      <SourceKindPicker :model-value="d.kind" @update:model-value="pickKind" />
      <div class="add-goal-dialog__actions add-goal-dialog__actions--cancel">
        <AppButton @click="emit('close')">取消</AppButton>
      </div>
      <Notice v-if="d.problem" tone="danger" class="add-goal-dialog__problem">{{ d.problem }}</Notice>
    </template>

    <template v-else-if="d.step === 5">
      <h2 class="add-goal-dialog__title">确认一下</h2>
      <p class="add-goal-dialog__answer"><b>{{ d.title }}</b></p>
      <p v-if="d.why" class="add-goal-dialog__answer">为什么：{{ d.why }}</p>
      <p class="add-goal-dialog__answer">算推进：{{ d.rule }}</p>
      <p class="add-goal-dialog__answer">
        不算：<template v-if="d.notrule">{{ d.notrule }}</template><b v-else>没划</b>
      </p>
      <p class="add-goal-dialog__answer">
        来源：{{ kindSpec.label }}<template v-if="d.target"> · {{ d.target }}</template>
      </p>
      <Notice v-if="!d.notrule" tone="danger" class="add-goal-dialog__problem">
        没划边界。以后出现「看起来像在推进它、其实不算」的活动时，这条曲线会说谎——而它会理直气壮地涨。
      </Notice>
      <div class="add-goal-dialog__actions">
        <AppButton variant="primary" :disabled="busy" @click="create">建这个目标</AppButton>
        <AppButton @click="emit('close')">取消</AppButton>
      </div>
      <Notice v-if="d.problem" tone="danger" class="add-goal-dialog__problem">{{ d.problem }}</Notice>
    </template>

    <template v-else-if="d.step === 6">
      <h2 class="add-goal-dialog__title">建好了</h2>
      <p class="add-goal-dialog__answer">
        当前值 <b>{{ d.valueText }}</b> 次<br />
        <template v-if="d.value === 0">
          还是 0。要么规则写错了，要么这条线真的还没动——两种都值得知道。
        </template>
        <template v-else>规则一写完就算出了数，说明它是可计算的。</template>
      </p>
      <div class="add-goal-dialog__actions">
        <AppButton variant="primary" @click="emit('created')">好</AppButton>
      </div>
    </template>
  </section>
</template>

<style scoped lang="scss">
// 只占右栏。目标是在左栏的卡片列里诞生的，对话不该跳到整屏页面去。
.add-goal-dialog {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-1) calc(var(--page-padding) + var(--page-margin)) var(--space-6) var(--space-1);
}

.add-goal-dialog__steps {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-bottom: var(--space-5);
}

.add-goal-dialog__step {
  font-size: var(--font-size-2);
  color: var(--text-faint);

  &--done {
    color: var(--text-faint);
  }

  &--current {
    color: var(--text-muted);
    font-weight: bold;
  }
}

.add-goal-dialog__title {
  margin-bottom: var(--space-2);
  font-size: var(--font-size-6);
  font-weight: bold;
}

.add-goal-dialog__note {
  margin-bottom: var(--space-3);
  font-size: var(--font-size-2);
  color: var(--text-muted);
}

.add-goal-dialog__actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  margin-top: var(--space-4);
}

.add-goal-dialog__actions--cancel {
  margin-top: var(--space-4);
}

.add-goal-dialog__skip {
  margin-top: var(--space-2);
}

.add-goal-dialog__answer {
  margin-bottom: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-muted);

  b {
    font-weight: bold;
    color: var(--text);
  }
}

.add-goal-dialog__problem {
  margin-top: var(--space-3);
}
</style>
