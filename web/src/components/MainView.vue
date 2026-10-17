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

<style scoped lang="scss">
// 网格一直伸到窗口右边缘，右侧滚动条才贴得住窗口边；
// 内容的 1120px 由左内边距和右栏滚动容器的内边距对齐。
.cols {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 308px 1fr;
  gap: calc(var(--page-padding) + var(--space-1));
  padding: 10px 0 0 calc(var(--page-padding) + var(--page-margin));
}

// 左栏：卡片自己滚，「新建目标」钉在底部左下角。钉住而不是跟着滚——
// 它是这一栏的入口，不该被三张卡挤出视野。
.left {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.goals {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  overflow-y: auto;
  overflow-x: hidden;
  padding-bottom: 12px;
}

// 底边距和输入框的 18px 对齐：左栏的加号和右栏那个框的底边应当落在同一条线上。
.lfoot {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: var(--space-1) 0 var(--space-4);
}

.addgoal {
  flex: none;
  align-self: flex-start;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: 0;
  border-style: dashed;
  color: var(--text-faint);

  svg {
    display: block;
    width: 11px;
    height: 11px;
  }

  &:hover {
    border-color: var(--text-faint);
    color: var(--text-muted);
  }
}

// 齿轮：和「新建目标」同一排、在它左边。用 Segoe Fluent Icons 的字符而不是画 SVG，
// 和顶栏那三个按钮同一个理由（形状与 DPI 缩放交给 Windows）。
.gear {
  flex: none;
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  color: var(--text-faint);
  font-family: var(--font-icon);
  font-size: var(--font-size-5);
  line-height: 1;
  transform: translateZ(0);

  &:hover {
    color: var(--text-muted);
  }
}

// 右栏一直伸到窗口右边缘。**内边距一律加在真正滚动/装内容的元素上**，
// 不要加在 .right 上——加在父级会把 .log 整体往左推，滚动条跟着进来一百多像素。
.right {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

// 左栏空了的时候不留一片白：告诉他一件事该怎么做，而且就在手边。
.nogoal {
  padding: 0 0 var(--space-2);
  color: var(--text-faint);
  font-size: var(--font-size-3);
}

// 待办提醒：排在卡片前面，清掉就没了。
// 样式在 styles/_shared.scss 里（和详情页、表单的提醒共用一套）。
</style>
