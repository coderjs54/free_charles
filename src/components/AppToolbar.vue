<script setup lang="ts">
import { api } from '../lib/api'
import { computed, ref } from 'vue'
import { useSession } from '../stores/useSession'
import { useToast } from '../stores/useToast'
import AppIcon from './AppIcon.vue'

const emit = defineEmits<{
  (event: 'clear-session'): void
  (event: 'compose'): void
  (event: 'open-settings'): void
}>()

const {
  view,
  filter,
  showHttps,
  stats,
  proxyStatus,
  proxyAddress,
  toggleRecording,
  toggleProxy,
  toggleHttpsVisible
} = useSession()
const { push } = useToast()
const filterInput = ref<HTMLInputElement | null>(null)

const proxyStopped = computed(() => !proxyStatus.value.running)
const recording = computed(() => proxyStatus.value.running && proxyStatus.value.recording)

async function onToggleProxy(): Promise<void> {
  await toggleProxy()
  if (proxyStatus.value.error) push(proxyStatus.value.error, 'error')
  else if (proxyStatus.value.running) push(`代理已启动：${proxyAddress.value}`, 'success')
}

async function copyProxyAddress(): Promise<void> {
  try {
    await api.copyText(proxyAddress.value)
    push(`已复制代理地址 ${proxyAddress.value}`)
  } catch {
    push('复制失败', 'error')
  }
}

function focusFilter(): void {
  filterInput.value?.focus()
  filterInput.value?.select()
}

defineExpose({ focusFilter })
</script>

<template>
  <header class="fc-toolbar">
    <div class="fc-brand">
      <div class="fc-brand-mark">
        <AppIcon name="globe" :size="15" />
      </div>
      <div class="fc-brand-text">
        <span class="fc-brand-name">FreeCharles</span>
        <span class="fc-brand-sub">HTTP Proxy Inspector</span>
      </div>
    </div>

    <div class="fc-toolbar-divider" />

    <button
      class="fc-record-button"
      :class="{ 'fc-record-button--active': recording }"
      type="button"
      :disabled="proxyStopped"
      :title="proxyStopped ? '请先启动代理' : recording ? '暂停记录（Ctrl+Shift+R）' : '继续记录（Ctrl+Shift+R）'"
      @click="toggleRecording"
    >
      <span class="fc-record-dot" />
      <span class="fc-record-label">{{ recording ? 'Recording' : 'Paused' }}</span>
    </button>

    <button class="fc-icon-button" type="button" title="清空会话（Ctrl+K）" @click="emit('clear-session')">
      <AppIcon name="trash" :size="15" />
    </button>

    <div class="fc-toolbar-divider" />

    <div class="fc-view-switch" role="tablist">
      <button
        class="fc-view-button"
        :class="{ 'fc-view-button--active': view === 'structure' }"
        type="button"
        data-testid="view-structure"
        @click="view = 'structure'"
      >
        <AppIcon name="tree" :size="13" />
        <span>Structure</span>
      </button>
      <button
        class="fc-view-button"
        :class="{ 'fc-view-button--active': view === 'sequence' }"
        type="button"
        data-testid="view-sequence"
        @click="view = 'sequence'"
      >
        <AppIcon name="list" :size="13" />
        <span>Sequence</span>
      </button>
    </div>

    <button
      class="fc-https-filter"
      :class="{ 'fc-https-filter--active': showHttps }"
      type="button"
      data-testid="https-filter"
      :aria-pressed="showHttps ? 'true' : 'false'"
      :title="showHttps ? '隐藏 HTTPS 请求' : '显示 HTTPS 请求'"
      @click="toggleHttpsVisible"
    >
      <AppIcon name="lock" :size="12" />
      <span class="fc-https-filter-label">HTTPS</span>
      <span class="fc-https-filter-count" :title="`当前会话共 ${stats.https} 条 HTTPS 请求`">
        {{ stats.https }}
      </span>
      <span class="fc-https-filter-switch">
        <span class="fc-https-filter-thumb" />
      </span>
    </button>

    <div class="fc-filter">
      <span class="fc-filter-icon">
        <AppIcon name="search" :size="13" />
      </span>
      <input
        ref="filterInput"
        v-model="filter"
        class="fc-filter-input"
        type="text"
        placeholder="Filter requests"
        title="按 URL、方法、状态过滤请求（Ctrl+F）"
        spellcheck="false"
        data-testid="filter-input"
      />
      <button v-if="filter" class="fc-filter-clear" type="button" title="清除过滤" @click="filter = ''">
        <AppIcon name="x" :size="11" />
      </button>
      <span class="fc-filter-hint">Ctrl F</span>
    </div>

    <div class="fc-toolbar-spacer" />

    <button class="fc-icon-button" type="button" title="新建请求（Ctrl+N）" @click="emit('compose')">
      <AppIcon name="plus" :size="16" />
    </button>
    <button class="fc-icon-button" type="button" title="设置（Ctrl+,）" @click="emit('open-settings')">
      <AppIcon name="settings" :size="15" />
    </button>

    <button
      class="fc-proxy-chip"
      :class="{ 'fc-proxy-chip--stopped': proxyStopped, 'fc-proxy-chip--error': Boolean(proxyStatus.error) }"
      type="button"
      :title="proxyStatus.error || `点击复制代理地址 ${proxyAddress}`"
      @click="copyProxyAddress"
    >
      <span class="fc-proxy-dot" />
      <span class="fc-proxy-address">{{ proxyStopped ? '代理未启动' : proxyAddress }}</span>
    </button>

    <button
      class="fc-icon-button fc-power-button"
      :class="{ 'fc-icon-button--active': !proxyStopped }"
      type="button"
      :title="proxyStopped ? '启动代理' : '停止代理'"
      @click="onToggleProxy"
    >
      <AppIcon name="power" :size="15" />
    </button>
  </header>
