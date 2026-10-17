<script setup>
import { computed, nextTick, ref, watch } from 'vue'
import { api } from '../api.js'

// 新建目标是**四步固定骨架 + 一步收口**，不是自由聊天。
//
// 固定骨架解决的是「自由聊天只能靠 prompt 求它自觉」：步数就是步数，
// 而「什么不算」这一问永远不会被跳过。AI 在这里没有位置——
// 要问什么是产品定死的，判据也全是机械的。
const emit = defineEmits(['close', 'created'])

const KINDS = [
  { k: 'manual_checkin', name: '手工打卡', desc: '我自己记一次。比如「读完一章算一次」', need: false },
  { k: 'git_commits', name: 'git 提交', desc: '某个仓库的提交数。还没接入，先登记规则', need: true, ask: '哪个仓库？（先填名字，接上之后才会计数）' },
  { k: 'external_metric', name: '外部数据', desc: '读别处已经记着的数，比如复习记录', need: true, ask: '读哪份数据？（先填标识，接上之后才会计数）' },
  { k: 'derived', name: '组合', desc: '上面几条按公式算', need: false },
]

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
const dlgEl = ref(null)

const kindSpec = computed(() => KINDS.find((x) => x.k === d.value.kind) || KINDS[0])

function focusInput() {
  const go = () => {
    const el = inputEl.value
    if (!el || document.activeElement === el) return
    try {
      el.focus()
    } catch {
      /* 忽略 */
    }
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
  const v = inputVal.value.trim()
  if (askingTarget.value) {
    takeTarget()
    return
  }
  switch (d.value.step) {
    case 0:
      if (!v) {
        d.value.problem = '总得先有个名字。'
        return
      }
      d.value.title = v
      break
    case 1:
      d.value.why = v // 可以不填：不是每个目标都需要问目的
      break
    case 2:
      if (!v) {
        // 这是「必须再问」的落点：收不出一条可计算的规则，就不该往下走。
        d.value.problem =
          '这条填不了。没有它，这个目标给不出「什么算推进它」——曲线永远不会动，它也就不是目标，只是一句愿望。'
        return
      }
      d.value.rule = v
      break
    case 3:
      d.value.notrule = v
      break
    default:
      return
  }
  nav()
}

function pickKind(k) {
  const spec = KINDS.find((x) => x.k === k) || KINDS[0]
  d.value.kind = k
  d.value.problem = ''
  if (spec.need) {
    // 需要 target 的来源：先问清对象。空着就等于「什么都算」。
    askingTarget.value = true
    return
  }
  d.value.target = ''
  d.value.step = 5
}

function takeTarget() {
  const v = inputVal.value.trim()
  if (!v) {
    d.value.problem =
      '空着就等于「什么都算」——那条规则会把所有活动都装进来，曲线永远在涨，也就不告诉你偏了。'
    askingTarget.value = true
    focusInput()
    return
  }
  d.value.target = v
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

function onKey(ev) {
  if (ev.key !== 'Enter' || ev.isComposing || ev.keyCode === 229) return
  const tag = ev.target && ev.target.tagName
  if (tag !== 'INPUT') return
  ev.preventDefault()
  if (d.value.step === 5) create()
  else next()
}
</script>

<template>
  <div ref="dlgEl" id="dlg" class="dlg" @keydown="onKey">
    <div class="dlg-steps">
      <div
        v-for="(name, i) in STEP_NAMES"
        :key="name"
        :class="i < d.step ? 'done' : i === d.step ? 'now' : ''"
      >
        {{ i + 1 }} · {{ name }}
      </div>
    </div>

    <template v-if="askingTarget">
      <h2>{{ kindSpec.name }} · 具体是哪个？</h2>
      <p class="note">{{ kindSpec.ask }}</p>
      <input ref="inputEl" v-model="inputVal" type="text" maxlength="120" />
      <div class="row">
        <button type="button" class="primary" @click="takeTarget">下一步</button>
        <button type="button" @click="back">上一步</button>
        <button type="button" @click="$emit('close')">取消</button>
      </div>
      <div v-if="d.problem" class="cur bad">{{ d.problem }}</div>
    </template>

    <template v-else-if="d.step === 0">
      <h2>你想推进什么？</h2>
      <p class="note">一句话就行。它会是左栏那张卡的标题。</p>
      <input ref="inputEl" v-model="inputVal" type="text" maxlength="40" placeholder="比如：打牢计算机基础" />
      <div class="row">
        <button type="button" class="primary" @click="next">下一步</button>
        <button type="button" @click="$emit('close')">取消</button>
      </div>
      <div v-if="d.problem" class="cur bad">{{ d.problem }}</div>
    </template>

    <template v-else-if="d.step === 1">
      <h2>为什么是现在？</h2>
      <p class="note">
        遇到什么事了，还是在准备什么？<br />
        这一问是为了推出「什么算推进它」，<b>不是问你感受</b>——所以只有在你上一句太宽、推不出规则的时候才问。
      </p>
      <input ref="inputEl" v-model="inputVal" type="text" maxlength="120" placeholder="比如：想做出自己的产品，但基础不够" />
      <div class="row">
        <button type="button" class="primary" @click="next">下一步</button>
        <button type="button" @click="back">上一步</button>
        <button type="button" @click="$emit('close')">取消</button>
      </div>
      <div v-if="d.problem" class="cur bad">{{ d.problem }}</div>
    </template>

    <template v-else-if="d.step === 2">
      <h2>什么算推进它？</h2>
      <p class="note">
        越具体越好。一句能装下任何活动的规则等于没有规则——那样的曲线会一直涨，也就永远不告诉你偏了。
      </p>
      <input ref="inputEl" v-model="inputVal" type="text" maxlength="120" placeholder="比如：读完一章，或做完一章题，算一次" />
      <div class="row">
        <button type="button" class="primary" @click="next">下一步</button>
        <button type="button" @click="back">上一步</button>
        <button type="button" @click="$emit('close')">取消</button>
      </div>
      <div v-if="d.problem" class="cur bad">{{ d.problem }}</div>
    </template>

    <template v-else-if="d.step === 3">
      <h2>什么不算？</h2>
      <p class="note">
        最容易跳过、也最值钱的一问。<br />
        同一件事看起来像在推进它、其实不算的，是什么？<br />
        （例：在 Learn-English 上写代码不算推进「英语」——只算复习记录。<br />
        不划这条边界，那条曲线会假装在涨。）
      </p>
      <input ref="inputEl" v-model="inputVal" type="text" maxlength="120" placeholder="比如：在它上面写代码、改功能，不算" />
      <div class="row row-skip">
        <button id="dlg-skip" type="button" @click="skipBoundary">想不出来，先跳过</button>
      </div>
      <div class="row">
        <button type="button" class="primary" @click="next">下一步</button>
        <button type="button" @click="back">上一步</button>
        <button type="button" @click="$emit('close')">取消</button>
      </div>
      <div v-if="d.problem" class="cur bad">{{ d.problem }}</div>
    </template>

    <template v-else-if="d.step === 4">
      <h2>这条规则落到哪种来源？</h2>
      <p class="note">四种，是上限。落到别的上去，它就开始变成另一件东西了。</p>
      <button v-for="k in KINDS" :key="k.k" type="button" class="kind" @click="pickKind(k.k)">
        <b>{{ k.name }}</b><span>{{ k.desc }}</span>
      </button>
      <div class="row row-cancel">
        <button type="button" @click="$emit('close')">取消</button>
      </div>
      <div v-if="d.problem" class="cur bad">{{ d.problem }}</div>
    </template>

    <template v-else-if="d.step === 5">
      <h2>确认一下</h2>
      <div class="answer"><b>{{ d.title }}</b></div>
      <div v-if="d.why" class="answer">为什么：{{ d.why }}</div>
      <div class="answer">算推进：{{ d.rule }}</div>
      <div class="answer">不算：<template v-if="d.notrule">{{ d.notrule }}</template><b v-else>没划</b></div>
      <div class="answer">来源：{{ kindSpec.name }}<template v-if="d.target"> · {{ d.target }}</template></div>
      <div v-if="!d.notrule" class="cur bad">
        没划边界。以后出现「看起来像在推进它、其实不算」的活动时，这条曲线会说谎——而它会理直气壮地涨。
      </div>
      <div class="row row-confirm">
        <button type="button" class="primary" :disabled="busy" @click="create">建这个目标</button>
        <button type="button" @click="$emit('close')">取消</button>
      </div>
      <div v-if="d.problem" class="cur bad">{{ d.problem }}</div>
    </template>

    <template v-else-if="d.step === 6">
      <h2>建好了</h2>
      <div class="cur">
        当前值 <b>{{ d.valueText }}</b> 次<br />
        <template v-if="d.value === 0">
          还是 0。要么规则写错了，要么这条线真的还没动——两种都值得知道。
        </template>
        <template v-else>规则一写完就算出了数，说明它是可计算的。</template>
      </div>
      <div class="row row-confirm">
        <button type="button" class="primary" @click="$emit('created')">好</button>
      </div>
    </template>
  </div>
</template>

<style scoped lang="scss">
// 只占右栏。目标是在左栏的卡片列里诞生的，对话不该跳到一个和卡片无关的整屏页面去。
.dlg {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-1) calc(var(--page-padding) + var(--page-margin)) var(--space-6) var(--space-1);

  .dlg-steps {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 22px;

    div {
      font-size: var(--font-size-2);
      color: var(--text-faint);

      &.done {
        color: var(--text-muted);
      }

      &.now {
        color: var(--text);
        font-weight: bold;
      }
    }
  }

  h2 {
    font-size: var(--font-size-5);
    font-weight: bold;
    margin-bottom: 6px;
  }

  p.note {
    font-size: var(--font-size-3);
    color: var(--text-faint);
    margin-bottom: 14px;
  }

  .answer {
    font-size: var(--font-size-3);
    color: var(--text-muted);
    margin-bottom: 4px;

    b {
      color: var(--text);
      font-weight: bold;
    }
  }

  input[type="text"] {
    width: 100%;
    background: var(--surface);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    padding: 10px 12px;
    font-size: var(--font-size-3);
    color: var(--text);
    outline: none;
    margin-bottom: 12px;

    &:focus {
      border-color: var(--blue);
    }
  }

  .row {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }

  // 几个只差一点间距的动作行（内联 style 收进来的）。
  .row-skip {
    margin-bottom: var(--space-2);
  }

  .row-cancel {
    margin-top: var(--space-3);
  }

  .row-confirm {
    margin-top: var(--space-4);
  }

  // 通用按钮。**排除 .kind**：那是全局共享的来源按钮，scoped 的 `.dlg button`
  // 会以更高优先级把它的内边距和排版盖掉。
  button:not(.kind) {
    &:hover {
      border-color: var(--text-faint);
      color: var(--text);
    }

    &.primary {
      background: var(--surface-2);
      color: var(--text);

      &:hover {
        border-color: var(--text-faint);
      }
    }

    &[disabled] {
      opacity: 0.45;

      &:hover {
        border-color: var(--border-strong);
        color: var(--text-muted);
      }
    }
  }

  .cur {
    margin-top: 14px;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    font-size: var(--font-size-3);
    color: var(--text-muted);

    b {
      font-size: var(--font-size-6);
      color: var(--text);
      font-weight: bold;
      font-variant-numeric: tabular-nums;
      margin-right: 3px;
    }
  }

  .bad {
    border-color: var(--border-strong);
    background: var(--surface-2);
    color: var(--text-muted);
  }
}
</style>
