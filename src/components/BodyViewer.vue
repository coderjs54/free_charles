<script setup lang="ts">
import { api } from '../lib/api'
import { computed, ref, watch } from 'vue'
import type { BodyView } from '../types/models'
import { useToast } from '../stores/useToast'
import { errorMessage } from '../utils/error'
import { formatBytes } from '../utils/format'
import { formatHexDump, highlightJson, prettyJson, toDataUrl } from '../utils/highlight'
import AppIcon from './AppIcon.vue'

const props = defineProps<{
  body: BodyView
  side: 'request' | 'response'
  entryId: string
  mime: string
}>()

const { push } = useToast()
const wrap = ref(true)
const jsonMode = ref<'pretty' | 'raw'>('pretty')
const showHexForImage = ref(false)
const htmlPreview = ref(false)
const saving = ref(false)

// 大 Body 由父级传入 deferred 占位，组件挂载后单独拉取完整内容
const loadedBody = ref<BodyView | null>(null)
const loadingBody = ref(false)
const bodyLoadError = ref('')
const activeBody = computed<BodyView>(() => loadedBody.value ?? props.body)
let bodyToken = 0

const effectiveMime = computed(() => activeBody.value.mime || props.mime || '')
const isJson = computed(() => {
  if (activeBody.value.kind !== 'text') return false
  const mime = effectiveMime.value.toLowerCase()
  return mime.includes('json') || /^\s*[{[][\s\S]*$/.test(activeBody.value.text ?? '')
})
const json = computed(() => prettyJson(activeBody.value.text ?? ''))
const formattedText = computed(() =>
  isJson.value && jsonMode.value === 'pretty' && json.value.ok ? json.value.text : activeBody.value.text ?? ''
)
const highlighted = computed(() =>
  isJson.value && jsonMode.value === 'pretty' && json.value.ok ? highlightJson(formattedText.value) : ''
)
const isHtml = computed(() => activeBody.value.kind === 'text' && effectiveMime.value.toLowerCase().includes('html'))
const isImage = computed(
  () =>
    activeBody.value.kind === 'binary' &&
    effectiveMime.value.toLowerCase().startsWith('image/') &&
    Boolean(activeBody.value.base64)
)
const imageUrl = computed(() =>
  isImage.value && activeBody.value.base64 ? toDataUrl(effectiveMime.value, activeBody.value.base64) : ''
)
const hexText = computed(() => (activeBody.value.base64 ? formatHexDump(activeBody.value.base64) : ''))
const showText = computed(() => activeBody.value.kind === 'text')
const showImage = computed(() => isImage.value && !showHexForImage.value)
const showHex = computed(() => activeBody.value.kind === 'binary' && !showImage.value)

const emptyText = computed(() => {
  if (activeBody.value.size > 0 && !activeBody.value.available) return '该内容未被捕获'
  return props.side === 'request' ? '此请求没有请求体' : '此响应没有内容'
})

/** 拉取延迟加载的大 Body */
async function loadDeferredBody(): Promise<void> {
  if (!props.body.deferred) return
  const token = ++bodyToken
  loadingBody.value = true
  bodyLoadError.value = ''
  try {
    const result = await api.getEntryBody(props.entryId, props.side)
    if (token !== bodyToken) return
    if (result) loadedBody.value = result
    else bodyLoadError.value = '内容已不存在'
  } catch (error) {
    if (token === bodyToken) bodyLoadError.value = errorMessage(error)
  } finally {
    if (token === bodyToken) loadingBody.value = false
  }
}

watch(
  () => [props.entryId, props.side, props.body.deferred] as const,
  () => {
    bodyToken += 1
    loadedBody.value = null
    bodyLoadError.value = ''
    loadingBody.value = false
    if (props.body.deferred) void loadDeferredBody()
  },
  { immediate: true }
)

async function copyBody(): Promise<void> {
  if (activeBody.value.kind !== 'text' || !activeBody.value.text) {
    push('二进制内容无法直接复制，请使用保存', 'error')
    return
  }
  try {
    await api.copyText(activeBody.value.text)
    push('已复制内容', 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

async function saveBody(): Promise<void> {
  if (saving.value) return
  saving.value = true
  try {
    const result = await api.saveBody({ id: props.entryId, side: props.side })
    if (result) push(`已保存到 ${result.path}`, 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="fc-body-viewer">
    <div class="fc-body-toolbar">
      <div class="fc-body-meta">
        <span class="fc-body-mime">{{ effectiveMime || 'unknown' }}</span>
        <span v-if="activeBody.encoding" class="fc-body-badge">{{ activeBody.encoding }}</span>
        <span v-if="activeBody.truncated" class="fc-body-badge fc-body-badge--warning">已截断</span>
        <span class="fc-body-size">
          {{ formatBytes(activeBody.size) }}<template v-if="activeBody.encoding"> · 解码后 {{ formatBytes(activeBody.decodedSize) }}</template>
        </span>
      </div>

      <div class="fc-body-actions">
        <div v-if="isJson && json.ok" class="fc-body-segment">
          <button
            class="fc-body-segment-button"
            :class="{ 'fc-body-segment-button--active': jsonMode === 'pretty' }"
            type="button"
            @click="jsonMode = 'pretty'"
          >
            格式化
          </button>
          <button
            class="fc-body-segment-button"
            :class="{ 'fc-body-segment-button--active': jsonMode === 'raw' }"
            type="button"
            @click="jsonMode = 'raw'"
          >
            原始
          </button>
        </div>
        <div v-if="isHtml" class="fc-body-segment">
          <button
            class="fc-body-segment-button"
            :class="{ 'fc-body-segment-button--active': !htmlPreview }"
            type="button"
            @click="htmlPreview = false"
          >
            源码
          </button>
          <button
            class="fc-body-segment-button"
            :class="{ 'fc-body-segment-button--active': htmlPreview }"
            type="button"
            @click="htmlPreview = true"
          >
            预览
          </button>
        </div>
        <div v-if="isImage" class="fc-body-segment">
          <button
            class="fc-body-segment-button"
            :class="{ 'fc-body-segment-button--active': !showHexForImage }"
            type="button"
            @click="showHexForImage = false"
          >
            图片
          </button>
          <button
            class="fc-body-segment-button"
            :class="{ 'fc-body-segment-button--active': showHexForImage }"
            type="button"
            @click="showHexForImage = true"
          >
            Hex
          </button>
        </div>
        <button
          v-if="showText"
          class="fc-body-tool"
          :class="{ 'fc-body-tool--active': wrap }"
          type="button"
          title="自动换行"
          aria-label="自动换行"
          @click="wrap = !wrap"
        >
          <AppIcon name="wrap" :size="14" />
        </button>
        <button class="fc-body-tool" type="button" title="复制内容" aria-label="复制内容" :disabled="activeBody.kind !== 'text'" @click="copyBody">
          <AppIcon name="copy" :size="14" />
        </button>
        <button class="fc-body-tool" type="button" title="保存内容" aria-label="保存内容" :disabled="!activeBody.available || saving" @click="saveBody">
          <AppIcon name="download" :size="14" />
        </button>
      </div>
    </div>

    <div v-if="loadingBody" class="fc-body-empty">
      <span class="fc-body-empty-icon"><AppIcon name="file" :size="20" /></span>
      <span>正在加载内容…</span>
    </div>

    <div v-else-if="bodyLoadError" class="fc-body-empty">
      <span class="fc-body-empty-icon"><AppIcon name="alert" :size="20" /></span>
      <span>{{ bodyLoadError }}</span>
    </div>

    <div v-else-if="activeBody.kind === 'empty'" class="fc-body-empty">
      <span class="fc-body-empty-icon"><AppIcon name="file" :size="20" /></span>
      <span>{{ emptyText }}</span>
    </div>

    <div v-else class="fc-body-content">
      <template v-if="showText">
        <iframe
          v-if="isHtml && htmlPreview"
          class="fc-body-frame"
          :srcdoc="activeBody.text ?? ''"
          sandbox=""
          title="HTML 预览"
        />
        <pre
          v-else-if="isJson && jsonMode === 'pretty' && json.ok"
          class="fc-body-code fc-body-code--tokens"
          :class="{ 'fc-body-code--wrap': wrap }"
          data-testid="body-text"
          v-html="highlighted"
        />
        <pre
          v-else
          class="fc-body-code"
          :class="{ 'fc-body-code--wrap': wrap }"
          data-testid="body-text"
        >{{ activeBody.text }}</pre>
      </template>

      <img v-if="showImage" class="fc-body-image" :src="imageUrl" alt="响应图片预览" />
      <pre v-if="showHex" class="fc-body-code fc-body-code--hex" data-testid="body-text">{{ hexText }}</pre>

      <div v-if="activeBody.truncated" class="fc-body-notice">
        内容超过捕获上限，仅显示前 {{ formatBytes(activeBody.capturedSize) }}。可在设置中调整单个 Body 上限。
      </div>
      <div v-if="activeBody.textTruncated" class="fc-body-notice">内容过长，展示文本已截断。</div>
      <div v-if="activeBody.decodeError" class="fc-body-notice fc-body-notice--error">{{ activeBody.decodeError }}</div>
    </div>
  </div>
</template>

<style scoped lang="scss">
.fc-body-viewer {
  display: flex;
  flex-direction: column;
  min-height: 100%;
  background: $color-surface;
}

.fc-body-toolbar {
  display: flex;
  align-items: center;
  gap: $spacing-md;
  min-height: 36px;
  padding: 4px $spacing-xl;
  border-bottom: 1px solid $color-divider;
  background: $color-surface-muted;
  flex: none;
}

.fc-body-meta {
  display: flex;
  align-items: center;
  gap: $spacing-sm;
  min-width: 0;
  overflow: hidden;
}

.fc-body-mime {
  overflow: hidden;
  color: $color-text-secondary;
  font-family: $font-mono;
  font-size: 11.5px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-body-badge {
  padding: 1px 5px;
  border-radius: $radius-xs;
  background: $color-primary-soft;
  color: $color-primary;
  font-family: $font-mono;
  font-size: 10px;
  font-weight: 600;
}

.fc-body-badge--warning {
  background: $color-warning-soft;
  color: $color-warning;
}

.fc-body-size {
  color: $color-text-tertiary;
  font-family: $font-mono;
  font-size: 10.5px;
  white-space: nowrap;
}

.fc-body-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-left: auto;
  flex: none;
}

.fc-body-segment {
  display: flex;
  gap: 1px;
  margin-right: $spacing-xs;
  padding: 2px;
  border-radius: $radius-xs;
  background: $color-code-bg;
}

.fc-body-segment-button {
  height: 20px;
  padding: 0 7px;
  border-radius: 3px;
  color: $color-text-tertiary;
  font-size: 11px;
}

.fc-body-segment-button:hover {
  color: $color-text-secondary;
}

.fc-body-segment-button--active {
  background: $color-surface;
  color: $color-text;
  box-shadow: $shadow-sm;
}

.fc-body-tool {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: $radius-xs;
  color: $color-text-tertiary;
}

.fc-body-tool:hover {
  background: $color-hover;
  color: $color-text;
}

.fc-body-tool--active {
  color: $color-primary;
  background: $color-primary-soft;
}

.fc-body-tool:disabled {
  opacity: 0.4;
  cursor: not-allowed;
  background: transparent;
}

.fc-body-content {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow: auto;
  background: $color-surface;
}

.fc-body-code {
  margin: 0;
  padding: $spacing-lg $spacing-xl;
  color: $color-code-text;
  font-family: $font-mono;
  font-size: 12.5px;
  line-height: 1.62;
  tab-size: 2;
  white-space: pre;
  overflow: auto;
}

.fc-body-code--wrap {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  word-break: break-word;
}

.fc-body-code--hex {
  color: $color-code-text-muted;
  font-size: 12px;
  line-height: 1.55;
}

.fc-body-code--tokens :deep(.fc-token-key) {
  color: $color-json-key;
}

.fc-body-code--tokens :deep(.fc-token-string) {
  color: $color-json-string;
}

.fc-body-code--tokens :deep(.fc-token-number) {
  color: $color-json-number;
}

.fc-body-code--tokens :deep(.fc-token-boolean) {
  color: $color-json-boolean;
}

.fc-body-code--tokens :deep(.fc-token-null) {
  color: $color-json-null;
}

.fc-body-image {
  display: block;
  max-width: calc(100% - 48px);
  max-height: 60vh;
  margin: $spacing-xl auto;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background-color: $color-image-bg;
  background-image: linear-gradient(45deg, $color-checker 25%, transparent 25%),
    linear-gradient(-45deg, $color-checker 25%, transparent 25%),
    linear-gradient(45deg, transparent 75%, $color-checker 75%),
    linear-gradient(-45deg, transparent 75%, $color-checker 75%);
  background-size: 14px 14px;
  background-position: 0 0, 0 7px, 7px -7px, -7px 0;
  box-shadow: $shadow-sm;
}

.fc-body-frame {
  width: 100%;
  height: 100%;
  min-height: 420px;
  border: 0;
  background: #fff;
}

.fc-body-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: $spacing-md;
  min-height: 300px;
  height: 100%;
  color: $color-text-tertiary;
  font-size: $font-size-sm;
}

.fc-body-empty-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 42px;
  height: 42px;
  border: 1px solid $color-border;
  border-radius: $radius-md;
  background: $color-surface-muted;
}

.fc-body-notice {
  margin: 0 $spacing-xl $spacing-lg;
  padding: 7px 10px;
  border: 1px solid $color-warning-border;
  border-radius: $radius-sm;
  background: $color-warning-soft;
  color: $color-warning;
  font-size: 11.5px;
  line-height: 1.6;
}

.fc-body-notice--error {
  border-color: $color-danger-border;
  background: $color-danger-soft;
  color: $color-danger;
}
</style>
