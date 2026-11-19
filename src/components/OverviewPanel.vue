<script setup lang="ts">
import { api } from '../lib/api'
import { computed } from 'vue'
import type { EntryDetail } from '../types/models'
import { useSession } from '../stores/useSession'
import { useToast } from '../stores/useToast'
import { buildCurl } from '../utils/curl'
import { errorMessage } from '../utils/error'
import {
  formatBytes,
  formatDateTime,
  formatDuration,
  hostLabel,
  statusFullText,
  statusTone
} from '../utils/format'
import AppIcon from './AppIcon.vue'

const props = defineProps<{ detail: EntryDetail }>()

const session = useSession()
const { push } = useToast()

const tone = computed(() => statusTone(props.detail))
const canRepeat = computed(
  () =>
    props.detail.state !== 'pending' &&
    props.detail.method !== 'CONNECT' &&
    !props.detail.tunnel &&
    !props.detail.upgrade
)
const waitMs = computed(() => props.detail.timings.waitMs ?? 0)
const receiveMs = computed(() => props.detail.timings.receiveMs ?? 0)
const timingTotal = computed(() => Math.max(1, waitMs.value + receiveMs.value))
const waitWidth = computed(() => `${Math.min(100, Math.max(waitMs.value > 0 ? 3 : 0, (waitMs.value / timingTotal.value) * 100))}%`)
const receiveWidth = computed(() => `${Math.max(receiveMs.value > 0 ? 2 : 0, (receiveMs.value / timingTotal.value) * 100)}%`)
const hasTiming = computed(() => waitMs.value > 0 || receiveMs.value > 0)

