<script setup>
import { computed, nextTick, ref } from 'vue'
import { api } from '../api.js'
import GoalCard from './GoalCard.vue'
import LogList from './LogList.vue'
import Composer from './Composer.vue'
import AddGoalDialog from './AddGoalDialog.vue'

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

// 关掉对话框（新建成功、取消、点「好」）之后把光标放回输入框：
// 那是打开对话框之前它待的地方，回来时不该要再点一下。
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
  <div class="cols">
    <div class="left">
      <div class="goals">
        <div v-if="!page.goals.length" class="nogoal">
          还没有目标。<br />点左下角的「新建目标」——建的时候要一并写清「什么算推进它」，不然那条曲线永远不会动。
        </div>

        <!-- 待办排在卡片前面：它要看得见才有用。卡片是常驻内容，待办清掉就没了。 -->
        <div v-if="unlinked" id="unlinked-warn" class="warn">
          有 <b>{{ unlinked.count }}</b> 条记录没关联目标，所以没进任何曲线。<br />
          <button
            v-if="unlinked.aiRetry"
            type="button"
            class="addrule airetry"
            :disabled="retrying"
            @click="retry"
          >
            {{ retrying ? '正在判…' : `让 AI 判这 ${unlinked.count} 条` }}
          </button>
          <template v-else>记一条的时候在下面点一下目标；或者在设置里打开 AI 判定，让它替你归。</template>
        </div>

        <GoalCard v-for="card in page.goals" :key="card.id" :card="card" @nav="(u) => emit('nav', u)" />

        <div v-if="noRule" class="warn">
          这些目标<b>没有判定规则</b>，曲线永远不会动：<b>{{ noRule.titles.join('、') }}</b><br />
          点开它们，在「判定规则」里补上。
        </div>
      </div>

      <div class="lfoot">
        <button id="gear" type="button" class="gear" title="设置（AI 判定）" aria-label="设置" @click="emit('nav', '?settings=1')">
          &#xE713;
        </button>
        <button id="addgoal" type="button" class="addgoal" @click="openDialog">
          <svg viewBox="0 0 12 12" aria-hidden="true">
            <path d="M6 1v10M1 6h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
          </svg>
          <span>新建目标</span>
        </button>
      </div>
    </div>

    <div class="right">
      <LogList v-show="!dialog" :rows="page.timeline" :ai-state="aiState" :visible="!dialog" />
      <Composer
        v-show="!dialog"
        :composer="page.composer"
        :visible="!dialog"
        @created="(res) => emit('created', res)"
      />
      <AddGoalDialog v-if="dialog" @close="closeDialog" @created="onCreated" />
    </div>
  </div>
</template>
