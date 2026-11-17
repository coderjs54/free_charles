import type { EntryMeta } from '../types/models'

export function formatBytes(value?: number | null): string {
  if (!value || value <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let size = value
  let index = 0
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024
    index += 1
  }
  if (index === 0) return `${Math.round(size)} B`
  return `${size >= 100 ? Math.round(size) : size.toFixed(1)} ${units[index]}`
}

export function formatDuration(ms?: number | null): string {
  if (ms === undefined || ms === null) return '—'
  if (ms < 1) return '<1 ms'
  if (ms < 1000) return `${Math.round(ms)} ms`
  return `${(ms / 1000).toFixed(2)} s`
}

export function formatTime(timestamp: number): string {
  const date = new Date(timestamp)
  const pad = (value: number, length = 2): string => String(value).padStart(length, '0')
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`
}

export function formatDateTime(timestamp: number): string {
  const date = new Date(timestamp)
  const pad = (value: number): string => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

export type Tone = 'success' | 'info' | 'warning' | 'danger' | 'muted'

export function statusTone(meta: Pick<EntryMeta, 'state' | 'status'>): Tone {
  if (meta.state === 'error') return 'danger'
  if (meta.state === 'pending') return 'muted'
  const code = meta.status ?? 0
  if (code >= 200 && code < 300) return 'success'
  if (code >= 300 && code < 400) return 'info'
  if (code >= 400 && code < 500) return 'warning'
  if (code >= 500) return 'danger'
  return 'muted'
}

export function statusCode(meta: Pick<EntryMeta, 'state' | 'status'>): string {
  if (meta.state === 'pending') return '···'
  if (meta.state === 'error' && !meta.status) return 'ERR'
  return meta.status ? String(meta.status) : '—'
}

export function statusFullText(meta: Pick<EntryMeta, 'state' | 'status' | 'statusText'>): string {
  if (meta.state === 'pending') return '等待响应…'
  if (meta.state === 'error' && !meta.status) return '请求失败'
  return `${meta.status ?? '—'} ${meta.statusText ?? ''}`.trim()
}

export function methodTone(method: string): Tone {
  switch (method.toUpperCase()) {
    case 'GET':
      return 'info'
    case 'POST':
      return 'success'
    case 'PUT':
      return 'warning'
    case 'DELETE':
      return 'danger'
    case 'PATCH':
      return 'info'
    default:
      return 'muted'
  }
}

export function hostLabel(meta: Pick<EntryMeta, 'host' | 'port' | 'protocol'>): string {
  const defaultPort = meta.protocol === 'https' ? 443 : 80
  return meta.port === defaultPort ? meta.host : `${meta.host}:${meta.port}`
}

export function sizeLabel(meta: EntryMeta): string {
  const size = meta.responseSize || meta.requestSize
  return size > 0 ? formatBytes(size) : '—'
}

export function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (char) => {
    switch (char) {
      case '&':
        return '&amp;'
      case '<':
        return '&lt;'
      case '>':
        return '&gt;'
      case '"':
        return '&quot;'
      default:
        return '&#39;'
    }
  })
}
