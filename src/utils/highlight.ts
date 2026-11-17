import { escapeHtml } from './format'

/** 将 JSON 文本渲染为带高亮 span 的 HTML（先转义，避免 XSS） */
export function highlightJson(text: string): string {
  const escaped = escapeHtml(text)
  return escaped.replace(
    /("(?:\\.|[^"\\])*")(\s*:)?|\b(true|false|null)\b|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g,
    (match, str: string | undefined, colon: string | undefined, bool: string | undefined, num: string | undefined) => {
      if (str !== undefined) {
        if (colon) return `<span class="fc-token-key">${str}</span>${colon}`
        return `<span class="fc-token-string">${str}</span>`
      }
      if (bool !== undefined) {
        const className = bool === 'null' ? 'fc-token-null' : 'fc-token-boolean'
        return `<span class="${className}">${match}</span>`
      }
      if (num !== undefined) return `<span class="fc-token-number">${match}</span>`
      return match
    }
  )
}

export function prettyJson(text: string): { text: string; ok: boolean } {
  try {
    return { text: JSON.stringify(JSON.parse(text), null, 2), ok: true }
  } catch {
    return { text, ok: false }
  }
}

export function base64ToBytes(base64: string, maxBytes = 64 * 1024): Uint8Array {
  const binary = atob(base64)
  const length = Math.min(binary.length, maxBytes)
  const bytes = new Uint8Array(length)
  for (let i = 0; i < length; i += 1) bytes[i] = binary.charCodeAt(i)
  return bytes
}

export function formatHexDump(base64: string, maxBytes = 64 * 1024): string {
  const bytes = base64ToBytes(base64, maxBytes)
  const lines: string[] = []
  for (let offset = 0; offset < bytes.length; offset += 16) {
    const chunk = bytes.subarray(offset, offset + 16)
    const hex = Array.from(chunk)
      .map((byte) => byte.toString(16).padStart(2, '0'))
      .join(' ')
      .padEnd(47, ' ')
    const ascii = Array.from(chunk)
      .map((byte) => (byte >= 32 && byte <= 126 ? String.fromCharCode(byte) : '.'))
      .join('')
    lines.push(`${offset.toString(16).padStart(8, '0')}  ${hex}  |${ascii}|`)
  }
  if (bytes.length === 0) lines.push('（空内容）')
  return lines.join('\n')
}

export function toDataUrl(mime: string, base64: string): string {
  return `data:${mime || 'application/octet-stream'};base64,${base64}`
}
