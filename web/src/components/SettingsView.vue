<script setup>
import { computed, ref, watch } from 'vue'
import { api } from '../api.js'
import BackLink from './BackLink.vue'
import PageShell from './PageShell.vue'
import AppSection from './AppSection.vue'
import FormField from './FormField.vue'
import AppInput from './AppInput.vue'
import ActionBar from './ActionBar.vue'
import AppButton from './AppButton.vue'
import Notice from './Notice.vue'

const props = defineProps({ page: { type: Object, required: true } })
const emit = defineEmits(['reload', 'nav'])

const s = computed(() => props.page.settings)
const base = ref('')
const model = ref('')
const key = ref('')
const threshold = ref(80)
const msg = ref('')
const bad = ref(false)
const armed = ref(false)
const busy = ref(false)

watch(
  s,
  (value) => {
    if (!value) return
    base.value = value.baseUrl
    model.value = value.model
    threshold.value = value.threshold
    key.value = ''
    armed.value = false
  },
  { immediate: true },
)

function show(text, isBad) {
  msg.value = text || ''
  bad.value = !!isBad
}

function keyValue() {
  const value = key.value.trim()
  return value === '' ? null : value
}

async function save(enabled, apiKey) {
  await api.saveSettings({
    enabled,
    baseUrl: base.value,
    model: model.value,
    threshold: Number(threshold.value),
    apiKey,
  })
}

async function onSave() {
  if (busy.value) return
  busy.value = true
  show('')
  try {
    await save(true, keyValue())
    emit('reload')
  } catch (e) {
    show('没存上：' + e.message, true)
  } finally {
    busy.value = false
  }
}

async function onTest() {
  if (busy.value) return
  busy.value = true
  show('正在测…')
  try {
    // 「测一下」要先把当前填的存下来再测，否则测的是上一次的配置，
    // 而人以为测的是屏幕上这些字。
    await save(true, keyValue())
    const result = await api.testAI()
    show(result.message)
  } catch (e) {
    show('没通：' + e.message, true)
  } finally {
    busy.value = false
  }
}

async function onToggle() {
  if (busy.value) return
  busy.value = true
  show('')
  try {
    // 按钮上是「关掉」还是「打开」由服务端定；点下去就是取反。
    await save(!s.value.enabled, keyValue())
    emit('reload')
  } catch (e) {
    show('没改成功：' + e.message, true)
  } finally {
    busy.value = false
  }
}

async function onClear() {
  // 两下：这个动作撤不回来（原来的密钥只在库里存着密文，删了就没了）。
  if (!armed.value) {
    armed.value = true
    return
  }
  if (busy.value) return
  busy.value = true
  show('')
  try {
    await save(true, '')
    emit('reload')
  } catch (e) {
    show('没清掉：' + e.message, true)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <PageShell>
    <template #hero>
      <BackLink @nav="emit('nav', '/')" />
      <h1 class="settings-view__title">设置</h1>
    </template>

    <div class="settings-view">
      <AppSection class="settings-view__section" title="AI 判定" :hint="s.state">
        <p class="settings-view__intro">{{ s.blurb }}</p>

        <Notice v-if="s.keyBroken" tone="danger" class="settings-view__notice">
          密钥读不出来了，重新填一次就好。
        </Notice>

        <FormField label="接口地址" for-id="ai-base" hint="OpenAI 兼容的地址。DeepSeek、OpenAI、Moonshot、本机跑的模型都行。">
          <AppInput
            id="ai-base"
            v-model="base"
            type="text"
            :placeholder="s.defaultBaseUrl"
            spellcheck="false"
          />
        </FormField>

        <FormField label="模型" for-id="ai-model">
          <AppInput
            id="ai-model"
            v-model="model"
            type="text"
            :placeholder="s.defaultModel"
            spellcheck="false"
          />
        </FormField>

        <FormField
          label="API 密钥"
          for-id="ai-key"
          hint="只存在这台电脑上，不会发到别处。留空就是不改动现在这把。"
        >
          <div class="settings-view__key-row">
            <AppInput
              id="ai-key"
              v-model="key"
              type="password"
              :placeholder="s.keyPlaceholder"
              autocomplete="off"
              spellcheck="false"
            />
            <AppButton
              id="ai-key-clear"
              :variant="armed ? 'danger' : 'default'"
              :disabled="s.clearDisabled || busy"
              @click="onClear"
            >
              {{ armed ? '再点一次' : '清掉' }}
            </AppButton>
          </div>
        </FormField>

        <FormField label="多大把握才自动选" for-id="ai-th">
          <p class="settings-view__threshold">当前把握门槛 <b id="ai-th-v">{{ threshold }}</b></p>
          <input
            id="ai-th"
            v-model.number="threshold"
            class="settings-view__range"
            type="range"
            min="0"
            max="100"
            step="5"
          />
          <p class="settings-view__range-hint">
            没到这个把握就不替你决定，改成问你。定得越高，自动决定的越少。
          </p>
        </FormField>

        <ActionBar class="settings-view__actions">
          <AppButton id="ai-save" :disabled="busy" @click="onSave">保存</AppButton>
          <AppButton id="ai-test" :disabled="busy" @click="onTest">测一下</AppButton>
          <AppButton id="ai-toggle" :disabled="busy" @click="onToggle">{{ s.toggle }}</AppButton>
        </ActionBar>

        <Notice id="ai-msg" :tone="bad ? 'danger' : 'muted'" class="settings-view__notice">
          {{ msg }}
        </Notice>
      </AppSection>
    </div>
  </PageShell>
</template>

<style scoped lang="scss">
.settings-view__title {
  font-size: var(--font-size-7);
  font-weight: bold;
}

// 设置页：单栏、自己滚。不复用详情页的两栏网格——只有一栏，
// 套上去会在右边留一大片空白，看起来像内容没加载出来。
.settings-view {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  padding: var(--space-5) var(--page-padding) var(--space-6);
  scrollbar-gutter: stable;
}

.settings-view__section {
  max-width: 620px;
}

.settings-view__intro {
  margin-bottom: var(--space-3);
  font-size: var(--font-size-2);
  color: var(--text-muted);
}

.settings-view__notice {
  margin-top: var(--space-2);
}

.settings-view__key-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.settings-view__key-row .app-button {
  flex: none;
  white-space: nowrap;
}

// 滑块：用目标色，和页面上其他可操作的东西一个调子。
.settings-view__range {
  width: 100%;
  height: 22px;
  accent-color: var(--blue);
}

.settings-view__threshold {
  display: block;
  margin-bottom: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-muted);
}

.settings-view__range-hint {
  margin-top: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-faint);
}

.settings-view__actions {
  margin-top: var(--space-4);
}
</style>
