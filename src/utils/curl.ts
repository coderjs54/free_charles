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

export interface ParsedCurlHeader {
  name: string
  value: string
}

export interface ParsedCurl {
  method: string
  url: string
  headers: ParsedCurlHeader[]
  body: string
  /** 解析过程中忽略或不支持的参数提示 */
  warnings: string[]
}

export interface CurlParseResult {
  ok: boolean
  /** 解析失败原因；空输入时为提示文案 */
  error: string
  parsed: ParsedCurl
}

/** 需要读取下一个 token 作为取值的 curl 参数 */
const CURL_VALUE_FLAGS = new Set([
  '-X',
  '--request',
  '-H',
  '--header',
  '-d',
  '--data',
  '--data-raw',
  '--data-binary',
  '--data-ascii',
  '--data-urlencode',
  '--json',
  '-u',
  '--user',
  '-b',
  '--cookie',
  '-A',
  '--user-agent',
  '-e',
  '--referer',
  '-F',
  '--form',
  '--form-string',
  '--url',
  '-o',
  '--output',
  '-m',
  '--max-time',
  '--connect-timeout',
  '--retry',
  '--retry-delay',
  '--retry-max-time',
  '--cacert',
  '--capath',
  '--cert',
  '--key',
  '--pass',
  '--resolve',
  '--interface',
  '--limit-rate',
  '--ciphers',
  '-T',
  '--upload-file',
  '-w',
  '--write-out',
  '-c',
  '--cookie-jar',
  '-x',
  '--proxy'
])

/** 会改变请求语义、需要单独处理的参数（其余布尔开关直接忽略） */
const CURL_IGNORED_FLAGS = new Set([
  '-i',
  '--include',
  '-s',
  '--silent',
  '-S',
  '--show-error',
  '-v',
  '--verbose',
  '-L',
  '--location',
  '-k',
  '--insecure',
  '-N',
  '--no-buffer',
  '-f',
  '--fail',
  '-g',
  '--globoff',
  '--http1.1',
  '--http2',
  '--http2-prior-knowledge',
  '--no-keepalive',
  '--progress-bar',
  '--tlsv1.2',
  '--tlsv1.3',
  '-4',
  '-6',
  '-q',
  '-#'
])

/**
 * 解析从浏览器 DevTools / Postman 复制的 cURL 命令。
 * 支持 -X / -H / -d 系列 / -u / -b / -A / -e / --json / -G / --url 等常用参数。
 */
export function parseCurlCommand(input: string): CurlParseResult {
  const parsed: ParsedCurl = { method: '', url: '', headers: [], body: '', warnings: [] }
  const text = input.trim()
  if (!text) {
    return { ok: false, error: '粘贴 cURL 命令后会自动解析', parsed }
  }

  const tokens = tokenizeCurl(text)
  if (tokens.length === 0) {
    return { ok: false, error: '未能识别任何参数', parsed }
  }

  let index = 0
  // 跳过命令本身
  if (/^curl(\.exe)?$/i.test(tokens[0])) index = 1

  const dataParts: string[] = []
  let method = ''
  let useGet = false
  let isHead = false

  const addHeader = (name: string, value: string): void => {
    const normalized = trimQuotes(name)
    if (!normalized) return
    // HTTP/2 伪头（如 :authority）在 HTTP/1.1 下无效
    if (normalized.startsWith(':')) {
      parsed.warnings.push(`已忽略 HTTP/2 伪头 ${normalized}`)
      return
    }
    parsed.headers.push({ name: normalized, value: trimQuotes(value) })
  }

  while (index < tokens.length) {
    let token = tokens[index]
    index += 1

    // --flag=value 形式
    let inlineValue: string | undefined
    if (token.startsWith('--') && token.includes('=')) {
      const eq = token.indexOf('=')
      inlineValue = token.slice(eq + 1)
      token = token.slice(0, eq)
    }

    const takeValue = (): string => {
      if (inlineValue !== undefined) return inlineValue
      const next = tokens[index]
      if (next === undefined) return ''
      index += 1
      return next
    }

    if (token === '-G' || token === '--get') {
      useGet = true
      continue
    }
    if (token === '-I' || token === '--head') {
      isHead = true
      continue
    }
    if (token === '--compressed') {
      if (!parsed.headers.some((header) => header.name.toLowerCase() === 'accept-encoding')) {
        addHeader('accept-encoding', 'gzip, deflate, br')
      }
      continue
    }
    if (CURL_IGNORED_FLAGS.has(token)) continue

    if (token.startsWith('-')) {
      if (!CURL_VALUE_FLAGS.has(token)) {
        // 未知参数（多为开关），忽略即可
        continue
      }
      const value = takeValue()
      switch (token) {
        case '-X':
        case '--request':
          method = trimQuotes(value).toUpperCase()
          break
        case '-H':
        case '--header': {
          const headerText = trimQuotes(value)
          const colon = headerText.indexOf(':')
          if (colon <= 0) {
            parsed.warnings.push(`已忽略无法解析的 Header：${value}`)
            break
          }
          addHeader(headerText.slice(0, colon), headerText.slice(colon + 1))
          break
        }
        case '-d':
        case '--data':
        case '--data-raw':
        case '--data-ascii':
        case '--data-urlencode':
          if (value.startsWith('@')) {
            parsed.warnings.push('不支持从文件读取请求体（-d @file）')
            break
          }
          dataParts.push(value)
          break
        case '--data-binary':
          if (value.startsWith('@')) {
            parsed.warnings.push('不支持从文件读取请求体（--data-binary @file）')
            break
          }
          dataParts.push(value)
          break
        case '--json':
          dataParts.push(value)
          if (!parsed.headers.some((header) => header.name.toLowerCase() === 'content-type')) {
            addHeader('content-type', 'application/json')
          }
          if (!parsed.headers.some((header) => header.name.toLowerCase() === 'accept')) {
            addHeader('accept', 'application/json')
          }
          break
        case '-u':
        case '--user':
          addHeader('authorization', `Basic ${encodeBasicAuth(value)}`)
          break
        case '-b':
        case '--cookie':
          if (value.startsWith('@')) {
            parsed.warnings.push('不支持从文件读取 Cookie（-b @file）')
            break
          }
          addHeader('cookie', value)
          break
        case '-A':
        case '--user-agent':
          addHeader('user-agent', value)
          break
        case '-e':
        case '--referer':
          addHeader('referer', value)
          break
        case '-F':
        case '--form':
        case '--form-string':
          parsed.warnings.push(`暂不支持 multipart 表单参数（${token}）`)
          break
        case '-T':
        case '--upload-file':
          parsed.warnings.push('暂不支持上传文件（-T）')
          break
        case '--url':
          if (!parsed.url) parsed.url = sanitizeUrlToken(value)
          break
        default:
          // -o / --max-time / --proxy 等与发送内容无关，忽略取值
          break
      }
      continue
    }

    // 非参数 token 视为请求地址
    if (!parsed.url) parsed.url = sanitizeUrlToken(token)
  }

  if (!parsed.url) {
    return { ok: false, error: '未找到请求 URL，请检查粘贴内容', parsed }
  }

  parsed.body = dataParts.join('&')
  parsed.method = method || (isHead ? 'HEAD' : parsed.body ? 'POST' : 'GET')

  // -G 会把请求体挪到查询串
  if (useGet) {
    if (parsed.body) {
      parsed.url = appendQuery(parsed.url, parsed.body)
      parsed.body = ''
    }
    if (!method) parsed.method = 'GET'
  }

  // content-length 由后端按实际 body 重新计算，避免粘贴的旧值导致截断
  parsed.headers = parsed.headers.filter((header) => header.name.toLowerCase() !== 'content-length')

  return { ok: true, error: '', parsed }
}

