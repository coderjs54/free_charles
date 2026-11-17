import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { documentDir, downloadDir, join } from '@tauri-apps/api/path'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { open as openFileDialog, save as saveFileDialog } from '@tauri-apps/plugin-dialog'
import type {
  AppCommand,
  AppSettings,
  ComposePayload,
  EntryDetail,
  EntryMeta,
  HttpsCaInfo,
  ProxyStatus,
  SystemProxyState
} from '../types/models'

export interface SaveResult {
  path: string
  count: number
}

export interface SaveBodyResult {
  path: string
  size: number
}

const SESSION_FILTERS = [
  { name: 'FreeCharles 会话', extensions: ['fchls', 'json'] },
  { name: '所有文件', extensions: ['*'] }
]

/** 注册事件监听并返回取消订阅函数，避免重复绑定 */
function subscribe<T>(event: string, callback: (payload: T) => void): () => void {
  let unlisten: UnlistenFn | null = null
  let disposed = false

  void listen<T>(event, (message) => callback(message.payload)).then((fn) => {
    if (disposed) fn()
    else unlisten = fn
  })

  return () => {
    disposed = true
    unlisten?.()
    unlisten = null
  }
}

/** 文件名用的时间戳，例如 2026-11-16T08-20-31 */
function timestamp(): string {
  return new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
}

async function defaultPathIn(folder: 'documents' | 'downloads', file: string): Promise<string> {
  try {
    const base = folder === 'documents' ? await documentDir() : await downloadDir()
    return await join(base, file)
  } catch {
    return file
  }
}

/**
 * 前端访问后端的统一入口：
 * 数据读写走 invoke，实时推送走事件订阅，系统对话框与剪贴板走官方插件。
 */
export const api = {
  startProxy: (port?: number) => invoke<ProxyStatus>('start_proxy', { port: port ?? null }),
  stopProxy: () => invoke<ProxyStatus>('stop_proxy'),
  getProxyStatus: () => invoke<ProxyStatus>('get_proxy_status'),
  setRecording: (recording: boolean) => invoke<ProxyStatus>('set_recording', { recording }),
  listEntries: () => invoke<EntryMeta[]>('list_entries'),
  clearSession: () => invoke<void>('clear_session'),
  removeEntry: (id: string) => invoke<void>('remove_entry', { id }),
  getEntry: (id: string) => invoke<EntryDetail | null>('get_entry', { id }),
  repeatEntry: (id: string) => invoke<EntryMeta>('repeat_entry', { id }),
  compose: (payload: ComposePayload) => invoke<EntryMeta>('compose', { payload }),
  getSettings: () => invoke<AppSettings>('get_settings'),
  updateSettings: (patch: Partial<AppSettings>) => invoke<AppSettings>('update_settings', { patch }),
  getSystemProxy: () => invoke<SystemProxyState>('get_system_proxy'),
  getHttpsCaInfo: () => invoke<HttpsCaInfo>('get_https_ca_info'),
  ensureHttpsCa: () => invoke<HttpsCaInfo>('ensure_https_ca'),
  installHttpsCa: () => invoke<HttpsCaInfo>('install_https_ca'),
  revealHttpsCa: () => invoke<HttpsCaInfo>('reveal_https_ca'),
  quitApp: () => invoke<void>('quit_app'),
  copyText: (text: string) => writeText(text),

  /** 保存会话：先弹出系统保存对话框，再把路径交给后端写文件 */
  async saveSession(): Promise<SaveResult | null> {
    const path = await saveFileDialog({
      title: '保存会话',
      defaultPath: await defaultPathIn('documents', `FreeCharles-${timestamp()}.fchls`),
      filters: SESSION_FILTERS
    })
    if (!path) return null
    return invoke<SaveResult>('save_session', { path })
  },

  /** 打开会话：选择文件后由后端解析并广播新鲜列表 */
  async openSession(): Promise<SaveResult | null> {
    const selected = await openFileDialog({
      title: '打开会话',
      multiple: false,
      directory: false,
      filters: SESSION_FILTERS
    })
    if (!selected || typeof selected !== 'string') return null
    return invoke<SaveResult>('open_session', { path: selected })
  },

  async exportHar(): Promise<SaveResult | null> {
    const path = await saveFileDialog({
      title: '导出 HAR',
      defaultPath: await defaultPathIn('documents', `FreeCharles-${timestamp()}.har`),
      filters: [{ name: 'HTTP Archive', extensions: ['har'] }]
    })
    if (!path) return null
    return invoke<SaveResult>('export_har', { path })
  },

  async saveBody(payload: { id: string; side: 'request' | 'response' }): Promise<SaveBodyResult | null> {
    const suggested = await invoke<string>('suggest_body_file_name', payload)
    const path = await saveFileDialog({
      title: '保存内容',
      defaultPath: await defaultPathIn('downloads', suggested)
    })
    if (!path) return null
    return invoke<SaveBodyResult>('save_body', { ...payload, path })
  },

  onEntry: (callback: (meta: EntryMeta) => void) => subscribe('event:entry', callback),
  onEntryUpdate: (callback: (meta: EntryMeta) => void) => subscribe('event:entry-update', callback),
  onEntryRemove: (callback: (id: string) => void) => subscribe('event:entry-remove', callback),
  onClear: (callback: () => void) => subscribe('event:clear', callback),
  onLoaded: (callback: (metas: EntryMeta[]) => void) => subscribe('event:loaded', callback),
  onStatus: (callback: (status: ProxyStatus) => void) => subscribe('event:status', callback),
  onSettings: (callback: (settings: AppSettings) => void) => subscribe('event:settings', callback),
  onSystemProxy: (callback: (state: SystemProxyState) => void) => subscribe('event:system-proxy', callback),
  onCommand: (callback: (command: AppCommand) => void) => subscribe('event:command', callback)
}
