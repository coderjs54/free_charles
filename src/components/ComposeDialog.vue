<script setup lang="ts">
import { api } from '../lib/api'
import { onMounted, reactive, ref } from 'vue'
import type { EntryMeta } from '../types/models'
import { useToast } from '../stores/useToast'
import { errorMessage } from '../utils/error'
import AppModal from './AppModal.vue'

const emit = defineEmits<{
  (event: 'close'): void
  (event: 'sent', meta: EntryMeta): void
}>()

const { push } = useToast()
const methods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
const form = reactive({
  method: 'GET',
  url: '',
  headers: 'Accept: */*\nUser-Agent: FreeCharles/1.0',
  body: ''
})
const errorText = ref('')
const sending = ref(false)
const urlInput = ref<HTMLInputElement | null>(null)

async function send(): Promise<void> {
  errorText.value = ''
  const url = form.url.trim()
  if (!url) {
    errorText.value = '请填写请求 URL'
    return
  }
  if (/^https:\/\//i.test(url)) {
    errorText.value = 'FreeCharles 仅支持发送 HTTP 请求，请去掉 URL 中的 https://'
    return
  }
  sending.value = true
  try {
    const meta = await api.compose({
      method: form.method,
      url,
      headers: form.headers,
      body: form.body
    })
    emit('sent', meta)
    push('请求已发送，正在等待响应', 'success')
    emit('close')
  } catch (error) {
    errorText.value = errorMessage(error)
  } finally {
    sending.value = false
  }
}

onMounted(() => urlInput.value?.focus())
</script>

<template>
  <AppModal title="新建请求" width="660px" @close="emit('close')">
    <div class="fc-compose">
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
        <span class="fc-compose-hint">请求体按 UTF-8 编码发送，仅支持 HTTP 地址</span>
      </label>

      <div v-if="errorText" class="fc-compose-error">{{ errorText }}</div>
    </div>

    <template #footer>
      <button class="fc-button" type="button" :disabled="sending" @click="emit('close')">取消</button>
      <button
        class="fc-button fc-button--primary"
        type="button"
        :disabled="sending"
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

.fc-compose-hint {
  color: $color-text-tertiary;
  font-size: 10.5px;
}

.fc-compose-hint code {
  padding: 0 3px;
  border-radius: 2px;
  background: $color-surface-muted;
  font-family: $font-mono;
}

.fc-compose-error {
  padding: 8px 10px;
  border: 1px solid rgba(192, 43, 43, 0.2);
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
