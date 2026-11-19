<script setup lang="ts">
import { api } from '../lib/api'
import { computed, onMounted, reactive, ref } from 'vue'
import type { ComposePayload, EntryMeta } from '../types/models'
import { useToast } from '../stores/useToast'
import { parseCurlCommand } from '../utils/curl'
import { errorMessage } from '../utils/error'
import AppModal from './AppModal.vue'

const emit = defineEmits<{
  (event: 'close'): void
  (event: 'sent', meta: EntryMeta): void
}>()

const { push } = useToast()
// 允许出现非标准方法（例如从 cURL 里粘贴 PROPFIND），因此用响应式数组
const methods = ref(['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'])
const form = reactive({
  method: 'GET',
  url: '',
  headers: 'Accept: */*\nUser-Agent: FreeCharles/1.0',
  body: ''
})

/** 表单 / cURL 两种录入方式；选择结果会记到 localStorage，下次打开沿用 */
const COMPOSE_MODE_KEY = 'freecharles:composeMode'

function readStoredMode(): 'form' | 'curl' {
  try {
    return localStorage.getItem(COMPOSE_MODE_KEY) === 'curl' ? 'curl' : 'form'
  } catch {
    // 隐私模式等场景下 localStorage 不可用，退回默认值
    return 'form'
  }
}

const mode = ref<'form' | 'curl'>(readStoredMode())
const curlText = ref('')
const curlResult = computed(() => parseCurlCommand(curlText.value))
const curlParsed = computed(() => curlResult.value.parsed)
const curlReady = computed(() => curlResult.value.ok && Boolean(curlParsed.value.url))
const curlBodySize = computed(() => new TextEncoder().encode(curlParsed.value.body).length)

const errorText = ref('')
const sending = ref(false)
const urlInput = ref<HTMLInputElement | null>(null)
const curlInput = ref<HTMLTextAreaElement | null>(null)

function switchMode(next: 'form' | 'curl'): void {
  mode.value = next
  try {
    localStorage.setItem(COMPOSE_MODE_KEY, next)
  } catch {
    // 存储失败不影响当前操作
  }
  requestAnimationFrame(() => {
    if (next === 'curl') curlInput.value?.focus()
    else urlInput.value?.focus()
  })
}

/** 把解析结果写回表单，便于继续微调 */
function applyToForm(): void {
  if (!curlReady.value) return
  const parsed = curlParsed.value
  if (!methods.value.includes(parsed.method)) methods.value = [...methods.value, parsed.method]
  form.method = parsed.method
  form.url = parsed.url
  form.headers = parsed.headers.map((header) => `${header.name}: ${header.value}`).join('\n')
  form.body = parsed.body
  switchMode('form')
}

async function send(): Promise<void> {
  errorText.value = ''
  let payload: ComposePayload

  if (mode.value === 'curl') {
    if (!curlResult.value.ok) {
      errorText.value = curlResult.value.error
      return
    }
    payload = {
      method: curlParsed.value.method,
      url: curlParsed.value.url,
      headers: curlParsed.value.headers.map((header) => `${header.name}: ${header.value}`).join('\n'),
      body: curlParsed.value.body
    }
  } else {
    const url = form.url.trim()
    if (!url) {
      errorText.value = '请填写请求 URL'
      return
    }
    payload = { method: form.method, url, headers: form.headers, body: form.body }
  }

  // 缺省协议按 http 处理，其余协议直接拒绝
  const normalized = /^[a-z]+:\/\//i.test(payload.url) ? payload.url : `http://${payload.url}`
  let scheme = ''
  try {
    scheme = new URL(normalized).protocol.replace(':', '').toLowerCase()
  } catch {
    errorText.value = `URL 格式不正确：${normalized}`
    return
  }
  if (scheme !== 'http' && scheme !== 'https') {
    errorText.value = '仅支持 http / https 请求'
    return
  }

  sending.value = true
  try {
    const meta = await api.compose({ ...payload, url: normalized })
    emit('sent', meta)
    push('请求已发送，正在等待响应', 'success')
    emit('close')
  } catch (error) {
    errorText.value = errorMessage(error)
  } finally {
    sending.value = false
  }
}

onMounted(() => {
  // 打开时按上次选择聚焦到对应输入框
  if (mode.value === 'curl') curlInput.value?.focus()
  else urlInput.value?.focus()
})
</script>

