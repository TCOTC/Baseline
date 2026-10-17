<script setup>
import { computed, ref } from 'vue'
import { api } from '../api.js'
import { askConfirm } from '../ui.js'
import { findKind } from '../kinds.js'
import CurveView from './CurveView.vue'
import LogList from './LogList.vue'
import BackLink from './BackLink.vue'
import PageShell from './PageShell.vue'
import AppSection from './AppSection.vue'
import FormField from './FormField.vue'
import AppInput from './AppInput.vue'
import AppTextarea from './AppTextarea.vue'
import ActionBar from './ActionBar.vue'
import AppButton from './AppButton.vue'
import Notice from './Notice.vue'
import SourceKindPicker from './SourceKindPicker.vue'

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

const needTarget = computed(() => findKind(kind.value).needsTarget)

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
  const value = reason.value.trim()
  if (!value) {
    fail(new Error('归档要写一句原因——那是三个月后唯一能回看的东西'))
    return
  }
  try {
    await api.archiveGoal(goal.value.id, value)
  } catch (e) {
    fail(e)
    return
  }
  // 归档完它就不在列表里了，回主视图。
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

function pickKind(value) {
  kind.value = value
  if (!findKind(value).needsTarget) target.value = ''
}

async function addSource() {
  if (!kind.value) {
    fail(new Error('先选一种来源'))
    return
  }
  const value = target.value.trim()
  if (needTarget.value && !value) {
    fail(new Error('空着就等于「什么都算」——那条规则会把所有活动都装进来，曲线永远在涨'))
    return
  }
  try {
    await api.addSource(goal.value.id, kind.value, value, rationale.value)
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
  <PageShell :theme="goal.color" :data-goal="goal.id">
    <template #hero>
      <BackLink @nav="emit('nav', '/')" />

      <div class="goal-detail__header">
        <div class="goal-detail__intro">
          <template v-if="!editing">
            <h1 class="goal-detail__title">{{ goal.title }}</h1>
            <p class="goal-detail__why">
              {{ goal.why || '还没有写「为什么想做」。' }}
            </p>
          </template>

          <div v-else class="goal-detail__edit">
            <FormField label="名字" for-id="etitle">
              <AppInput id="etitle" v-model="editTitle" maxlength="40" />
            </FormField>
            <FormField label="为什么想做">
              <AppTextarea v-model="editWhy" :rows="3" />
            </FormField>
            <ActionBar class="goal-detail__edit-actions">
              <AppButton id="esave" variant="primary" @click="saveEdit">保存</AppButton>
              <AppButton id="ecancel" @click="editing = false">取消</AppButton>
            </ActionBar>
          </div>
        </div>

        <p class="goal-detail__current">
          <strong class="goal-detail__value" :class="{ 'goal-detail__value--zero': goal.zero }">
            {{ goal.currentText }}
          </strong>
          <span class="goal-detail__unit">次</span>
        </p>
      </div>

      <CurveView class="goal-detail__curve" :curve="goal.curve" />

      <div class="goal-detail__curve-caption">
        <span>{{ goal.firstDay }}</span>
        <span>{{ goal.spanDays }} 天 · 阶梯线不做插值</span>
        <span>{{ goal.lastDay }}</span>
      </div>
    </template>

    <div class="goal-detail__panes">
      <div class="goal-detail__pane goal-detail__pane--rules">
        <AppSection title="判定规则" hint="什么算推进它">
          <div class="goal-detail__rules">
            <article v-for="rule in goal.rules" :key="rule.id" class="goal-detail__rule">
              <div>
                <p class="goal-detail__rule-kind">
                  {{ rule.kindLabel }}{{ rule.implemented ? '' : '（未接入）' }}
                </p>
                <p v-if="rule.target" class="goal-detail__rule-target">{{ rule.target }}</p>
                <p v-if="rule.rationale" class="goal-detail__rule-why">{{ rule.rationale }}</p>
              </div>
              <AppButton variant="ghost" class="goal-detail__rule-delete" @click="deleteRule(rule)">
                删除规则
              </AppButton>
            </article>

            <Notice v-if="!goal.rules.length" class="goal-detail__hint">
              还没有判定规则。没有它，这个目标给不出「什么算推进它」——曲线永远不会动。
            </Notice>

            <!-- 挂上来了、但没归到任何规则。别让人自己发现：一条记录在流水里，
                 数字却不动，看起来就是坏的。 -->
            <Notice v-if="goal.loose > 0 && goal.manualCount > 0" tone="warning" class="goal-detail__hint">
              {{ looseMsg }}
              <AppButton
                v-if="goal.aiRetry"
                variant="dashed"
                :disabled="retrying"
                @click="retry"
              >
                {{ retrying ? '正在判…' : `让 AI 判这 ${goal.loose} 条` }}
              </AppButton>
            </Notice>
          </div>

          <AppButton id="addrbtn" variant="dashed" class="goal-detail__add-rule" @click="addrOpen = !addrOpen">
            ＋ 加一条规则
          </AppButton>

          <div v-if="addrOpen" id="addrform" class="goal-detail__add-form">
            <SourceKindPicker id="addrkinds" :model-value="kind" @update:model-value="pickKind" />
            <FormField v-if="needTarget" label="具体是哪个？" :hint="findKind(kind).targetPrompt">
              <AppInput id="artarget" v-model="target" maxlength="120" />
            </FormField>
            <FormField label="什么算推进它">
              <AppInput
                id="arrationale"
                v-model="rationale"
                maxlength="120"
                placeholder="比如：读完一章，或做完一章题，算一次"
              />
            </FormField>
            <ActionBar class="goal-detail__form-actions">
              <AppButton id="arsave" variant="primary" @click="addSource">加上</AppButton>
              <AppButton id="arcancel" @click="addrOpen = false">取消</AppButton>
            </ActionBar>
          </div>
        </AppSection>

        <ActionBar class="goal-detail__meta-actions">
          <AppButton id="dedit" @click="startEdit">修改</AppButton>
          <AppButton id="darch" @click="archOpen = true">归档</AppButton>
          <AppButton id="ddel" variant="danger" :disabled="!goal.canDelete" @click="deleteGoal">
            删除
          </AppButton>
        </ActionBar>

        <Notice v-if="!goal.canDelete" class="goal-detail__hint">
          已经记了 {{ goal.checkinCount }} 条，所以删不掉。动过的历史不该被一次点击抹掉——不要了请走归档，它会保留曲线和放弃的理由。
        </Notice>
        <Notice v-else class="goal-detail__hint">
          还没有任何记录，所以可以直接删掉。删了就没了，归档则会留下痕迹。
        </Notice>

        <Notice v-if="err" id="derr" tone="danger" class="goal-detail__hint">{{ err }}</Notice>

        <div v-if="archOpen" class="goal-detail__archive-form">
          <FormField label="为什么放弃它？">
            <AppTextarea
              id="areason"
              v-model="reason"
              :rows="2"
              placeholder="这句话三个月后你会想再看一眼"
            />
          </FormField>
          <ActionBar class="goal-detail__form-actions">
            <AppButton id="asave" variant="primary" @click="saveArchive">归档</AppButton>
            <AppButton id="acancel" @click="archOpen = false">取消</AppButton>
          </ActionBar>
        </div>
      </div>

      <div class="goal-detail__pane goal-detail__pane--records">
        <AppSection title="记录" :hint="`${goal.checkinCount} 条`">
          <LogList variant="detail" :rows="goal.timeline" :ai-state="aiState" :show-empty="false" />
          <Notice v-if="!goal.timeline.length" class="goal-detail__hint">
            这个目标还没有任何记录。回主页，在底部输入框里记一条。
          </Notice>
        </AppSection>
      </div>
    </div>
  </PageShell>
</template>

<style scoped lang="scss">
// 当前值对齐 why 那一行的底边，不是顶部——顶对齐时它悬在标题旁边。
.goal-detail__header {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: var(--space-4);
}

.goal-detail__intro {
  min-width: 0;
}

.goal-detail__title {
  font-size: var(--font-size-7);
  font-weight: bold;
}

.goal-detail__why {
  margin-top: var(--space-2);
  font-size: var(--font-size-2);
  color: var(--text-muted);
}

.goal-detail__edit {
  max-width: 460px;
}

.goal-detail__edit-actions {
  margin-top: var(--space-3);
}

.goal-detail__current {
  flex: none;
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  color: var(--goal-color);
}

.goal-detail__value {
  font-size: var(--font-size-8);
  font-weight: bold;
  line-height: 1;
  font-variant-numeric: tabular-nums;

  &--zero {
    color: var(--text-faint);
    font-weight: normal;
  }
}

.goal-detail__unit {
  font-size: var(--font-size-2);
  color: var(--text-faint);
}

.goal-detail__curve {
  height: 116px;
  margin-top: var(--space-4);
}

.goal-detail__curve-caption {
  display: flex;
  justify-content: space-between;
  margin-top: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-faint);
}

.goal-detail__panes {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 1fr 1.4fr;
}

.goal-detail__pane {
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-gutter: stable;
  padding: var(--space-4) var(--space-5) var(--space-4) var(--page-padding);
}

.goal-detail__pane--rules {
  border-right: 1px solid var(--border);
}

.goal-detail__pane--records {
  padding-right: 0;
  padding-left: var(--space-5);
}

.goal-detail__rules {
  margin-bottom: var(--space-3);
}

.goal-detail__rule {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-3) 0;
  border-bottom: 1px solid var(--border);
}

.goal-detail__rule-kind {
  font-size: var(--font-size-3);
  font-weight: bold;
}

.goal-detail__rule-target,
.goal-detail__rule-why {
  margin-top: 2px;
  font-size: var(--font-size-2);
  color: var(--text-muted);
}

.goal-detail__rule-why {
  color: var(--text-faint);
}

.goal-detail__rule-delete {
  flex: none;
  font-size: var(--font-size-2);
}

.goal-detail__add-rule {
  margin-top: var(--space-2);
}

.goal-detail__add-form {
  margin-top: var(--space-3);
}

.goal-detail__form-actions {
  margin-top: var(--space-3);
}

.goal-detail__meta-actions {
  margin-top: var(--space-5);
}

.goal-detail__hint {
  margin-top: var(--space-2);
}

.goal-detail__archive-form {
  margin-top: var(--space-3);
}
</style>
