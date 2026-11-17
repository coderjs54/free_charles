<script setup lang="ts">
import { api } from '../lib/api'
import { computed, ref } from 'vue'
import type { EntryDetail, HeaderPair } from '../types/models'
import { buildRawRequest, buildRawResponse } from '../utils/curl'
import { errorMessage } from '../utils/error'
import { formatBytes } from '../utils/format'
import type { TabItem } from '../types/ui'
import { useToast } from '../stores/useToast'
import AppIcon from './AppIcon.vue'
import BodyViewer from './BodyViewer.vue'
import HeadersTable from './HeadersTable.vue'
import TabsBar from './TabsBar.vue'

interface CookieRow {
  name: string
  value: string
  attributes: string
}

const props = defineProps<{
  detail: EntryDetail
  side: 'request' | 'response'
}>()

const { push } = useToast()
const activeTab = ref('headers')

const isRequest = computed(() => props.side === 'request')
const headers = computed<HeaderPair[]>(() => (isRequest.value ? props.detail.requestHeaders : props.detail.responseHeaders))
const headerSize = computed(() => (isRequest.value ? props.detail.requestHeaderSize : props.detail.responseHeaderSize))
const body = computed(() => (isRequest.value ? props.detail.requestBody : props.detail.responseBody))
const mime = computed(() => (isRequest.value ? props.detail.requestMime : props.detail.responseMime))

const tabs = computed<TabItem[]>(() => {
  const base: TabItem[] = [
    { key: 'headers', label: 'Headers', badge: headers.value.length },
    ...(isRequest.value ? [{ key: 'query', label: 'Query' }] : []),
    { key: 'cookies', label: 'Cookies' },
    { key: 'body', label: 'Body' },
    { key: 'raw', label: 'Raw' }
  ]
  return base
})

const queryPairs = computed<HeaderPair[]>(() => {
  try {
    return [...new URL(props.detail.url).searchParams.entries()]
  } catch {
    return []
  }
})

// 请求侧解析 Cookie，响应侧解析可能重复出现的 Set-Cookie
const cookies = computed<CookieRow[]>(() => {
  if (isRequest.value) {
    const header = headers.value.find(([name]) => name.toLowerCase() === 'cookie')
    if (!header) return []
    return header[1]
      .split(';')
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) => {
        const index = part.indexOf('=')
        return index > 0
          ? { name: part.slice(0, index), value: part.slice(index + 1), attributes: '' }
          : { name: part, value: '', attributes: '' }
      })
  }

  const rows: CookieRow[] = []
  for (const [name, value] of headers.value) {
    if (name.toLowerCase() !== 'set-cookie') continue
    const segments = value.split(';').map((part) => part.trim())
    const first = segments.shift() ?? ''
    const index = first.indexOf('=')
    if (index <= 0) continue
    rows.push({
      name: first.slice(0, index),
      value: first.slice(index + 1),
      attributes: segments.join('; ')
    })
  }
  return rows
})

const rawText = computed(() => (isRequest.value ? buildRawRequest(props.detail) : buildRawResponse(props.detail)))

