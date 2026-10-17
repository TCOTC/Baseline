<script setup>
import { computed, ref } from 'vue'
import { api } from '../api.js'
import { askConfirm } from '../ui.js'
import CurveView from './CurveView.vue'
import LogList from './LogList.vue'

const props = defineProps({
  page: { type: Object, required: true },
  aiState: { type: Object, default: () => ({}) },
})
const emit = defineEmits(['nav', 'reload'])

const goal = computed(() => props.page.goal)

const editing = ref(false)
const editTitle = ref('')
const editWhy = ref('')

const archOpen = ref(false)
const reason = ref('')

const addrOpen = ref(false)
const kind = ref('')
const target = ref('')
const rationale = ref('')

const err = ref('')
const retrying = ref(false)

const KINDS = [
  { k: 'manual_checkin', name: '手工打卡', desc: '我自己记一次' },
  { k: 'git_commits', name: 'git 提交', desc: '某个仓库的提交数（还没接入）' },
  { k: 'external_metric', name: '外部数据', desc: '读别处已经记着的数（还没接入）' },
  { k: 'derived', name: '组合', desc: '上面几条按公式算（还没接入）' },
]

const needTarget = computed(() => kind.value === 'git_commits' || kind.value === 'external_metric')

const looseMsg = computed(() => {
  const g = goal.value
  if (!g || !g.loose) return ''
  return g.manualCount > 1
    ? `有 ${g.loose} 条记录没归到规则，所以没进曲线。这个目标下有多条规则，每条记录要算哪一条得指认。`
    : `有 ${g.loose} 条记录没归到规则，所以没进曲线。`
})

function fail(e) {
  err.value = (e && e.message) || String(e)
}

function startEdit() {
  editTitle.value = goal.value.title
  editWhy.value = goal.value.why
  editing.value = true
}

async function saveEdit() {
  const title = editTitle.value.trim()
  if (!title) {
    fail(new Error('目标得有个名字'))
    return
  }
  try {
    await api.updateGoal(goal.value.id, title, editWhy.value)
    editing.value = false
    err.value = ''
    emit('reload')
  } catch (e) {
    fail(e)
  }
}

async function saveArchive() {
  const r = reason.value.trim()
  if (!r) {
    fail(new Error('归档要写一句原因——那是三个月后唯一能回看的东西'))
    return
  }
  try {
    await api.archiveGoal(goal.value.id, r)
  } catch (e) {
    fail(e)
    return
  }
  // 归档完它就不在列表里了，回主视图，别停在一个已经归档的目标上。
  emit('nav', '/')
}

async function deleteGoal() {
  const yes = await askConfirm({
    title: '删掉这个目标？',
    text: '它还没有任何记录，删了就没了。如果只是暂时不想做，走归档——曲线和记录都会留着，还有你写下的那句理由。',
    what: goal.value.title,
    ok: '删掉',
  })
  if (!yes) return
  try {
    await api.deleteGoal(goal.value.id)
    emit('nav', '/')
  } catch (e) {
    fail(e)
  }
}

async function deleteRule(rule) {
  const yes = await askConfirm({
    title: '删掉这条判定规则？',
    text: '删掉之后，这类活动就不再推进这个目标了。如果这是最后一条规则，它的曲线会停在这里不再动。',
    what: [rule.kindLabel, rule.rationale].filter(Boolean).join(' · '),
    ok: '删掉规则',
  })
  if (!yes) return
  try {
    await api.deleteSource(rule.id)
    err.value = ''
    emit('reload')
  } catch (e) {
    fail(e)
  }
}

function pickKind(k) {
  kind.value = k
  if (k !== 'git_commits' && k !== 'external_metric') target.value = ''
}

async function addSource() {
  if (!kind.value) {
    fail(new Error('先选一种来源'))
    return
  }
  const t = target.value.trim()
  if (needTarget.value && !t) {
    fail(new Error('空着就等于「什么都算」——那条规则会把所有活动都装进来，曲线永远在涨'))
    return
  }
  try {
    await api.addSource(goal.value.id, kind.value, t, rationale.value)
    addrOpen.value = false
    err.value = ''
    rationale.value = ''
    target.value = ''
    kind.value = ''
    emit('reload')
  } catch (e) {
    fail(e)
  }
}

async function retry() {
  retrying.value = true
  try {
    await api.backlog(goal.value.id)
    emit('reload')
  } catch (e) {
    console.error('ai-backlog', e)
  } finally {
    retrying.value = false
  }
}
</script>

