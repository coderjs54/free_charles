<script setup lang="ts">
import { api } from './lib/api'
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { AppCommand, EntryMeta, ThemeMode } from './types/models'
import { useSession } from './stores/useSession'
import { useToast } from './stores/useToast'
import { errorMessage } from './utils/error'
import AppIcon from './components/AppIcon.vue'
import AppToolbar from './components/AppToolbar.vue'
import ComposeDialog from './components/ComposeDialog.vue'
import ConfirmDialog from './components/ConfirmDialog.vue'
import DetailPane from './components/DetailPane.vue'
import EntryList from './components/EntryList.vue'
import SettingsDialog from './components/SettingsDialog.vue'
import ToastStack from './components/ToastStack.vue'

const session = useSession()
const { proxyStatus, proxyAddress, stats, filter, settings, systemProxyState } = session
const { push } = useToast()

const toolbarRef = ref<InstanceType<typeof AppToolbar> | null>(null)
const settingsOpen = ref(false)
const composeOpen = ref(false)
const clearOpen = ref(false)
const systemProxyPromptOpen = ref(false)
const listWidth = ref(700)
const lastProxyError = ref('')
const lastSystemProxyError = ref('')
let unsubscribers: Array<() => void> = []

const recordingLabel = computed(() => {
  if (!proxyStatus.value.running) return '未抓包'
  return proxyStatus.value.recording ? '记录中' : '已暂停'
})

/** 把主题写到根节点，CSS 变量会随 data-theme 切换 */
function applyTheme(theme: ThemeMode): void {
  document.documentElement.dataset.theme = theme
  try {
    // 缓存一份主题，下次启动时可在设置异步加载前先应用，避免闪白
    localStorage.setItem('freecharles:theme', theme)
  } catch {
    // localStorage 不可用时忽略，主题仍会在设置加载后应用
  }
}

try {
  if (localStorage.getItem('freecharles:theme') === 'dark') {
    document.documentElement.dataset.theme = 'dark'
  }
} catch {
  // ignore
}

watch(
  () => settings.value?.theme,
  (theme) => {
    if (theme) applyTheme(theme)
  },
  { immediate: true }
)

/** 首次在 Windows 上运行且代理已启动时，先询问是否允许接管系统代理 */
const needsSystemProxyConsent = computed(
  () =>
    systemProxyState.value.supported &&
    Boolean(settings.value?.autoSystemProxy) &&
    settings.value?.systemProxyPrompted === false &&
    proxyStatus.value.running
)

watch(needsSystemProxyConsent, (needs) => {
  if (needs) systemProxyPromptOpen.value = true
})