</template>

<style scoped lang="scss">
.fc-toolbar {
  display: flex;
  align-items: center;
  gap: 6px;
  height: $toolbar-height;
  padding: 0 $spacing-lg;
  border-bottom: 1px solid $color-border;
  background: $color-surface;
  box-shadow: 0 1px 0 rgba(16, 24, 40, 0.02);
  flex: none;
}

.fc-brand {
  display: flex;
  align-items: center;
  gap: $spacing-md;
  padding-right: 2px;
}

.fc-brand-mark {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 8px;
  background: linear-gradient(145deg, #3b82f6 0%, #1d4ed8 60%, #1e3a8a 100%);
  box-shadow: 0 3px 8px rgba(37, 99, 235, 0.28), inset 0 1px 0 rgba(255, 255, 255, 0.35);
  color: #fff;
}

.fc-brand-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  line-height: 1;
}

.fc-brand-name {
  font-size: 14px;
  font-weight: 650;
  letter-spacing: -0.015em;
  color: $color-text;
}

.fc-brand-sub {
  color: $color-text-tertiary;
  font-size: 9.5px;
  font-weight: 600;
  letter-spacing: 0.09em;
  text-transform: uppercase;
}

.fc-toolbar-divider {
  width: 1px;
  height: 22px;
  margin: 0 3px;
  background: $color-divider;
}

.fc-record-button {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text-secondary;
  font-size: 12.5px;
  font-weight: 500;
  transition: border-color $transition-fast, background $transition-fast, color $transition-fast;
}

.fc-record-button:hover {
  border-color: $color-border-strong;
  background: $color-surface-muted;
}

.fc-record-button:disabled {
  opacity: 0.55;
  cursor: not-allowed;
  background: $color-surface;
  border-color: $color-border;
}

.fc-record-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #c3cad4;
  transition: background $transition-fast;
}

.fc-record-button--active {
  color: $color-danger;
  border-color: rgba(192, 43, 43, 0.22);
  background: $color-danger-soft;
}

.fc-record-button--active:hover {
  border-color: rgba(192, 43, 43, 0.35);
  background: #fce3e3;
}

.fc-record-button--active .fc-record-dot {
  background: #e5484d;
  box-shadow: 0 0 0 3px rgba(229, 72, 77, 0.16);
  animation: fc-record-pulse 1.8s ease-in-out infinite;
}

.fc-icon-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: $radius-sm;
  color: $color-text-secondary;
  transition: background $transition-fast, color $transition-fast;
}

.fc-icon-button:hover {
  background: $color-hover;
  color: $color-text;
}

.fc-icon-button--active {
  color: $color-primary;
  background: $color-primary-soft;
}

.fc-icon-button--active:hover {
  background: $color-selected;
  color: $color-primary;
}

.fc-view-switch {
  display: flex;
  gap: 2px;
  padding: 2px;
  border-radius: 7px;
  background: $color-surface-sunken;
}

.fc-view-button {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 24px;
  padding: 0 9px;
  border-radius: 5px;
  color: $color-text-tertiary;
  font-size: 12.5px;
  font-weight: 500;
  transition: background $transition-fast, color $transition-fast, box-shadow $transition-fast;
}

.fc-view-button:hover {
  color: $color-text-secondary;
}

.fc-view-button--active {
  background: $color-surface;
  color: $color-text;
  box-shadow: $shadow-sm;
}

.fc-https-filter {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 28px;
  padding: 0 7px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text-tertiary;
  font-size: 11.5px;
  font-weight: 500;
  transition: border-color $transition-fast, background $transition-fast, color $transition-fast;
}

