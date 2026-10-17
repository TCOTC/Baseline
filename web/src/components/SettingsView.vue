<script setup>
import { computed, ref, watch } from 'vue'
import { api } from '../api.js'
import BackLink from './BackLink.vue'
import PageShell from './PageShell.vue'
import SettingsTabs from './SettingsTabs.vue'
import SettingsGroup from './SettingsGroup.vue'
import FormField from './FormField.vue'
import AppInput from './AppInput.vue'
import ActionBar from './ActionBar.vue'
import AppButton from './AppButton.vue'
import Notice from './Notice.vue'

const props = defineProps({ page: { type: Object, required: true } })
const emit = defineEmits(['reload', 'nav'])

const tabs = [
  { id: 'ai', label: 'AI 判定', hint: '启用状态与置信度阈值' },
  { id: 'connection', label: '接口与密钥', hint: '地址、模型与密钥' },
]

const s = computed(() => props.page.settings)
const activeId = ref('ai')
const activeTab = computed(() => tabs.find((tab) => tab.id === activeId.value) || tabs[0])
const activeHint = computed(() =>
  activeId.value === 'ai' ? s.value.blurb : '配置 OpenAI 兼容接口。',
)

const base = ref('')
const model = ref('')
const key = ref('')
const threshold = ref(80)
const msg = ref('')
const bad = ref(false)
const armed = ref(false)
const busy = ref(false)
const scrollEl = ref(null)

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

function selectTab(id) {
  if (id === activeId.value) return
  activeId.value = id
  // 「确认清除」只对刚刚那个分类里的清空有意义；切走再回来不该还是上膛状态。
  armed.value = false
  if (scrollEl.value) scrollEl.value.scrollTop = 0
}

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
    show('保存成功')
  } catch (e) {
    show('保存失败：' + e.message, true)
  } finally {
    busy.value = false
  }
}

async function onTest() {
  if (busy.value) return
  busy.value = true
  show('正在测试…')
  try {
    // 「测试连接」要先把当前填的存下来再测，否则测的是上一次的配置，
    // 而人以为测的是屏幕上这些字。
    await save(true, keyValue())
    const result = await api.testAI()
    show(result.message)
    emit('reload')
  } catch (e) {
    show('连接失败：' + e.message, true)
  } finally {
    busy.value = false
  }
}

