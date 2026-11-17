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

let bound = false
let detailToken = 0
let refreshTimer: number | undefined

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

const stats = computed(() => ({
  total: entries.value.length,
  filtered: filteredEntries.value.length,
  https: entries.value.filter((entry) => isHttpsRequest(entry)).length,
  errors: entries.value.filter((entry) => entry.state === 'error').length
}))

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

function upsert(meta: EntryMeta): void {
  const index = entries.value.findIndex((entry) => entry.id === meta.id)
  if (index === -1) {
    entries.value = sortEntries([...entries.value, meta])
    return
  }
  const next = entries.value.slice()
  next[index] = meta
  entries.value = next
}

async function loadDetail(id: string, silent = false): Promise<void> {
  const token = ++detailToken
  if (!silent) {
    detail.value = null
    loadingDetail.value = true
  }
  try {
    const data = await api.getEntry(id)
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
    upsert(meta)
    if (meta.id === selectedId.value) scheduleDetailRefresh()
  })
  api.onEntryUpdate((meta) => {
    upsert(meta)
    if (meta.id === selectedId.value) scheduleDetailRefresh()
  })
  api.onEntryRemove((id) => {
    entries.value = entries.value.filter((entry) => entry.id !== id)
    if (selectedId.value === id) void select(null)
  })
  api.onClear(() => {
    entries.value = []
    void select(null)
  })
  api.onLoaded((metas) => {
    entries.value = sortEntries(metas)
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

  try {
    const [list, status, config, systemState] = await Promise.all([
      api.listEntries(),
      api.getProxyStatus(),
      api.getSettings(),
      api.getSystemProxy()
    ])
    entries.value = sortEntries(list)
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