.fc-https-filter:hover {
  border-color: $color-border-strong;
  background: $color-surface-muted;
  color: $color-text-secondary;
}

.fc-https-filter--active {
  border-color: rgba(37, 99, 235, 0.24);
  background: $color-primary-soft;
  color: $color-primary;
}

.fc-https-filter--active:hover {
  border-color: rgba(37, 99, 235, 0.36);
  background: $color-selected;
  color: $color-primary;
}

.fc-https-filter:focus-visible {
  border-color: rgba(37, 99, 235, 0.55);
  box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.1);
}

.fc-https-filter-label {
  letter-spacing: 0.02em;
  white-space: nowrap;
}

.fc-https-filter-count {
  min-width: 15px;
  padding: 0 4px;
  border-radius: $radius-pill;
  background: $color-surface-sunken;
  color: $color-text-tertiary;
  font-family: $font-mono;
  font-size: 10px;
  line-height: 14px;
  text-align: center;
}

.fc-https-filter--active .fc-https-filter-count {
  background: rgba(37, 99, 235, 0.12);
  color: $color-primary;
}

.fc-https-filter-switch {
  position: relative;
  width: 22px;
  height: 12px;
  border-radius: $radius-pill;
  background: #d5dbe4;
  transition: background $transition-fast;
}

.fc-https-filter-thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 2px rgba(16, 24, 40, 0.2);
  transition: transform $transition-fast;
}

.fc-https-filter--active .fc-https-filter-switch {
  background: $color-primary;
}

.fc-https-filter--active .fc-https-filter-thumb {
  transform: translateX(10px);
}

.fc-filter {
  position: relative;
  display: flex;
  align-items: center;
  width: 292px;
  height: 28px;
  margin-left: 4px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface-muted;
  transition: border-color $transition-fast, background $transition-fast, box-shadow $transition-fast;
}

.fc-filter:focus-within {
  border-color: rgba(37, 99, 235, 0.55);
  background: $color-surface;
  box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.1);
}

.fc-filter-icon {
  position: absolute;
  left: 8px;
  display: flex;
  color: $color-text-tertiary;
  pointer-events: none;
}

.fc-filter-input {
  width: 100%;
  height: 100%;
  padding: 0 66px 0 27px;
  color: $color-text;
  font-size: 12.5px;
}

.fc-filter-input::placeholder {
  color: $color-text-tertiary;
}

.fc-filter-clear {
  position: absolute;
  right: 46px;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: $color-border;
  color: $color-text-secondary;
}

.fc-filter-clear:hover {
  background: $color-border-strong;
}

.fc-filter-hint {
  position: absolute;
  right: 8px;
  color: $color-text-tertiary;
  font-family: $font-mono;
  font-size: 10px;
  pointer-events: none;
}

.fc-toolbar-spacer {
  flex: 1;
}

.fc-proxy-chip {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 26px;
  padding: 0 9px;
  border: 1px solid $color-border;
  border-radius: $radius-pill;
  background: $color-surface-muted;
  color: $color-text-secondary;
  transition: border-color $transition-fast, background $transition-fast;
}

.fc-proxy-chip:hover {
  border-color: $color-border-strong;
  background: $color-surface;
}

.fc-proxy-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #1a9d63;
  box-shadow: 0 0 0 3px rgba(26, 157, 99, 0.13);
}

.fc-proxy-address {
  font-family: $font-mono;
  font-size: 11.5px;
  letter-spacing: -0.01em;
}

.fc-proxy-chip--stopped {
  color: $color-text-tertiary;
}

.fc-proxy-chip--stopped .fc-proxy-dot {
  background: #b8c0cc;
  box-shadow: none;
}

.fc-proxy-chip--error .fc-proxy-dot {
  background: $color-danger;
  box-shadow: 0 0 0 3px rgba(192, 43, 43, 0.13);
}

.fc-power-button {
  margin-left: -2px;
}

@media (max-width: 1280px) {
  .fc-brand-sub {
    display: none;
  }

  .fc-filter {
    width: 220px;
  }
}

@media (max-width: 1200px) {
  .fc-https-filter-label {
    display: none;
  }
}

@media (max-width: 1120px) {
  .fc-filter-hint {
    display: none;
  }

  .fc-filter-input {
    padding-right: 32px;
  }

  .fc-filter-clear {
    right: 8px;
  }
}

@keyframes fc-record-pulse {
  0%,
  100% {
    box-shadow: 0 0 0 3px rgba(229, 72, 77, 0.12);
  }
  50% {
    box-shadow: 0 0 0 5px rgba(229, 72, 77, 0.05);
  }
}
</style>