<template>
  <AppModal title="新建请求" width="680px" @close="emit('close')">
    <div class="fc-compose">
      <div class="fc-compose-tabs" role="tablist">
        <button
          class="fc-compose-tab"
          :class="{ 'fc-compose-tab--active': mode === 'form' }"
          type="button"
          role="tab"
          :aria-selected="mode === 'form'"
          data-testid="compose-tab-form"
          @click="switchMode('form')"
        >
          手动填写
        </button>
        <button
          class="fc-compose-tab"
          :class="{ 'fc-compose-tab--active': mode === 'curl' }"
          type="button"
          role="tab"
          :aria-selected="mode === 'curl'"
          data-testid="compose-tab-curl"
          @click="switchMode('curl')"
        >
          粘贴 cURL
        </button>
      </div>

      <template v-if="mode === 'form'">
        <div class="fc-compose-line">
          <select v-model="form.method" class="fc-compose-method">
            <option v-for="method in methods" :key="method" :value="method">{{ method }}</option>
          </select>
          <input
            ref="urlInput"
            v-model="form.url"
            class="fc-compose-url"
            type="text"
            placeholder="http://example.com/api/users"
            spellcheck="false"
            data-testid="compose-url"
            @keydown.enter.prevent="send"
          />
        </div>

        <label class="fc-compose-field">
          <span class="fc-compose-label">Headers</span>
          <textarea
            v-model="form.headers"
            class="fc-compose-textarea"
            rows="5"
            spellcheck="false"
            placeholder="Name: value"
          />
          <span class="fc-compose-hint">每行一个 Header，格式为 <code>Name: value</code></span>
        </label>

        <label class="fc-compose-field">
          <span class="fc-compose-label">Body</span>
          <textarea
            v-model="form.body"
            class="fc-compose-textarea fc-compose-textarea--body"
            rows="7"
            spellcheck="false"
            placeholder='{"hello": "world"}'
          />
          <span class="fc-compose-hint">请求体按 UTF-8 编码发送，支持 http / https 地址</span>
        </label>
      </template>

      <template v-else>
        <label class="fc-compose-field">
          <span class="fc-compose-label">cURL 命令</span>
          <textarea
            ref="curlInput"
            v-model="curlText"
            class="fc-compose-textarea fc-compose-textarea--curl"
            rows="8"
            spellcheck="false"
            data-testid="compose-curl-input"
            placeholder="curl 'http://example.com/api/users' -X POST -H 'Content-Type: application/json' --data-raw '{&quot;name&quot;:&quot;demo&quot;}'"
            @keydown.ctrl.enter.prevent="send"
          />
          <span class="fc-compose-hint">
            支持 Chrome DevTools 的「Copy as cURL (bash)」与「Copy as cURL (cmd)」两种格式，兼容
            <code>-X</code>、<code>-H</code>、<code>-d</code>、<code>-u</code>、<code>-b</code>、
            <code>--json</code> 等参数与 <code>\</code> / <code>^</code> 续行符；Ctrl+Enter 发送
          </span>
        </label>

        <div v-if="curlReady" class="fc-curl-preview">
          <div class="fc-curl-preview-item">
            <span class="fc-curl-preview-label">方法</span>
            <code class="fc-curl-preview-value">{{ curlParsed.method }}</code>
          </div>
          <div class="fc-curl-preview-item fc-curl-preview-item--wide">
            <span class="fc-curl-preview-label">URL</span>
            <code class="fc-curl-preview-value" :title="curlParsed.url">{{ curlParsed.url }}</code>
          </div>
          <div class="fc-curl-preview-item">
            <span class="fc-curl-preview-label">Headers</span>
            <code class="fc-curl-preview-value">{{ curlParsed.headers.length }} 个</code>
          </div>
          <div class="fc-curl-preview-item">
            <span class="fc-curl-preview-label">Body</span>
            <code class="fc-curl-preview-value">{{ curlBodySize }} B</code>
          </div>
        </div>

        <div v-if="curlReady && curlParsed.warnings.length > 0" class="fc-curl-warnings">
          <p v-for="warning in curlParsed.warnings" :key="warning">{{ warning }}</p>
        </div>

        <div v-if="curlReady" class="fc-curl-actions">
          <button class="fc-button" type="button" @click="applyToForm">填入表单继续编辑</button>
        </div>
      </template>

      <div v-if="errorText" class="fc-compose-error">{{ errorText }}</div>
    </div>

    <template #footer>
      <button class="fc-button" type="button" :disabled="sending" @click="emit('close')">取消</button>
      <button
        class="fc-button fc-button--primary"
        type="button"
        :disabled="sending || (mode === 'curl' && !curlReady)"
        data-testid="compose-send"
        @click="send"
      >
        {{ sending ? '发送中…' : '发送请求' }}
      </button>
    </template>
  </AppModal>