async function copyRaw(): Promise<void> {
  try {
    await api.copyText(rawText.value)
    push('已复制原始报文', 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}
</script>

<template>
  <div class="fc-message-panel">
    <TabsBar v-model="activeTab" :tabs="tabs" variant="sub" testid-prefix="subtab" />

    <div class="fc-message-body">
      <template v-if="activeTab === 'headers'">
        <div class="fc-panel-meta">
          <span>{{ headers.length }} 个头部</span>
          <span class="fc-panel-meta-dot">·</span>
          <span>{{ formatBytes(headerSize) }}</span>
        </div>
        <HeadersTable :headers="headers" :empty-text="isRequest ? '此请求没有头部' : '此响应没有头部'" />
      </template>

      <template v-else-if="activeTab === 'query'">
        <HeadersTable :headers="queryPairs" empty-text="URL 中没有查询参数" />
      </template>

      <template v-else-if="activeTab === 'cookies'">
        <div v-if="cookies.length > 0" class="fc-cookie-table">
          <div class="fc-cookie-row fc-cookie-row--head">
            <span class="fc-cookie-name">Name</span>
            <span class="fc-cookie-value">Value</span>
            <span class="fc-cookie-attrs">Attributes</span>
          </div>
          <div v-for="cookie in cookies" :key="`${cookie.name}-${cookie.value}`" class="fc-cookie-row">
            <span class="fc-cookie-name">{{ cookie.name }}</span>
            <span class="fc-cookie-value">{{ cookie.value }}</span>
            <span class="fc-cookie-attrs">{{ cookie.attributes || '—' }}</span>
          </div>
        </div>
        <div v-else class="fc-cookie-empty">
          {{ isRequest ? '此请求没有 Cookie' : '此响应没有 Set-Cookie' }}
        </div>
      </template>

      <template v-else-if="activeTab === 'body'">
        <BodyViewer
          :body="body"
          :side="side"
          :entry-id="detail.id"
          :mime="mime"
        />
      </template>

      <template v-else>
        <div class="fc-raw">
          <div class="fc-raw-toolbar">
            <span class="fc-raw-label">原始报文</span>
            <span class="fc-raw-hint">按捕获顺序展示头部与内容</span>
            <button class="fc-raw-copy" type="button" @click="copyRaw">
              <AppIcon name="copy" :size="13" />
              <span>复制</span>
            </button>
          </div>
          <pre class="fc-raw-code" data-testid="raw-text">{{ rawText }}</pre>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped lang="scss">
.fc-message-panel {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}

.fc-message-body {
  flex: 1;
}

.fc-panel-meta {
  display: flex;
  align-items: center;
  gap: $spacing-sm;
  padding: 7px $spacing-xl;
  border-bottom: 1px solid $color-divider;
  background: $color-surface;
  color: $color-text-tertiary;
  font-family: $font-mono;
  font-size: 10.5px;
}

.fc-panel-meta-dot {
  opacity: 0.6;
}

.fc-cookie-table {
  min-height: 100%;
}

.fc-cookie-row {
  display: grid;
  grid-template-columns: minmax(120px, 22%) minmax(160px, 34%) minmax(0, 1fr);
  gap: $spacing-lg;
  padding: 6px $spacing-xl;
  border-bottom: 1px solid $color-divider;
  font-family: $font-mono;
  font-size: 12px;
  line-height: 1.5;
}

.fc-cookie-row--head {
  position: sticky;
  top: 0;
  z-index: 1;
  background: $color-surface-muted;
  color: $color-text-tertiary;
  font-family: $font-ui;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  border-bottom-color: $color-border;
}

.fc-cookie-row:not(.fc-cookie-row--head):hover {
  background: #fafbfd;
}

.fc-cookie-name {
  color: $color-text-secondary;
  font-weight: 600;
  word-break: break-all;
}

.fc-cookie-value {
  color: $color-text;
  overflow-wrap: anywhere;
}

.fc-cookie-attrs {
  color: $color-text-tertiary;
  overflow-wrap: anywhere;
}

.fc-cookie-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 180px;
  color: $color-text-tertiary;
  font-size: $font-size-sm;
}

.fc-raw {
  min-height: 100%;
}

.fc-raw-toolbar {
  display: flex;
  align-items: center;
  gap: $spacing-md;
  padding: 6px $spacing-xl;
  border-bottom: 1px solid $color-divider;
  background: $color-surface-muted;
}

.fc-raw-label {
  color: $color-text-secondary;
  font-size: 11.5px;
  font-weight: 600;
}

.fc-raw-hint {
  color: $color-text-tertiary;
  font-size: 10.5px;
}

.fc-raw-copy {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 22px;
  margin-left: auto;
  padding: 0 7px;
  border-radius: $radius-xs;
  color: $color-text-secondary;
  font-size: 11px;
}

.fc-raw-copy:hover {
  background: $color-hover;
  color: $color-text;
}

.fc-raw-code {
  margin: 0;
  padding: $spacing-lg $spacing-xl;
  color: #23303f;
  font-family: $font-mono;
  font-size: 12.5px;
  line-height: 1.62;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  word-break: break-word;
}
</style>
