<script setup>
import { computed, ref, watch } from 'vue'
import { api } from '../api.js'

const props = defineProps({ page: { type: Object, required: true } })
const emit = defineEmits(['reload', 'nav'])

const s = computed(() => props.page.settings)
const base = ref('')
const model = ref('')
const key = ref('')
const th = ref(80)
const msg = ref('')
const bad = ref(false)
const armed = ref(false)
const busy = ref(false)

watch(
  s,
  (v) => {
    if (!v) return
    base.value = v.baseUrl
    model.value = v.model
    th.value = v.threshold
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
  const v = key.value.trim()
  return v === '' ? null : v
}

async function save(enabled, apiKey) {
  await api.saveSettings({
    enabled,
    baseUrl: base.value,
    model: model.value,
    threshold: Number(th.value),
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
    const r = await api.testAI()
    show(r.message)
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
  <div class="detail">
    <header class="hero">
      <div class="hero-top">
        <div>
          <a class="back" href="/" @click.prevent="emit('nav', '/')">← 返回主页</a>
          <div class="dtitle">设置</div>
        </div>
      </div>
    </header>

    <div class="settings">
      <section class="dsec">
        <h3>AI 判定<span>{{ s.state }}</span></h3>
        <p class="dwhy">{{ s.blurb }}</p>
        <div v-if="s.keyBroken" class="hint bad">密钥读不出来了，重新填一次就好。</div>

        <div class="field">
          <label for="ai-base">接口地址</label>
          <input
            id="ai-base"
            v-model="base"
            type="text"
            :placeholder="s.defaultBaseUrl"
            spellcheck="false"
          />
          <div class="fhint">OpenAI 兼容的地址。DeepSeek、OpenAI、Moonshot、本机跑的模型都行。</div>
        </div>

        <div class="field">
          <label for="ai-model">模型</label>
          <input id="ai-model" v-model="model" type="text" :placeholder="s.defaultModel" spellcheck="false" />
        </div>

        <div class="field">
          <label for="ai-key">API 密钥</label>
          <div class="frow">
            <input
              id="ai-key"
              v-model="key"
              type="password"
              value=""
              :placeholder="s.keyPlaceholder"
              autocomplete="off"
              spellcheck="false"
            />
            <button id="ai-key-clear" type="button" :class="{ armed }" :disabled="s.clearDisabled || busy" @click="onClear">
              {{ armed ? '再点一次' : '清掉' }}
            </button>
          </div>
          <div class="fhint">只存在这台电脑上，不会发到别处。留空就是不改动现在这把。</div>
        </div>

        <div class="field">
          <label for="ai-th">多大把握才自动选 <b id="ai-th-v">{{ th }}</b></label>
          <input id="ai-th" v-model.number="th" type="range" min="0" max="100" step="5" />
          <div class="fhint">没到这个把握就不替你决定，改成问你。定得越高，自动决定的越少。</div>
        </div>

        <div class="dacts">
          <button id="ai-save" type="button" :disabled="busy" @click="onSave">保存</button>
          <button id="ai-test" type="button" :disabled="busy" @click="onTest">测一下</button>
          <button id="ai-toggle" type="button" :disabled="busy" @click="onToggle">{{ s.toggle }}</button>
        </div>
        <div class="hint" :class="{ bad }" id="ai-msg">{{ msg }}</div>
      </section>
    </div>
  </div>
</template>