async function allowSystemProxy(): Promise<void> {
  systemProxyPromptOpen.value = false
  try {
    await session.updateSettings({ autoSystemProxy: true, systemProxyPrompted: true })
    push('已开启系统代理接管', 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

async function declineSystemProxy(): Promise<void> {
  systemProxyPromptOpen.value = false
  try {
    await session.updateSettings({ autoSystemProxy: false, systemProxyPrompted: true })
    push('已跳过系统代理接管，可稍后在代理设置中开启')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

async function handleCommand(command: AppCommand): Promise<void> {
  switch (command) {
    case 'clear-session':
      requestClear()
      break
    case 'open-settings':
      settingsOpen.value = true
      break
    case 'compose':
      composeOpen.value = true
      break
    case 'save-session':
      await saveSession()
      break
    case 'open-session':
      await openSession()
      break
    case 'export-har':
      await exportHar()
      break
    case 'toggle-recording':
      await session.toggleRecording()
      break
    case 'toggle-proxy':
      await session.toggleProxy()
      break
    case 'focus-filter':
      toolbarRef.value?.focusFilter()
      break
    case 'copy-proxy':
      await copyProxyAddress()
      break
    case 'quit-app':
      await quitApp()
      break
    default:
      break
  }
}

/**
 * 应用级快捷键映射。
 * Windows 下菜单的原生加速键会被 TranslateAccelerator 吞掉（目标句柄是 WebView2 子窗口），
 * 所以菜单不再注册 accelerator，快捷键统一在这里处理。
 */
const SHORTCUTS: Record<string, AppCommand> = {
  'ctrl+n': 'compose',
  'ctrl+o': 'open-session',
  'ctrl+s': 'save-session',
  'ctrl+shift+e': 'export-har',
  'ctrl+k': 'clear-session',
  'ctrl+f': 'focus-filter',
  'ctrl+shift+r': 'toggle-recording',
  'ctrl+shift+p': 'toggle-proxy',
  'ctrl+,': 'open-settings',
  'ctrl+q': 'quit-app'
}

function commandFromKeyboard(event: KeyboardEvent): AppCommand | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null
  const combo = `${event.shiftKey ? 'ctrl+shift+' : 'ctrl+'}${event.key.toLowerCase()}`
  return SHORTCUTS[combo] ?? null
}

function onKeydown(event: KeyboardEvent): void {
  // 弹窗打开时不拦截快捷键，避免在对话框里误触发全局命令
  if (settingsOpen.value || composeOpen.value || clearOpen.value) return
  const command = commandFromKeyboard(event)
  if (!command) return
  event.preventDefault()
  void handleCommand(command)
}

async function quitApp(): Promise<void> {
  try {
    await api.quitApp()
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

async function copyProxyAddress(): Promise<void> {
  try {
    await api.copyText(proxyAddress.value)
    push(`已复制代理地址 ${proxyAddress.value}`)
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

async function saveSession(): Promise<void> {
  try {
    const result = await api.saveSession()
    if (result) push(`会话已保存（${result.count} 条请求）`, 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

async function openSession(): Promise<void> {
  try {
    const result = await api.openSession()
    if (result) push(`已打开会话（${result.count} 条请求）`, 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

async function exportHar(): Promise<void> {
  try {
    const result = await api.exportHar()
    if (result) push(`已导出 HAR（${result.count} 条 HTTP 请求）`, 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

function requestClear(): void {
  if (stats.value.total === 0) return
  clearOpen.value = true
}

async function confirmClear(): Promise<void> {
  clearOpen.value = false
  await session.clearSession()
  push('会话已清空', 'success')
}

async function onComposeSent(meta: EntryMeta): Promise<void> {
  await session.select(meta.id)
}

/** 拖动中间分隔条调整列表宽度，并持久化到 localStorage */
function startResize(event: PointerEvent): void {
  event.preventDefault()
  const startX = event.clientX
  const startWidth = listWidth.value
  const maxWidth = Math.max(380, window.innerWidth - 440)

  const onMove = (moveEvent: PointerEvent): void => {
    listWidth.value = Math.min(Math.max(380, startWidth + moveEvent.clientX - startX), maxWidth)
  }
  const onUp = (): void => {
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    document.body.style.cursor = ''
    document.body.style.userSelect = ''
    try {
      localStorage.setItem('freecharles:listWidth', String(listWidth.value))
    } catch {
      // ignore
    }
  }

  document.body.style.cursor = 'col-resize'
  document.body.style.userSelect = 'none'
  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
}

function resetListWidth(): void {
  listWidth.value = Math.min(700, Math.max(380, window.innerWidth - 440))
}

function clampListWidth(): void {
  listWidth.value = Math.min(Math.max(380, listWidth.value), Math.max(380, window.innerWidth - 440))
}

watch(
  () => proxyStatus.value.error,
  (error) => {
    if (error && error !== lastProxyError.value) {
      lastProxyError.value = error
      push(error, 'error')
    }
  }
)

watch(
  () => proxyStatus.value.running,
  (running) => {
    if (running) lastProxyError.value = ''
  }
)

watch(
  () => systemProxyState.value.error,
  (error) => {
    if (error && error !== lastSystemProxyError.value) {
      lastSystemProxyError.value = error
      push(error, 'error')
    }
  }
)

watch(
  () => systemProxyState.value.active,
  (active) => {
    if (active) lastSystemProxyError.value = ''
  }
)

onMounted(async () => {
  try {
    const stored = Number(localStorage.getItem('freecharles:listWidth'))
    if (Number.isFinite(stored) && stored >= 380) listWidth.value = stored
  } catch {
    // ignore
  }
  clampListWidth()
  window.addEventListener('resize', clampListWidth)
  window.addEventListener('keydown', onKeydown)

  unsubscribers = [api.onCommand((command) => void handleCommand(command))]
  await session.init()
})

onBeforeUnmount(() => {
  unsubscribers.forEach((unsubscribe) => unsubscribe())
  window.removeEventListener('resize', clampListWidth)
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <div class="fc-app">
    <AppToolbar
      ref="toolbarRef"
      @clear-session="requestClear"
      @compose="composeOpen = true"
      @open-settings="settingsOpen = true"
    />

    <div v-if="!proxyStatus.running" class="fc-proxy-alert" :class="{ 'fc-proxy-alert--error': proxyStatus.error }">
      <span class="fc-proxy-alert-icon"><AppIcon name="alert" :size="14" /></span>
      <span class="fc-proxy-alert-text">
        {{ proxyStatus.error || '代理未启动，浏览器请求不会被捕获' }}
      </span>
      <button class="fc-proxy-alert-button" type="button" @click="session.toggleProxy()">重试</button>
      <button class="fc-proxy-alert-button fc-proxy-alert-button--ghost" type="button" @click="settingsOpen = true">
        代理设置
      </button>
    </div>

    <main class="fc-workspace">
      <div class="fc-list-pane" :style="{ width: `${listWidth}px` }">
        <EntryList />
      </div>
      <div class="fc-splitter" title="拖动调整宽度，双击重置" @pointerdown="startResize" @dblclick="resetListWidth" />
      <DetailPane class="fc-detail-pane" />
    </main>

    <footer class="fc-statusbar">
      <span class="fc-status-item fc-status-proxy" :class="{ 'fc-status-proxy--stopped': !proxyStatus.running }">
        <i class="fc-status-dot" />
        {{ proxyStatus.running ? `代理 ${proxyAddress}` : '代理未启动' }}
      </span>
      <span class="fc-status-divider" />
      <span class="fc-status-item">{{ recordingLabel }}</span>
      <span class="fc-status-divider" />
      <span class="fc-status-item">{{ stats.total }} 个请求</span>
      <span v-if="filter.trim()" class="fc-status-item fc-status-item--accent">匹配 {{ stats.filtered }}</span>
      <span v-if="stats.errors > 0" class="fc-status-item fc-status-item--danger">{{ stats.errors }} 个失败</span>
      <span v-if="proxyStatus.error" class="fc-status-item fc-status-item--danger fc-status-error" :title="proxyStatus.error">
        {{ proxyStatus.error }}
      </span>
      <span v-if="systemProxyState.active" class="fc-status-item fc-status-item--accent">系统代理已接管</span>
      <span
        v-else-if="systemProxyState.error"
        class="fc-status-item fc-status-item--danger"
        :title="systemProxyState.error"
      >
        系统代理未接管
      </span>
      <span class="fc-statusbar-spacer" />
      <span class="fc-status-item fc-status-item--muted">HTTP 抓包 · HTTPS 可解密</span>
    </footer>

    <SettingsDialog v-if="settingsOpen" @close="settingsOpen = false" />
    <ComposeDialog v-if="composeOpen" @close="composeOpen = false" @sent="onComposeSent" />
    <ConfirmDialog
      v-if="clearOpen"
      title="清空当前会话"
      :message="`将删除当前捕获的 ${stats.total} 条请求，此操作无法撤销。建议先保存会话。`"
      confirm-label="清空会话"
      danger
      @confirm="confirmClear"
      @close="clearOpen = false"
    />
    <ConfirmDialog
      v-if="systemProxyPromptOpen"
      title="允许接管 Windows 系统代理？"
      message="开启后 FreeCharles 会自动把系统代理指向本机，浏览器和其他遵循系统代理的程序无需手动配置；停止代理或退出应用时会恢复原设置。你也可以稍后在「代理设置」中修改。"
      confirm-label="允许接管"
      cancel-label="暂不接管"
      @confirm="allowSystemProxy"
      @close="declineSystemProxy"
    />
    <ToastStack />
  </div>
</template>

<style scoped lang="scss">
.fc-app {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: $color-bg;
}

.fc-proxy-alert {
  display: flex;
  align-items: center;
  gap: $spacing-md;
  min-height: 34px;
  padding: 5px $spacing-lg;
  border-bottom: 1px solid $color-warning-border;
  background: $color-warning-soft;
  color: $color-warning;
  font-size: 11.5px;
  line-height: 1.5;
  flex: none;
}

.fc-proxy-alert--error {
  border-bottom-color: $color-danger-border;
  background: $color-danger-soft;
  color: $color-danger;
}

.fc-proxy-alert-icon {
  display: flex;
  flex: none;
}

.fc-proxy-alert-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
}

.fc-proxy-alert-button {
  flex: none;
  height: 22px;
  padding: 0 8px;
  border: 1px solid currentColor;
  border-radius: $radius-xs;
  color: currentColor;
  font-size: 11px;
  opacity: 0.85;
}

.fc-proxy-alert-button:hover {
  opacity: 1;
  background: $color-overlay-soft;
}

.fc-proxy-alert-button--ghost {
  border-color: transparent;
  opacity: 0.7;
}

.fc-proxy-alert-button--ghost:hover {
  opacity: 1;
  background: $color-overlay-softer;
}

.fc-workspace {
  display: flex;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  background: $color-surface;
}

.fc-list-pane {
  flex: none;
  min-width: 380px;
  max-width: calc(100% - 440px);
  height: 100%;
  overflow: hidden;
}

.fc-splitter {
  position: relative;
  z-index: 2;
  width: 5px;
  flex: none;
  cursor: col-resize;
  background: $color-bg;
}

.fc-splitter::after {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  left: 2px;
  width: 1px;
  background: $color-border;
  transition: background $transition-fast, width $transition-fast;
}

.fc-splitter:hover::after {
  left: 1px;
  width: 3px;
  background: rgba(37, 99, 235, 0.4);
}

.fc-detail-pane {
  flex: 1;
  min-width: 440px;
  height: 100%;
  overflow: hidden;
}

.fc-statusbar {
  display: flex;
  align-items: center;
  gap: $spacing-md;
  height: $statusbar-height;
  padding: 0 $spacing-lg;
  border-top: 1px solid $color-border;
  background: $color-surface-muted;
  color: $color-text-tertiary;
  font-size: 10.5px;
  flex: none;
  overflow: hidden;
  white-space: nowrap;
}

.fc-status-item {
  display: flex;
  align-items: center;
  gap: 5px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.fc-status-proxy {
  color: $color-text-secondary;
  font-family: $font-mono;
}

.fc-status-proxy--stopped {
  color: $color-text-tertiary;
}

.fc-status-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: $color-online;
  box-shadow: 0 0 0 2px rgba(26, 157, 99, 0.13);
}

.fc-status-proxy--stopped .fc-status-dot {
  background: $color-dot-idle;
  box-shadow: none;
}

.fc-status-divider {
  width: 1px;
  height: 11px;
  background: $color-border;
  flex: none;
}

.fc-status-item--accent {
  color: $color-primary;
}

.fc-status-item--danger {
  color: $color-danger;
}

.fc-status-item--muted {
  color: $color-text-tertiary;
}

.fc-status-error {
  max-width: 380px;
}

.fc-statusbar-spacer {
  flex: 1;
}
</style>
