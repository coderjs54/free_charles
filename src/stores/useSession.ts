import { computed, ref } from 'vue'
import type { AppSettings, EntryDetail, EntryMeta, ProxyStatus, SystemProxyState } from '../types/models'
import { api } from '../lib/api'
import { errorMessage } from '../utils/error'
import { useToast } from './useToast'

// 会话状态在模块级共享，所有组件通过 useSession() 访问同一份数据
const entries = ref<EntryMeta[]>([])
const selectedId = ref<string | null>(null)
const detail = ref<EntryDetail | null>(null)
const loadingDetail = ref(false)
const proxyStatus = ref<ProxyStatus>({
  running: false,
  host: '127.0.0.1',
  port: 8888,
  recording: true
})
const settings = ref<AppSettings | null>(null)
const systemProxyState = ref<SystemProxyState>({ supported: false, active: false, server: '' })
const filter = ref('')
// 是否在列表中显示 HTTPS 请求（包括解密后的请求和未解密的 CONNECT 隧道）
const showHttps = ref(true)
const view = ref<'structure' | 'sequence'>('structure')
const initialized = ref(false)

/** 列表事件批量刷新间隔：高频抓包时合并为一次数组重建 */
const ENTRY_FLUSH_INTERVAL_MS = 50

/** 与 entries 保持同步的索引，批量刷新时避免逐条线性查找 */
const entryIndex = new Map<string, EntryMeta>()
const pendingUpserts = new Map<string, EntryMeta>()
const pendingRemovals = new Set<string>()
let entryFlushTimer: number | undefined

let bound = false
let detailToken = 0
let refreshTimer: number | undefined
// 同类运行期错误 5 秒内只提示一次，避免 MITM 失败等情况刷屏
let lastRuntimeError = ''
let lastRuntimeErrorAt = 0

const filteredEntries = computed<EntryMeta[]>(() => {
  const query = filter.value.trim().toLowerCase()
  return entries.value.filter((entry) => {
    if (!showHttps.value && isHttpsRequest(entry)) return false
    return query ? matchesQuery(entry, query) : true
  })
})

const selectedMeta = computed<EntryMeta | null>(() => {
  if (!selectedId.value) return null
  return entries.value.find((entry) => entry.id === selectedId.value) ?? null
})

const proxyAddress = computed(() => `${proxyStatus.value.host}:${proxyStatus.value.port}`)

const stats = computed(() => {
  let https = 0
  let errors = 0
  for (const entry of entries.value) {
    if (isHttpsRequest(entry)) https += 1
    if (entry.state === 'error') errors += 1
  }
  return {
    total: entries.value.length,
    filtered: filteredEntries.value.length,
    https,
    errors
  }
})

/** HTTPS 请求包含解密后的请求和未解密的 CONNECT 隧道 */
function isHttpsRequest(entry: EntryMeta): boolean {
  return entry.protocol === 'https' || entry.tunnel || entry.method === 'CONNECT'
}

function matchesQuery(entry: EntryMeta, query: string): boolean {
  return (
    entry.url.toLowerCase().includes(query) ||
    entry.method.toLowerCase().includes(query) ||
    entry.host.toLowerCase().includes(query) ||
    String(entry.status ?? '').includes(query) ||
    (entry.note ?? '').toLowerCase().includes(query)
  )
}

function sortEntries(list: EntryMeta[]): EntryMeta[] {
  return list.slice().sort((a, b) => a.seq - b.seq)
}

/** 用完整列表替换当前会话，并重建索引 */
function replaceEntries(metas: EntryMeta[]): void {
  const sorted = sortEntries(metas)
  entryIndex.clear()
  for (const meta of sorted) entryIndex.set(meta.id, meta)
  entries.value = sorted
}

/** 把 pending 的增删改合并进索引，只重建一次列表 */
function applyPendingEntries(): void {
  if (pendingUpserts.size === 0 && pendingRemovals.size === 0) return

  for (const id of pendingRemovals) entryIndex.delete(id)
  let hasNewEntry = false
  for (const meta of pendingUpserts.values()) {
    if (!entryIndex.has(meta.id)) hasNewEntry = true
    entryIndex.set(meta.id, meta)
  }
  pendingUpserts.clear()
  pendingRemovals.clear()

  const list = [...entryIndex.values()]
  if (!hasNewEntry) {
    // 纯更新或删除时 Map 的插入顺序仍然是 seq 顺序，无需再排序
    entries.value = list
    return
  }

  const sorted = sortEntries(list)
  entryIndex.clear()
  for (const meta of sorted) entryIndex.set(meta.id, meta)
  entries.value = sorted
}

function scheduleEntryFlush(): void {
  if (entryFlushTimer !== undefined) return
  entryFlushTimer = window.setTimeout(() => {
    entryFlushTimer = undefined
    applyPendingEntries()
  }, ENTRY_FLUSH_INTERVAL_MS)
}

/** 丢弃尚未刷新的增量与定时器 */
function cancelEntryFlush(): void {
  if (entryFlushTimer !== undefined) {
    window.clearTimeout(entryFlushTimer)
    entryFlushTimer = undefined
  }
  pendingUpserts.clear()
  pendingRemovals.clear()
}