</template>

<style scoped lang="scss">
.fc-compose {
  display: flex;
  flex-direction: column;
  gap: $spacing-lg;
}

.fc-compose-tabs {
  display: inline-flex;
  align-self: flex-start;
  padding: 2px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface-muted;
}

.fc-compose-tab {
  height: 26px;
  padding: 0 14px;
  border-radius: $radius-xs;
  color: $color-text-secondary;
  font-size: 12px;
  font-weight: 500;
  transition: background $transition-fast, color $transition-fast, box-shadow $transition-fast;
}

.fc-compose-tab:hover {
  color: $color-text;
}

.fc-compose-tab--active {
  background: $color-surface;
  color: $color-primary;
  box-shadow: $shadow-sm;
}

.fc-compose-line {
  display: flex;
  gap: $spacing-md;
}

.fc-compose-method {
  width: 92px;
  height: 34px;
  padding: 0 8px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text;
  font-family: $font-mono;
  font-size: 12px;
  font-weight: 600;
}

.fc-compose-url {
  flex: 1;
  height: 34px;
  padding: 0 10px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text;
  font-family: $font-mono;
  font-size: 12.5px;
  transition: border-color $transition-fast, box-shadow $transition-fast;
}

.fc-compose-url:focus,
.fc-compose-method:focus,
.fc-compose-textarea:focus {
  border-color: rgba(37, 99, 235, 0.6);
  box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.1);
}

.fc-compose-url::placeholder,
.fc-compose-textarea::placeholder {
  color: $color-text-tertiary;
}

.fc-compose-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.fc-compose-label {
  color: $color-text-secondary;
  font-size: 12.5px;
  font-weight: 600;
}

.fc-compose-textarea {
  width: 100%;
  padding: 8px 10px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text;
  font-family: $font-mono;
  font-size: 12px;
  line-height: 1.6;
  resize: vertical;
  transition: border-color $transition-fast, box-shadow $transition-fast;
}

.fc-compose-textarea--body {
  min-height: 120px;
}

.fc-compose-textarea--curl {
  min-height: 168px;
}

.fc-compose-hint {
  color: $color-text-tertiary;
  font-size: 10.5px;
  line-height: 1.6;
}

.fc-compose-hint code {
  padding: 0 3px;
  border-radius: 2px;
  background: $color-surface-muted;
  font-family: $font-mono;
}

.fc-curl-preview {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: $spacing-md $spacing-lg;
  padding: 9px 11px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface-muted;
}

.fc-curl-preview-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.fc-curl-preview-item--wide {
  grid-column: 1 / -1;
}

.fc-curl-preview-label {
  color: $color-text-tertiary;
  font-size: 10.5px;
}

.fc-curl-preview-value {
  overflow: hidden;
  color: $color-text;
  font-family: $font-mono;
  font-size: 11.5px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-curl-warnings {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 8px 10px;
  border: 1px solid $color-warning-border;
  border-radius: $radius-sm;
  background: $color-warning-soft;
  color: $color-warning;
  font-size: 11.5px;
  line-height: 1.5;
}

.fc-curl-actions {
  display: flex;
  justify-content: flex-end;
}

.fc-compose-error {
  padding: 8px 10px;
  border: 1px solid $color-danger-border;
  border-radius: $radius-sm;
  background: $color-danger-soft;
  color: $color-danger;
  font-size: 12px;
  line-height: 1.5;
}

.fc-button {
  height: 30px;
  padding: 0 14px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text-secondary;
  font-size: 12.5px;
  font-weight: 500;
}

.fc-button:hover {
  border-color: $color-border-strong;
  background: $color-surface-muted;
}

.fc-button--primary {
  border-color: $color-primary;
  background: $color-primary;
  color: #fff;
}

.fc-button--primary:hover {
  border-color: $color-primary-hover;
  background: $color-primary-hover;
}

.fc-button:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