async function onToggle() {
  if (busy.value) return
  busy.value = true
  show('')
  try {
    // 按钮文案由服务端返回；点下去就是取反。
    await save(!s.value.enabled, keyValue())
    emit('reload')
  } catch (e) {
    show('切换失败：' + e.message, true)
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
    show('清除失败：' + e.message, true)
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
      <SettingsTabs :tabs="tabs" :active="activeId" @select="selectTab" />

      <div class="settings-view__content">
        <div ref="scrollEl" class="settings-view__scroll">
          <div class="settings-view__pane">
            <header class="settings-view__heading">
              <h2 class="settings-view__heading-title">{{ activeTab.label }}</h2>
              <p class="settings-view__heading-desc">{{ activeHint }}</p>
            </header>

            <div class="settings-view__panels">
              <div
                v-show="activeId === 'ai'"
                id="settings-panel-ai"
                class="settings-view__panel"
                role="tabpanel"
                aria-labelledby="settings-tab-ai"
              >
                <SettingsGroup title="启用状态">
                  <div class="settings-view__state-row">
                    <div class="settings-view__state-text">
                      <p class="settings-view__state">{{ s.state }}</p>
                      <p class="settings-view__hint">
                        {{
                          s.enabled
                            ? 'AI 不会新增或改写规则，也不会改动已确定的归属。'
                            : '记录仍会保存；无法唯一归属的记录将保留为待归属状态。'
                        }}
                      </p>
                    </div>
                    <AppButton id="ai-toggle" :disabled="busy" @click="onToggle">
                      {{ s.toggle }}
                    </AppButton>
                  </div>
                </SettingsGroup>

                <SettingsGroup title="置信度阈值">
                  <FormField label="自动归属的最低置信度" for-id="ai-th" last>
                    <p class="settings-view__threshold">
                      当前阈值：<b id="ai-th-v">{{ threshold }}</b>
                    </p>
                    <input
                      id="ai-th"
                      v-model.number="threshold"
                      class="settings-view__range"
                      type="range"
                      min="0"
                      max="100"
                      step="5"
                    />
                    <p class="settings-view__hint">
                      低于该阈值时不会自动归属；阈值越高，自动归属越少。
                    </p>
                  </FormField>
                </SettingsGroup>
              </div>

              <div
                v-show="activeId === 'connection'"
                id="settings-panel-connection"
                class="settings-view__panel"
                role="tabpanel"
                aria-labelledby="settings-tab-connection"
              >
                <SettingsGroup title="服务接口" hint="OpenAI 兼容">
                  <FormField
                    label="接口地址"
                    for-id="ai-base"
                    hint="支持 DeepSeek、OpenAI、Moonshot 及本地部署模型。"
                  >
                    <AppInput
                      id="ai-base"
                      v-model="base"
                      type="text"
                      :placeholder="s.defaultBaseUrl"
                      spellcheck="false"
                    />
                  </FormField>

                  <FormField label="模型" for-id="ai-model" last>
                    <AppInput
                      id="ai-model"
                      v-model="model"
                      type="text"
                      :placeholder="s.defaultModel"
                      spellcheck="false"
                    />
                  </FormField>
                </SettingsGroup>

                <SettingsGroup title="API 密钥" hint="加密保存在本机">
                  <Notice v-if="s.keyBroken" tone="danger" class="settings-view__key-broken">
                    密钥读取失败，请重新填写。
                  </Notice>

                  <FormField
                    label="密钥"
                    for-id="ai-key"
                    hint="仅发送给上述接口。留空表示保留当前密钥。"
                    last
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
                        {{ armed ? '确认清除' : '清除密钥' }}
                      </AppButton>
                    </div>
                  </FormField>
                </SettingsGroup>
              </div>
            </div>
          </div>
        </div>

        <footer class="settings-view__footer">
          <div class="settings-view__footer-inner">
            <ActionBar class="settings-view__actions">
              <AppButton id="ai-save" variant="primary" :disabled="busy" @click="onSave">
                保存
              </AppButton>
              <AppButton id="ai-test" :disabled="busy" @click="onTest">测试连接</AppButton>
            </ActionBar>

            <Notice
              v-if="msg"
              id="ai-msg"
              :tone="bad ? 'danger' : 'muted'"
              class="settings-view__msg"
              aria-live="polite"
            >
              {{ msg }}
            </Notice>
          </div>
        </footer>
      </div>
    </div>
  </PageShell>
</template>

<style scoped lang="scss">
.settings-view__title {
  font-size: var(--font-size-7);
  font-weight: bold;
}

// 设置页：左边分类、右边内容。两边各自滚，操作条钉在内容区底部。
.settings-view {
  --settings-tabs-width: 224px;
  --settings-content-width: 720px;
  flex: 1;
  min-height: 0;
  display: flex;
}

.settings-view__content {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.settings-view__scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  padding: var(--space-5) var(--page-padding) var(--space-6);
  scrollbar-gutter: stable;
}

.settings-view__pane,
.settings-view__footer-inner {
  max-width: var(--settings-content-width);
  margin: 0 auto;
}

.settings-view__heading-title {
  font-size: var(--font-size-6);
  font-weight: bold;
}

.settings-view__heading-desc {
  margin-top: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-muted);
}

.settings-view__panels {
  margin-top: var(--space-5);
}

// 第三层：组里的设置项。组和组之间留出比组内更大的间距。
.settings-view__panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.settings-view__state-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
}

.settings-view__state-text {
  min-width: 0;
}

.settings-view__state {
  font-size: var(--font-size-3);
}

.settings-view__hint {
  margin-top: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-muted);
}

.settings-view__key-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.settings-view__key-broken {
  margin-bottom: var(--space-3);
}

.settings-view__state-row .app-button,
.settings-view__key-row .app-button {
  flex: none;
  white-space: nowrap;
}

// 滑块沿用目标色，与页面其他可操作控件一致。
.settings-view__range {
  width: 100%;
  accent-color: var(--blue);
}

.settings-view__threshold {
  margin-bottom: var(--space-1);
  font-size: var(--font-size-2);
  color: var(--text-muted);

  b {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
}

.settings-view__footer {
  flex: none;
  padding: var(--space-3) var(--page-padding);
  border-top: 1px solid var(--border);
  background: var(--surface);
}

.settings-view__footer-inner {
  display: flex;
  align-items: center;
  gap: var(--space-4);
}

.settings-view__actions {
  flex: none;
  margin-top: 0;
}

.settings-view__msg {
  flex: 1;
  min-width: 0;
}
</style>
