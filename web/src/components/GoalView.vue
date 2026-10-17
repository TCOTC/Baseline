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
    ? `有 ${g.loose} 条记录未归属到规则，未计入曲线。该目标下有多条规则，无法自动确定归属。`
    : `有 ${g.loose} 条记录未归属到规则，未计入曲线。`
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
    fail(new Error('目标名称不能为空。'))
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
    fail(new Error('归档必须填写原因，便于日后回顾。'))
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
    title: '删除目标',
    text: '该目标暂无记录，删除后无法恢复。如需暂时停止，请使用归档：曲线、记录和归档原因都会保留。',
    what: goal.value.title,
    ok: '删除',
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
    title: '删除判定规则',
    text: '删除后，此类活动不再计入该目标。若这是最后一条规则，该目标的曲线将停止更新。',
    what: [rule.kindLabel, rule.rationale].filter(Boolean).join(' · '),
    ok: '删除规则',
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
    fail(new Error('请选择一种来源类型。'))
    return
  }
  const value = target.value.trim()
  if (needTarget.value && !value) {
    fail(new Error('目标对象不能为空。否则规则会匹配所有活动，导致曲线持续上升。'))
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
              {{ goal.why || '暂无背景说明。' }}
            </p>
          </template>

          <div v-else class="goal-detail__edit">
            <FormField label="目标名称" for-id="etitle">
              <AppInput id="etitle" v-model="editTitle" maxlength="40" />
            </FormField>
            <FormField label="目标背景">
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
        <span>{{ goal.spanDays }} 天 · 阶梯线，不插值</span>
        <span>{{ goal.lastDay }}</span>
      </div>
    </template>

    <div class="goal-detail__panes">
      <div class="goal-detail__pane goal-detail__pane--rules">
        <AppSection title="判定规则" hint="计入条件">
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
              暂无判定规则。缺少规则时，该目标无法计算推进。
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
                {{ retrying ? '判定中…' : `由 AI 判定 ${goal.loose} 条` }}
              </AppButton>
            </Notice>
          </div>

          <AppButton id="addrbtn" variant="dashed" class="goal-detail__add-rule" @click="addrOpen = !addrOpen">
            ＋ 添加规则
          </AppButton>

          <div v-if="addrOpen" id="addrform" class="goal-detail__add-form">
            <SourceKindPicker id="addrkinds" :model-value="kind" @update:model-value="pickKind" />
            <FormField v-if="needTarget" label="目标对象" :hint="findKind(kind).targetPrompt">
              <AppInput id="artarget" v-model="target" maxlength="120" />
            </FormField>
            <FormField label="计入条件">
              <AppInput
                id="arrationale"
                v-model="rationale"
                maxlength="120"
                placeholder="例如：读完一章，或完成一章习题，计一次"
              />
            </FormField>
            <ActionBar class="goal-detail__form-actions">
              <AppButton id="arsave" variant="primary" @click="addSource">添加</AppButton>
              <AppButton id="arcancel" @click="addrOpen = false">取消</AppButton>
            </ActionBar>
          </div>
        </AppSection>

        <ActionBar class="goal-detail__meta-actions">
          <AppButton id="dedit" @click="startEdit">编辑</AppButton>
          <AppButton id="darch" @click="archOpen = true">归档</AppButton>
          <AppButton id="ddel" variant="danger" :disabled="!goal.canDelete" @click="deleteGoal">
            删除
          </AppButton>
        </ActionBar>

        <Notice v-if="!goal.canDelete" class="goal-detail__hint">
          已记录 {{ goal.checkinCount }} 条，无法删除。如需停止，请使用归档，曲线与归档原因都会保留。
        </Notice>
        <Notice v-else class="goal-detail__hint">
          暂无记录，可以删除。删除后无法恢复；归档会保留曲线和原因。
        </Notice>

        <Notice v-if="err" id="derr" tone="danger" class="goal-detail__hint">{{ err }}</Notice>

        <div v-if="archOpen" class="goal-detail__archive-form">
          <FormField label="归档原因">
            <AppTextarea
              id="areason"
              v-model="reason"
              :rows="2"
              placeholder="例如：方向调整，暂时不再投入。"
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
            该目标暂无记录。返回首页，在底部输入框中添加记录。
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