/**
 * 按 shell / cmd 语法切分参数。
 * - bash：单/双引号、反斜杠续行与 \" 转义
 * - cmd：^ 续行、^" 转义引号（部分版本会写成 ^\"）
 */
function tokenizeCurl(source: string): string[] {
  const text = source
    .replace(/\\\r?\n/g, ' ')
    .replace(/`\r?\n/g, ' ')
    .replace(/\^\r?\n/g, ' ')
    // cmd 变体里整条参数可能用 ^" 包住，先还原成普通引号再切分
    .replace(/(^|\s)\^"/g, '$1"')
    .replace(/\^"(?=\s|$)/g, '"')

  const tokens: string[] = []
  let current = ''
  let quote: '"' | "'" | null = null
  let started = false

  for (let i = 0; i < text.length; i += 1) {
    const char = text[i]
    const next = text[i + 1]

    if (quote === "'") {
      if (char === "'") quote = null
      else current += char
      continue
    }

    if (quote === '"') {
      if (char === '"') {
        quote = null
        continue
      }
      // cmd 的 ^" 与 ^\" 都表示一个字面量引号
      if (char === '^') {
        if (next === '"') {
          current += '"'
          i += 1
          continue
        }
        if (next === '\\' && text[i + 2] === '"') {
          current += '"'
          i += 2
          continue
        }
        // 粘贴 cmd 命令时常见 ^{ ^} 这类转义残留，去掉脱字符
        if (next !== undefined && '{}[]'.includes(next)) {
          current += next
          i += 1
          continue
        }
        current += char
        continue
      }
      // bash 的 \" \\ 转义
      if (char === '\\' && (next === '"' || next === '\\' || next === '$' || next === '`')) {
        current += next
        i += 1
        continue
      }
      current += char
      continue
    }

    if (char === '"' || char === "'") {
      quote = char
      started = true
      continue
    }
    if (/\s/.test(char)) {
      if (started || current) {
        tokens.push(current)
        current = ''
        started = false
      }
      continue
    }
    // 引号外的 cmd ^ 转义（常见于 ^" 包住整个参数）
    if (char === '^' && next !== undefined) {
      current += next
      i += 1
      started = true
      continue
    }
    if (char === '\\' && next !== undefined) {
      current += next
      i += 1
      started = true
      continue
    }
    current += char
    started = true
  }
  if (started || current) tokens.push(current)
  return tokens
}

/** 清掉 cmd 粘贴残留的包裹符号，避免把 URL 判定为非法 */
function sanitizeUrlToken(value: string): string {
  return value.replace(/^[\s"'^\\]+/, '').replace(/[\s"'^\\]+$/, '')
}

/** 去掉 cmd / bash 包裹参数时残留的首尾引号 */
function trimQuotes(value: string): string {
  return value.trim().replace(/^["']+/, '').replace(/["']+$/, '').trim()
}

function appendQuery(url: string, query: string): string {
  if (!query) return url
  return url.includes('?') ? `${url}&${query}` : `${url}?${query}`
}

function encodeBasicAuth(credentials: string): string {
  const bytes = new TextEncoder().encode(credentials)
  let binary = ''
  bytes.forEach((byte) => {
    binary += String.fromCharCode(byte)
  })
  return btoa(binary)
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
