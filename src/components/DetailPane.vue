<script setup lang="ts">
import { api } from '../lib/api'
import { computed, ref } from 'vue'
import { useSession } from '../stores/useSession'
import { useToast } from '../stores/useToast'
import { buildCurl } from '../utils/curl'
import { errorMessage } from '../utils/error'
import {
  formatBytes,
  formatDateTime,
  formatDuration,
  hostLabel,
  methodTone,
  statusFullText,
  statusTone
} from '../utils/format'
import type { TabItem } from '../types/ui'
import AppIcon from './AppIcon.vue'
import MessagePanel from './MessagePanel.vue'
import OverviewPanel from './OverviewPanel.vue'
import TabsBar from './TabsBar.vue'

const { detail, loadingDetail, selectedId, proxyAddress, systemProxyState } = useSession()
const { push } = useToast()
const mainTab = ref('overview')

const tabs: TabItem[] = [
  { key: 'overview', label: 'Overview' },
  { key: 'request', label: 'Request' },
  { key: 'response', label: 'Response' }
]

const canRepeat = computed(
  () =>
    Boolean(detail.value) &&
    detail.value!.state !== 'pending' &&
    detail.value!.method !== 'CONNECT' &&
    !detail.value!.tunnel &&
    !detail.value!.upgrade
)

const tone = computed(() => (detail.value ? statusTone(detail.value) : 'muted'))
const methodClass = computed(() => (detail.value ? `fc-method-pill--${methodTone(detail.value.method)}` : ''))