<template>
  <div class="detail" :class="goal.color" :data-goal="goal.id">
    <header class="hero">
      <div class="hero-top">
        <div>
          <a class="back" href="/" @click.prevent="emit('nav', '/')">← 返回主页</a>
          <div v-if="!editing">
            <div class="dtitle"><i></i>{{ goal.title }}</div>
            <p v-if="goal.why" class="dwhy">{{ goal.why }}</p>
            <p v-else class="dwhy">还没有写「为什么想做」。</p>
          </div>
          <div v-else>
            <div class="field">
              <label>名字</label>
              <input v-model="editTitle" maxlength="40" />
            </div>
            <div class="field">
              <label>为什么想做</label>
              <textarea v-model="editWhy" rows="3"></textarea>
            </div>
            <div class="dacts" style="border: 0; padding: 0; margin-top: 14px">
              <button id="esave" type="button" @click="saveEdit">保存</button>
              <button type="button" @click="editing = false">取消</button>
            </div>
          </div>
        </div>
        <div class="dcur">
          <span :class="goal.zero ? 'dval zero' : 'dval'">{{ goal.currentText }}</span><span class="unit">次</span>
        </div>
      </div>
      <div class="dcurve"><CurveView :curve="goal.curve" /></div>
      <div class="cv-foot">
        <span>{{ goal.firstDay }}</span>
        <span>{{ goal.spanDays }} 天 · 阶梯线不做插值</span>
        <span>{{ goal.lastDay }}</span>
      </div>
    </header>

    <div class="panes">
      <div class="pane rules">
        <section class="dsec">
          <h3>判定规则<span>什么算推进它</span></h3>
          <div>
            <div v-for="r in goal.rules" :key="r.id" class="rule">
              <div>
                <div class="rkind">{{ r.kindLabel }}{{ r.implemented ? '' : '（未接入）' }}</div>
                <div v-if="r.target" class="rtarget">{{ r.target }}</div>
                <div v-if="r.rationale" class="rwhy">{{ r.rationale }}</div>
              </div>
              <button type="button" class="x" title="删掉这条规则" @click="deleteRule(r)">删除规则</button>
            </div>

            <div v-if="!goal.rules.length" class="hint">
              还没有判定规则。没有它，这个目标给不出「什么算推进它」——曲线永远不会动。
            </div>

            <!-- 挂上来了、但没归到任何规则。别让人自己发现这件事：
                 一条记录明明在流水里，数字却不动，看起来就是坏的。 -->
            <div v-if="goal.loose > 0 && goal.manualCount > 0" class="hint warn">
              {{ looseMsg }}
              <button
                v-if="goal.aiRetry"
                type="button"
                class="addrule airetry"
                :disabled="retrying"
                @click="retry"
              >
                {{ retrying ? '正在判…' : `让 AI 判这 ${goal.loose} 条` }}
              </button>
            </div>
          </div>

          <button id="addrbtn" type="button" class="addrule" @click="addrOpen = !addrOpen">＋ 加一条规则</button>

          <div v-if="addrOpen" style="margin-top: 12px">
            <div>
              <button
                v-for="k in KINDS"
                :key="k.k"
                type="button"
                class="kind"
                :class="{ on: kind === k.k }"
                @click="pickKind(k.k)"
              >
                <b>{{ k.name }}</b><span>{{ k.desc }}</span>
              </button>
            </div>
            <div v-if="needTarget" class="field">
              <label>具体是哪个？<span>{{ kind === 'git_commits' ? '哪个仓库？' : '读哪份数据？' }}</span></label>
              <input v-model="target" maxlength="120" />
            </div>
            <div class="field">
              <label>什么算推进它</label>
              <input v-model="rationale" maxlength="120" placeholder="比如：读完一章，或做完一章题，算一次" />
            </div>
            <div class="dacts" style="border: 0; padding: 0; margin-top: 10px">
              <button type="button" @click="addSource">加上</button>
              <button type="button" @click="addrOpen = false">取消</button>
            </div>
          </div>
        </section>

        <div class="dacts">
          <button id="dedit" type="button" @click="startEdit">修改</button>
          <button id="darch" type="button" @click="archOpen = true">归档</button>
          <button id="ddel" type="button" class="danger" :disabled="!goal.canDelete" @click="deleteGoal">删除</button>
        </div>
        <div v-if="!goal.canDelete" class="hint">
          已经记了 {{ goal.checkinCount }} 条，所以删不掉。动过的历史不该被一次点击抹掉——不要了请走归档，它会保留曲线和放弃的理由。
        </div>
        <div v-else class="hint">还没有任何记录，所以可以直接删掉。删了就没了，归档则会留下痕迹。</div>

        <div v-if="err" class="warnline">{{ err }}</div>

        <div v-if="archOpen" style="margin-top: 16px">
          <div class="field">
            <label>为什么放弃它？</label>
            <textarea id="areason" v-model="reason" rows="2" placeholder="这句话三个月后你会想再看一眼"></textarea>
          </div>
          <div class="dacts" style="border: 0; padding: 0; margin-top: 10px">
            <button type="button" @click="saveArchive">归档</button>
            <button type="button" @click="archOpen = false">取消</button>
          </div>
        </div>
      </div>

      <div class="pane recs">
        <section class="dsec">
          <h3>记录<span>{{ goal.checkinCount }} 条</span></h3>
          <LogList variant="dlog" :rows="goal.timeline" :ai-state="aiState" :show-empty="false" />
          <div v-if="!goal.timeline.length" class="hint">
            这个目标还没有任何记录。回主页，在底部输入框里记一条。
          </div>
        </section>
      </div>
    </div>
  </div>
</template>
