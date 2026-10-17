<script setup>
import { computed, nextTick, ref } from 'vue'
import { api } from '../api.js'
import GoalCard from './GoalCard.vue'
import LogList from './LogList.vue'
import Composer from './Composer.vue'
import AddGoalDialog from './AddGoalDialog.vue'
import Notice from './Notice.vue'
import AppButton from './AppButton.vue'

const props = defineProps({
  page: { type: Object, required: true },
  aiState: { type: Object, default: () => ({}) },
})
const emit = defineEmits(['nav', 'reload', 'created'])

const dialog = ref(false)
const retrying = ref(false)

const noRule = computed(() => props.page.warnings.find((w) => w.kind === 'no-rule') || null)
const unlinked = computed(() => props.page.warnings.find((w) => w.kind === 'unlinked') || null)

async function retry() {
  retrying.value = true
  try {
    await api.backlog(null)
    emit('reload')
  } catch (e) {
    console.error('ai-backlog', e)
  } finally {
    retrying.value = false
  }
}

function openDialog() {
  dialog.value = true
}

// 关掉对话框之后把光标放回输入框：那是打开之前它待的地方。
async function focusComposer() {
  await nextTick()
  const el = document.getElementById('composer-input')
  if (el && !el.disabled) el.focus()
}

function closeDialog() {
  dialog.value = false
  focusComposer()
}
function onCreated() {
  dialog.value = false
  emit('reload')
  focusComposer()
}
</script>

<template>
  <main class="main-view">
    <section class="main-view__goals-column" aria-label="目标">
      <div class="main-view__goals">
        <p v-if="!page.goals.length" class="main-view__empty">
          还没有目标。<br />点左下角的「新建目标」——建的时候要一并写清「什么算推进它」，不然那条曲线永远不会动。
        </p>

        <!-- 待办排在卡片前面：它要看得见才有用。 -->
        <Notice v-if="unlinked" id="unlinked-warn" tone="warning">
          有 <b>{{ unlinked.count }}</b> 条记录没关联目标，所以没进任何曲线。<br />
          <AppButton
            v-if="unlinked.aiRetry"
            variant="dashed"
            :disabled="retrying"
            @click="retry"
          >
            {{ retrying ? '正在判…' : `让 AI 判这 ${unlinked.count} 条` }}
          </AppButton>
          <template v-else>记一条的时候在下面点一下目标；或者在设置里打开 AI 判定，让它替你归。</template>
        </Notice>

        <GoalCard
          v-for="card in page.goals"
          :key="card.id"
          :card="card"
          @nav="(url) => emit('nav', url)"
        />

        <Notice v-if="noRule" tone="warning">
          这些目标<b>没有判定规则</b>，曲线永远不会动：<b>{{ noRule.titles.join('、') }}</b><br />
          点开它们，在「判定规则」里补上。
        </Notice>
      </div>

      <div class="main-view__footer">
        <button
          id="gear"
          type="button"
          class="main-view__settings"
          title="设置（AI 判定）"
          aria-label="设置"
          @click="emit('nav', '?settings=1')"
        >
          &#xE713;
        </button>
        <AppButton id="addgoal" variant="dashed" class="main-view__add-goal" @click="openDialog">
          <svg viewBox="0 0 12 12" aria-hidden="true">
            <path
              d="M6 1v10M1 6h10"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
            />
          </svg>
          <span>新建目标</span>
        </AppButton>
      </div>
    </section>

    <section class="main-view__log-column" aria-label="记录">
      <LogList v-show="!dialog" :rows="page.timeline" :ai-state="aiState" :visible="!dialog" />
      <Composer
        v-show="!dialog"
        :composer="page.composer"
        :visible="!dialog"
        @created="(res) => emit('created', res)"
      />
      <AddGoalDialog v-if="dialog" @close="closeDialog" @created="onCreated" />
    </section>
  </main>
</template>

<style scoped lang="scss">
// 只有两栏：左边是目标（窄、固定），右边是流水（宽、可滚）。
.main-view {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 308px 1fr;
  gap: 34px;
  // 右侧不留页面留白：流水要一直伸到窗口右边缘，滚动条才贴边。
  padding: 10px 0 0 calc(var(--page-padding) + var(--page-margin));
}

.main-view__goals-column {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.main-view__goals {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-gutter: stable;
  padding-bottom: var(--space-4);
}

.main-view__empty {
  color: var(--text-faint);
  font-size: var(--font-size-3);
}

.main-view__footer {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-top: var(--space-4);
  padding-bottom: var(--space-4);
}

.main-view__settings {
  flex: none;
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-faint);
  font-family: var(--font-icon);
  font-size: var(--font-size-5);
  line-height: 1;
  transform: translateZ(0);

  &:hover {
    color: var(--text-muted);
  }
}

.main-view__add-goal {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-3);

  svg {
    width: 12px;
    height: 12px;
  }
}

.main-view__log-column {
  display: flex;
  flex-direction: column;
  min-height: 0;
}
</style>