async function copy(text: string, message: string): Promise<void> {
  try {
    await api.copyText(text)
    push(message, 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

function copyUrl(): void {
  if (detail.value) void copy(detail.value.url, '已复制 URL')
}

function copyCurl(): void {
  if (detail.value) void copy(buildCurl(detail.value), '已复制 cURL 命令')
}

function copyProxy(): void {
  void copy(proxyAddress.value, `已复制代理地址 ${proxyAddress.value}`)
}

async function repeat(): Promise<void> {
  if (!detail.value) return
  try {
    const meta = await api.repeatEntry(detail.value.id)
    await useSession().select(meta.id)
    push('已重放请求，新条目已添加到会话', 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}
</script>

<template>
  <section class="fc-detail">
    <template v-if="detail">
      <header class="fc-detail-head">
        <div class="fc-detail-head-top">
          <span class="fc-method-pill" :class="methodClass">{{ detail.method }}</span>
          <span class="fc-detail-url" :title="detail.url" data-testid="detail-url">{{ detail.url }}</span>
          <span class="fc-status-pill" :class="`fc-status-pill--${tone}`">{{ statusFullText(detail) }}</span>
          <div class="fc-detail-actions">
            <button v-if="canRepeat" class="fc-detail-action" type="button" title="重放请求" aria-label="重放请求" @click="repeat">
              <AppIcon name="repeat" :size="14" />
            </button>
            <button class="fc-detail-action" type="button" title="复制 URL" aria-label="复制 URL" @click="copyUrl">
              <AppIcon name="copy" :size="14" />
            </button>
            <button v-if="canRepeat" class="fc-detail-action" type="button" title="复制为 cURL" aria-label="复制为 cURL" @click="copyCurl">
              <AppIcon name="code" :size="14" />
            </button>
          </div>
        </div>
        <div class="fc-detail-sub">
          <span class="fc-detail-sub-item">{{ hostLabel(detail) }}</span>
          <span class="fc-detail-sub-sep">·</span>
          <span class="fc-detail-sub-item">{{ formatDateTime(detail.startTime) }}</span>
          <span class="fc-detail-sub-sep">·</span>
          <span class="fc-detail-sub-item">{{ formatDuration(detail.duration) }}</span>
          <span class="fc-detail-sub-sep">·</span>
          <span class="fc-detail-sub-item">{{ formatBytes(detail.requestSize + detail.responseSize) }}</span>
          <span class="fc-detail-sub-sep">·</span>
          <span class="fc-detail-sub-item">from {{ detail.clientIp }}</span>
          <span v-if="detail.tunnel" class="fc-detail-tag">
            <AppIcon name="lock" :size="10" />
            HTTPS 隧道
          </span>
          <span v-else-if="detail.decrypted" class="fc-detail-tag fc-detail-tag--decrypted">
            <AppIcon name="lock" :size="10" />
            HTTPS 解密
          </span>
        </div>
      </header>

      <TabsBar v-model="mainTab" :tabs="tabs" testid-prefix="tab" />

      <div class="fc-detail-scroll">
        <OverviewPanel v-if="mainTab === 'overview'" :detail="detail" />
        <MessagePanel v-else-if="mainTab === 'request'" :detail="detail" side="request" />
        <MessagePanel v-else :detail="detail" side="response" />
      </div>
    </template>

    <div v-else-if="loadingDetail || selectedId" class="fc-detail-skeleton">
      <div class="fc-skeleton-line fc-skeleton-line--title" />
      <div class="fc-skeleton-line fc-skeleton-line--sub" />
      <div class="fc-skeleton-tabs">
        <span v-for="index in 3" :key="index" class="fc-skeleton-tab" />
      </div>
      <div class="fc-skeleton-card" />
      <div class="fc-skeleton-card fc-skeleton-card--short" />
    </div>

    <div v-else class="fc-detail-empty">
      <div class="fc-empty-art">
        <svg width="168" height="118" viewBox="0 0 168 118" fill="none" aria-hidden="true">
          <rect x="22" y="52" width="28" height="20" rx="3" stroke="var(--fc-color-border-strong)" stroke-width="1.4" />
          <path d="M36 42v10" stroke="var(--fc-color-border-strong)" stroke-width="1.4" stroke-linecap="round" />
          <circle cx="36" cy="38" r="3" fill="var(--fc-color-border-strong)" />
          <rect x="118" y="52" width="28" height="20" rx="3" stroke="var(--fc-color-border-strong)" stroke-width="1.4" />
          <path d="M132 42v10" stroke="var(--fc-color-border-strong)" stroke-width="1.4" stroke-linecap="round" />
          <circle cx="132" cy="38" r="3" fill="var(--fc-color-border-strong)" />
          <rect x="65" y="43" width="38" height="38" rx="8" fill="var(--fc-color-primary-soft)" stroke="var(--fc-color-timeline-soft)" stroke-width="1.4" />
          <path d="M74 53h6M74 60h20M74 67h14" stroke="var(--fc-color-timeline)" stroke-width="1.6" stroke-linecap="round" />
          <path d="M50 62h15M103 62h15" stroke="var(--fc-color-border-strong)" stroke-width="1.4" stroke-dasharray="3 4" />
          <circle cx="84" cy="26" r="4" fill="var(--fc-color-timeline-soft)" />
          <path d="M84 30v13" stroke="var(--fc-color-border-strong)" stroke-width="1.4" stroke-dasharray="3 4" />
          <path d="M50 26h62" stroke="var(--fc-color-divider)" stroke-width="1.4" stroke-linecap="round" />
          <circle cx="50" cy="26" r="2.5" fill="var(--fc-color-border-strong)" />
          <circle cx="112" cy="26" r="2.5" fill="var(--fc-color-border-strong)" />
        </svg>
      </div>
      <h2 class="fc-detail-empty-title">选择一个请求查看详情</h2>
      <p class="fc-detail-empty-text">
        在左侧 Structure 或 Sequence 列表中点击请求，这里会展示 Overview、请求头、响应头与内容。
      </p>

      <div class="fc-detail-proxy-card">
        <div class="fc-detail-proxy-row">
          <span class="fc-detail-proxy-label">HTTP 代理</span>
          <code class="fc-detail-proxy-address">{{ proxyAddress }}</code>
          <button class="fc-detail-proxy-copy" type="button" @click="copyProxy">
            <AppIcon name="copy" :size="12" />
            复制
          </button>
        </div>
        <div v-if="systemProxyState.active" class="fc-detail-proxy-hint fc-detail-proxy-hint--active">
          系统代理已自动接管，直接访问以 http:// 开头的页面即可。停止代理或退出应用会自动恢复原来的系统代理设置。
        </div>
        <div v-else class="fc-detail-proxy-hint">
          将浏览器或系统的 HTTP 代理设置为上述地址。HTTPS 默认以 CONNECT 隧道显示；如需查看内容，请在设置中开启 HTTPS 解密。
        </div>
        <code class="fc-detail-curl">curl -x http://{{ proxyAddress }} http://example.com</code>
      </div>
    </div>
  </section>
</template>

<style scoped lang="scss">
.fc-detail {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-width: 0;
  background: $color-surface;
}

.fc-detail-head {
  padding: 10px $spacing-xl 8px;
  border-bottom: 1px solid $color-divider;
  flex: none;
}

.fc-detail-head-top {
  display: flex;
  align-items: center;
  gap: $spacing-md;
  min-width: 0;
}

.fc-method-pill {
  flex: none;
  padding: 2px 6px;
  border-radius: $radius-xs;
  background: $color-surface-sunken;
  color: $color-text-secondary;
  font-family: $font-mono;
  font-size: 10.5px;
  font-weight: 700;
  letter-spacing: 0.02em;
}

.fc-method-pill--info {
  background: $color-info-soft;
  color: $color-method-get;
}

.fc-method-pill--success {
  background: $color-success-soft;
  color: $color-method-post;
}

.fc-method-pill--warning {
  background: $color-warning-soft;
  color: $color-method-put;
}

.fc-method-pill--danger {
  background: $color-danger-soft;
  color: $color-method-delete;
}

.fc-method-pill--muted {
  background: $color-surface-sunken;
  color: $color-method-connect;
}

.fc-detail-url {
  flex: 1;
  overflow: hidden;
  color: $color-text;
  font-family: $font-mono;
  font-size: 12.5px;
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-status-pill {
  flex: none;
  padding: 2px 7px;
  border-radius: $radius-xs;
  background: $color-surface-sunken;
  color: $color-text-secondary;
  font-family: $font-mono;
  font-size: 10.5px;
  font-weight: 600;
}

.fc-status-pill--success {
  background: $color-success-soft;
  color: $color-success;
}

.fc-status-pill--info {
  background: $color-info-soft;
  color: $color-info;
}

.fc-status-pill--warning {
  background: $color-warning-soft;
  color: $color-warning;
}

.fc-status-pill--danger {
  background: $color-danger-soft;
  color: $color-danger;
}

.fc-status-pill--muted {
  background: $color-surface-sunken;
  color: $color-text-tertiary;
}

.fc-detail-actions {
  display: flex;
  gap: 1px;
  flex: none;
}

.fc-detail-action {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 25px;
  height: 25px;
  border-radius: $radius-xs;
  color: $color-text-tertiary;
}

.fc-detail-action:hover {
  background: $color-hover;
  color: $color-text;
}

.fc-detail-sub {
  display: flex;
  align-items: center;
  gap: 5px;
  margin-top: 5px;
  overflow: hidden;
  color: $color-text-tertiary;
  font-size: 10.5px;
  white-space: nowrap;
}

.fc-detail-sub-item {
  font-family: $font-mono;
}

.fc-detail-sub-sep {
  opacity: 0.55;
}

.fc-detail-tag {
  display: flex;
  align-items: center;
  gap: 3px;
  margin-left: 2px;
  padding: 1px 5px;
  border-radius: $radius-xs;
  background: $color-warning-soft;
  color: $color-warning;
  font-family: $font-ui;
  font-size: 10px;
  font-weight: 600;
}

.fc-detail-tag--decrypted {
  background: $color-success-soft;
  color: $color-success;
}

.fc-detail-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.fc-detail-skeleton {
  padding: $spacing-xl;
}

.fc-skeleton-line {
  height: 12px;
  border-radius: 4px;
  background: linear-gradient(90deg, $color-skeleton-base 25%, $color-skeleton-shine 37%, $color-skeleton-base 63%);
  background-size: 400% 100%;
  animation: fc-skeleton 1.4s ease infinite;
}

.fc-skeleton-line--title {
  width: 58%;
  height: 15px;
}

.fc-skeleton-line--sub {
  width: 34%;
  height: 9px;
  margin-top: 8px;
}

.fc-skeleton-tabs {
  display: flex;
  gap: $spacing-lg;
  margin: 22px 0 $spacing-xl;
}

.fc-skeleton-tab {
  width: 64px;
  height: 10px;
  border-radius: 4px;
  background: $color-skeleton-base;
}

.fc-skeleton-card {
  height: 140px;
  border: 1px solid $color-divider;
  border-radius: $radius-md;
  background: $color-surface-muted;
}

.fc-skeleton-card--short {
  height: 88px;
  margin-top: $spacing-lg;
}

.fc-detail-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  padding: $spacing-xxl;
  text-align: center;
}

.fc-empty-art {
  margin-bottom: $spacing-lg;
  opacity: 0.9;
}

.fc-detail-empty-title {
  color: $color-text;
  font-size: 16px;
  font-weight: 600;
  letter-spacing: -0.01em;
}

.fc-detail-empty-text {
  max-width: 430px;
  margin-top: $spacing-sm;
  color: $color-text-tertiary;
  font-size: $font-size-sm;
  line-height: 1.7;
}

.fc-detail-proxy-card {
  width: min(480px, 100%);
  margin-top: $spacing-xl;
  padding: $spacing-lg $spacing-xl;
  border: 1px solid $color-border;
  border-radius: $radius-md;
  background: $color-surface-muted;
  text-align: left;
}

.fc-detail-proxy-row {
  display: flex;
  align-items: center;
  gap: $spacing-md;
}

.fc-detail-proxy-label {
  color: $color-text-tertiary;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.fc-detail-proxy-address {
  flex: 1;
  color: $color-text;
  font-family: $font-mono;
  font-size: 12.5px;
}

.fc-detail-proxy-copy {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 7px;
  border: 1px solid $color-border;
  border-radius: $radius-xs;
  background: $color-surface;
  color: $color-text-secondary;
  font-size: 10.5px;
}

.fc-detail-proxy-copy:hover {
  border-color: $color-border-strong;
  color: $color-text;
}

.fc-detail-proxy-hint {
  margin-top: 7px;
  color: $color-text-tertiary;
  font-size: 11.5px;
  line-height: 1.65;
}

.fc-detail-proxy-hint--active {
  color: $color-success;
}

.fc-detail-curl {
  display: block;
  margin-top: $spacing-md;
  padding: 6px 8px;
  border-radius: $radius-xs;
  background: $color-surface;
  color: $color-text-secondary;
  font-family: $font-mono;
  font-size: 10.5px;
  overflow-wrap: anywhere;
}

@keyframes fc-skeleton {
  from {
    background-position: 100% 0;
  }
  to {
    background-position: 0 0;
  }
}
</style>