async function loadDetail(id: string, silent = false): Promise<void> {
  const token = ++detailToken
  if (!silent) {
    detail.value = null
    loadingDetail.value = true
  }
  try {
    // 详情优先拉取元数据和小 Body，大 Body 由 BodyViewer 按需加载
    const data = await api.getEntry(id, true)
    if (token === detailToken && selectedId.value === id) detail.value = data
  } catch {
    if (token === detailToken) detail.value = null
  } finally {
    if (token === detailToken) loadingDetail.value = false
  }
}

function scheduleDetailRefresh(): void {
  if (refreshTimer) window.clearTimeout(refreshTimer)
  refreshTimer = window.setTimeout(() => {
    const id = selectedId.value
    if (id) void loadDetail(id, true)
  }, 100)
}

/** 控制列表中是否显示 HTTPS 请求；隐藏时如果正在查看 HTTPS 详情则清空选择 */
function setHttpsVisible(visible: boolean): void {
  showHttps.value = visible
  if (visible || !selectedId.value) return
  const current = entries.value.find((entry) => entry.id === selectedId.value)
  if (current && isHttpsRequest(current)) void select(null)
}

function toggleHttpsVisible(): void {
  setHttpsVisible(!showHttps.value)
}

/** 切换选中项并按需从后端拉取详情；token 用于丢弃过期的异步响应 */
async function select(id: string | null): Promise<void> {
  if (selectedId.value === id && detail.value) return
  selectedId.value = id
  detail.value = null
  if (!id) {
    detailToken += 1
    loadingDetail.value = false
    return
  }
  await loadDetail(id)
}

/** 首次挂载时订阅后端事件并同步当前会话、代理状态与设置 */
async function init(): Promise<void> {
  if (bound) return
  bound = true

  api.onEntry((meta) => {
    pendingUpserts.set(meta.id, meta)
    scheduleEntryFlush()
    if (meta.id === selectedId.value) scheduleDetailRefresh()
  })
  api.onEntryUpdate((meta) => {
    pendingUpserts.set(meta.id, meta)
    scheduleEntryFlush()
    if (meta.id === selectedId.value) scheduleDetailRefresh()
  })
  api.onEntryRemove((id) => {
    pendingUpserts.delete(id)
    pendingRemovals.add(id)
    scheduleEntryFlush()
    if (selectedId.value === id) void select(null)
  })
  api.onClear(() => {
    cancelEntryFlush()
    entryIndex.clear()
    entries.value = []
    void select(null)
  })
  api.onLoaded((metas) => {
    cancelEntryFlush()
    replaceEntries(metas)
    void select(null)
  })
  api.onStatus((status) => {
    proxyStatus.value = status
  })
  api.onSettings((next) => {
    settings.value = next
  })
  api.onSystemProxy((state) => {
    systemProxyState.value = state
  })
  api.onError((message) => {
    const now = Date.now()
    if (message === lastRuntimeError && now - lastRuntimeErrorAt < 5000) return
    lastRuntimeError = message
    lastRuntimeErrorAt = now
    useToast().push(message, 'error')
  })

  try {
    const [list, status, config, systemState] = await Promise.all([
      api.listEntries(),
      api.getProxyStatus(),
      api.getSettings(),
      api.getSystemProxy()
    ])
    replaceEntries(list)
    // 合并初始化请求期间已经到达的增量事件，避免覆盖它们
    applyPendingEntries()
    proxyStatus.value = status
    settings.value = config
    systemProxyState.value = systemState
  } catch (error) {
    useToast().push(`初始化失败：${errorMessage(error)}`, 'error')
  } finally {
    initialized.value = true
  }
}

async function toggleRecording(): Promise<void> {
  if (!proxyStatus.value.running) return
  try {
    proxyStatus.value = await api.setRecording(!proxyStatus.value.recording)
  } catch (error) {
    useToast().push(errorMessage(error), 'error')
  }
}

async function toggleProxy(): Promise<void> {
  try {
    proxyStatus.value = proxyStatus.value.running ? await api.stopProxy() : await api.startProxy()
  } catch (error) {
    useToast().push(errorMessage(error), 'error')
  }
}

async function refreshStatus(): Promise<void> {
  try {
    proxyStatus.value = await api.getProxyStatus()
  } catch {
    // ignore
  }
}

async function clearSession(): Promise<void> {
  try {
    await api.clearSession()
  } catch (error) {
    useToast().push(errorMessage(error), 'error')
  }
}

async function removeEntry(id: string): Promise<void> {
  try {
    await api.removeEntry(id)
    if (selectedId.value === id) await select(null)
  } catch (error) {
    useToast().push(errorMessage(error), 'error')
  }
}

async function updateSettings(patch: Partial<AppSettings>): Promise<AppSettings> {
  const next = await api.updateSettings(patch)
  settings.value = next
  await refreshStatus()
  return next
}

export function useSession() {
  return {
    entries,
    filteredEntries,
    selectedId,
    selectedMeta,
    detail,
    loadingDetail,
    proxyStatus,
    proxyAddress,
    settings,
    systemProxyState,
    filter,
    showHttps,
    view,
    initialized,
    stats,
    init,
    select,
    toggleHttpsVisible,
    setHttpsVisible,
    toggleRecording,
    toggleProxy,
    refreshStatus,
    clearSession,
    removeEntry,
    updateSettings
  }
}