async function copy(text: string, message: string): Promise<void> {
  try {
    await api.copyText(text)
    push(message, 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

async function repeat(): Promise<void> {
  if (!canRepeat.value) return
  try {
    const meta = await api.repeatEntry(props.detail.id)
    await session.select(meta.id)
    push('已重放请求，新条目已添加到会话', 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

function copyUrl(): void {
  void copy(props.detail.url, '已复制 URL')
}

function copyCurl(): void {
  void copy(buildCurl(props.detail), '已复制 cURL 命令')
}
</script>

<template>
  <div class="fc-overview">
    <div class="fc-overview-top">
      <div class="fc-overview-cards">
        <div class="fc-overview-item fc-overview-item--wide">
          <span class="fc-overview-label">Request URL</span>
          <span class="fc-overview-value fc-overview-value--mono">{{ detail.url }}</span>
        </div>
        <div class="fc-overview-item">
          <span class="fc-overview-label">Method</span>
          <span class="fc-overview-value">{{ detail.method }}</span>
        </div>
        <div class="fc-overview-item">
          <span class="fc-overview-label">Status</span>
          <span class="fc-overview-status" :class="`fc-overview-status--${tone}`">
            {{ statusFullText(detail) }}
          </span>
        </div>
        <div class="fc-overview-item">
          <span class="fc-overview-label">Protocol</span>
          <span class="fc-overview-value fc-overview-value--mono">{{ detail.httpVersion }}</span>
        </div>
        <div class="fc-overview-item">
          <span class="fc-overview-label">Duration</span>
          <span class="fc-overview-value">{{ formatDuration(detail.duration) }}</span>
        </div>
        <div class="fc-overview-item">
          <span class="fc-overview-label">Client</span>
          <span class="fc-overview-value fc-overview-value--mono">{{ detail.clientIp }}</span>
        </div>
        <div class="fc-overview-item">
          <span class="fc-overview-label">Remote host</span>
          <span class="fc-overview-value fc-overview-value--mono">{{ hostLabel(detail) }}</span>
        </div>
        <div class="fc-overview-item">
          <span class="fc-overview-label">Started at</span>
          <span class="fc-overview-value">{{ formatDateTime(detail.startTime) }}</span>
        </div>
        <div class="fc-overview-item">
          <span class="fc-overview-label">Content-Type</span>
          <span class="fc-overview-value fc-overview-value--mono">{{ detail.responseMime || '—' }}</span>
        </div>
        <div class="fc-overview-item">
          <span class="fc-overview-label">Request size</span>
          <span class="fc-overview-value">{{ formatBytes(detail.requestSize) }}</span>
        </div>
        <div class="fc-overview-item">
          <span class="fc-overview-label">Response size</span>
          <span class="fc-overview-value">{{ formatBytes(detail.responseSize) }}</span>
        </div>
      </div>

      <div class="fc-overview-actions">
        <button v-if="canRepeat" class="fc-overview-action" type="button" @click="repeat">
          <AppIcon name="repeat" :size="13" />
          <span>重放</span>
        </button>
        <button class="fc-overview-action" type="button" @click="copyUrl">
          <AppIcon name="copy" :size="13" />
          <span>复制 URL</span>
        </button>
        <button v-if="canRepeat" class="fc-overview-action" type="button" @click="copyCurl">
          <AppIcon name="code" :size="13" />
          <span>复制 cURL</span>
        </button>
      </div>
    </div>

    <div v-if="detail.tunnel" class="fc-overview-banner fc-overview-banner--info">
      <AppIcon name="lock" :size="15" />
      <div class="fc-overview-banner-text">
        <strong>HTTPS 隧道</strong>
        <span>{{ detail.note || '该连接仅做隧道透传，内容未解密，因此无法查看具体的 HTTPS 请求。' }}</span>
      </div>
    </div>

    <div v-if="detail.state === 'error'" class="fc-overview-banner fc-overview-banner--error">
      <AppIcon name="alert" :size="15" />
      <div class="fc-overview-banner-text">
        <strong>请求失败</strong>
        <span>{{ detail.error || '目标服务器没有返回有效响应。' }}</span>
      </div>
    </div>

    <div v-else-if="detail.note && !detail.tunnel" class="fc-overview-banner fc-overview-banner--info">
      <AppIcon name="info" :size="15" />
      <div class="fc-overview-banner-text">
        <span>{{ detail.note }}</span>
      </div>
    </div>

    <section class="fc-timing">
      <div class="fc-timing-head">
        <h3 class="fc-timing-title">Timing</h3>
        <span class="fc-timing-total">{{ formatDuration(detail.duration) }}</span>
      </div>
      <div class="fc-timing-bar" :class="{ 'fc-timing-bar--empty': !hasTiming }">
        <div class="fc-timing-segment fc-timing-segment--wait" :style="{ width: hasTiming ? waitWidth : '100%' }" />
        <div class="fc-timing-segment fc-timing-segment--receive" :style="{ width: hasTiming ? receiveWidth : '0%' }" />
      </div>
      <div class="fc-timing-legend">
        <span class="fc-timing-legend-item">
          <i class="fc-timing-dot fc-timing-dot--wait" />
          等待响应 <strong>{{ formatDuration(detail.timings.waitMs) }}</strong>
        </span>
        <span class="fc-timing-legend-item">
          <i class="fc-timing-dot fc-timing-dot--receive" />
          接收内容 <strong>{{ formatDuration(detail.timings.receiveMs) }}</strong>
        </span>
        <span class="fc-timing-legend-item fc-timing-legend-item--size">
          请求头 {{ formatBytes(detail.requestHeaderSize) }} · 响应头 {{ formatBytes(detail.responseHeaderSize) }}
        </span>
      </div>
      <p v-if="!hasTiming" class="fc-timing-pending">
        {{ detail.state === 'pending' ? '正在等待服务器响应…' : '没有可用的耗时数据' }}
      </p>
    </section>
  </div>
</template>

<style scoped lang="scss">
.fc-overview {
  padding: $spacing-xl;
}

.fc-overview-top {
  display: flex;
  align-items: flex-start;
  gap: $spacing-lg;
}

.fc-overview-cards {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: $spacing-md;
  flex: 1;
  min-width: 0;
}

.fc-overview-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  padding: 9px $spacing-lg;
  border: 1px solid $color-divider;
  border-radius: $radius-md;
  background: $color-surface-muted;
}

.fc-overview-item--wide {
  grid-column: 1 / -1;
}

.fc-overview-label {
  color: $color-text-tertiary;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.fc-overview-value {
  overflow: hidden;
  color: $color-text;
  font-size: 13px;
  font-weight: 500;
  line-height: 1.5;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-overview-item--wide .fc-overview-value {
  white-space: normal;
  word-break: break-all;
}

.fc-overview-value--mono {
  font-family: $font-mono;
  font-size: 12px;
}

.fc-overview-status {
  color: $color-text-secondary;
  font-family: $font-mono;
  font-size: 12.5px;
  font-weight: 600;
}

.fc-overview-status--success {
  color: $color-success;
}

.fc-overview-status--info {
  color: $color-info;
}

.fc-overview-status--warning {
  color: $color-warning;
}

.fc-overview-status--danger {
  color: $color-danger;
}

.fc-overview-status--muted {
  color: $color-text-tertiary;
}

.fc-overview-actions {
  display: flex;
  flex-direction: column;
  gap: $spacing-sm;
  flex: none;
}

.fc-overview-action {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 26px;
  padding: 0 9px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text-secondary;
  font-size: 11.5px;
  white-space: nowrap;
}

.fc-overview-action:hover {
  border-color: $color-border-strong;
  background: $color-surface-muted;
  color: $color-text;
}

.fc-overview-banner {
  display: flex;
  align-items: flex-start;
  gap: $spacing-md;
  margin-top: $spacing-lg;
  padding: 10px $spacing-lg;
  border: 1px solid rgba(37, 99, 235, 0.16);
  border-radius: $radius-md;
  background: $color-primary-soft;
  color: $color-info;
}

.fc-overview-banner--info {
  color: $color-info;
}

.fc-overview-banner--error {
  border-color: $color-danger-border;
  background: $color-danger-soft;
  color: $color-danger;
}

.fc-overview-banner-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  color: $color-text-secondary;
  font-size: 12px;
  line-height: 1.6;
}

.fc-overview-banner-text strong {
  color: $color-text;
  font-size: 12.5px;
}

.fc-timing {
  margin-top: $spacing-xl;
  padding: $spacing-lg $spacing-xl $spacing-xl;
  border: 1px solid $color-divider;
  border-radius: $radius-md;
  background: $color-surface;
}

.fc-timing-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: $spacing-lg;
}

.fc-timing-title {
  color: $color-text-secondary;
  font-size: 12px;
  font-weight: 650;
  letter-spacing: 0.05em;
  text-transform: uppercase;
}

.fc-timing-total {
  color: $color-text-tertiary;
  font-family: $font-mono;
  font-size: 11px;
}

.fc-timing-bar {
  display: flex;
  height: 8px;
  overflow: hidden;
  border-radius: $radius-pill;
  background: $color-surface-sunken;
}

.fc-timing-bar--empty {
  background: linear-gradient(
    90deg,
    $color-skeleton-base 25%,
    $color-skeleton-shine 25%,
    $color-skeleton-shine 50%,
    $color-skeleton-base 50%,
    $color-skeleton-base 75%,
    $color-skeleton-shine 75%
  );
  background-size: 24px 100%;
  animation: fc-timing-stripes 1.4s linear infinite;
}

.fc-timing-segment {
  height: 100%;
}

.fc-timing-segment--wait {
  background: $color-timeline-soft;
}

.fc-timing-segment--receive {
  background: $color-timeline;
}

.fc-timing-legend {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: $spacing-lg;
  margin-top: $spacing-md;
}

.fc-timing-legend-item {
  display: flex;
  align-items: center;
  gap: 5px;
  color: $color-text-tertiary;
  font-size: 11px;
}

.fc-timing-legend-item strong {
  color: $color-text-secondary;
  font-family: $font-mono;
  font-weight: 500;
}

.fc-timing-legend-item--size {
  margin-left: auto;
  font-family: $font-mono;
  font-size: 10.5px;
}

.fc-timing-dot {
  width: 6px;
  height: 6px;
  border-radius: 2px;
}

.fc-timing-dot--wait {
  background: $color-timeline-soft;
}

.fc-timing-dot--receive {
  background: $color-timeline;
}

.fc-timing-pending {
  margin-top: $spacing-md;
  color: $color-text-tertiary;
  font-size: 11px;
}

@keyframes fc-timing-stripes {
  from {
    background-position: 0 0;
  }
  to {
    background-position: 24px 0;
  }
}
</style>
