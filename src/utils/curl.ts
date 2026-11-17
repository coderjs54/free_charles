import type { EntryDetail } from '../types/models'

function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`
}

/** 根据捕获到的请求生成可直接执行的 cURL 命令 */
export function buildCurl(detail: EntryDetail): string {
  const parts = [`curl -X ${detail.method}`, shellQuote(detail.url)]
  for (const [name, value] of detail.requestHeaders) {
    const lower = name.toLowerCase()
    if (lower === 'host' || lower === 'content-length' || lower === 'proxy-connection' || lower === 'connection') continue
    parts.push(`-H ${shellQuote(`${name}: ${value}`)}`)
  }
  if (detail.requestBody.kind === 'text' && detail.requestBody.text) {
    parts.push(`--data-raw ${shellQuote(detail.requestBody.text)}`)
  }
  return parts.join(' \\\n  ')
}

export function buildRawRequest(detail: EntryDetail): string {
  const lines = [`${detail.method} ${detail.path || '/'} ${detail.httpVersion}`]
  for (const [name, value] of detail.requestHeaders) lines.push(`${name}: ${value}`)
  let text = lines.join('\r\n') + '\r\n\r\n'
  if (detail.requestBody.kind === 'text' && detail.requestBody.text) text += detail.requestBody.text
  else if (detail.requestBody.kind === 'binary') text += `[二进制内容，共 ${detail.requestBody.decodedSize} 字节]`
  return text
}

export function buildRawResponse(detail: EntryDetail): string {
  const lines = [`${detail.httpVersion} ${detail.status ?? ''} ${detail.statusText ?? ''}`.trim()]
  for (const [name, value] of detail.responseHeaders) lines.push(`${name}: ${value}`)
  let text = lines.join('\r\n') + '\r\n\r\n'
  if (detail.responseBody.kind === 'text' && detail.responseBody.text) text += detail.responseBody.text
  else if (detail.responseBody.kind === 'binary') text += `[二进制内容，共 ${detail.responseBody.decodedSize} 字节]`
  return text
}
