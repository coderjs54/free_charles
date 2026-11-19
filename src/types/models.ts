/** 请求/响应头键值对（保留重复头，如 Set-Cookie） */
export type HeaderPair = [string, string]

export type EntryState = 'pending' | 'complete' | 'error'

/** 列表行使用的轻量元数据，后端会通过事件增量推送给前端 */
export interface EntryMeta {
  id: string
  /** 会话内自增序号 */
  seq: number
  /** 请求开始时间（毫秒时间戳） */
  startTime: number
  method: string
  /** 完整 URL */
  url: string
  /** 主机名（不含端口） */
  host: string
  port: number
  /** pathname + search */
  path: string
  protocol: 'http' | 'https'
  clientIp: string
  status?: number | null
  statusText?: string | null
  state: EntryState
  error?: string | null
  /** 请求头 + 请求体字节数 */
  requestSize: number
  /** 响应头 + 响应体字节数 */
  responseSize: number
  /** 耗时（毫秒） */
  duration?: number | null
  requestMime: string
  responseMime: string
  /** 是否为 HTTPS CONNECT 隧道（未解密） */
  tunnel: boolean
  /** 是否为通过本地 CA 解密后的 HTTPS 请求 */
  decrypted?: boolean
  /** 是否为 WebSocket / Upgrade 连接（仅透传，不解析帧） */
  upgrade?: boolean
  note?: string | null
  /** 若为重放请求，指向原请求 id */
  repeatOf?: string | null
}

export interface BodyView {
  available: boolean
  /** 网络传输的字节数 */
  size: number
  /** 实际捕获到的字节数 */
  capturedSize: number
  /** 解码后字节数 */
  decodedSize: number
  /** 捕获是否被大小限制截断 */
  truncated: boolean
  /** 文本展示是否因过长被截断 */
  textTruncated: boolean
  kind: 'empty' | 'text' | 'binary'
  mime: string
  encoding?: string | null
  text?: string | null
  base64?: string | null
  decodeError?: string | null
  /** 大 Body 被延迟加载，需通过 getEntryBody 单独拉取 */
  deferred?: boolean
}

export interface TimingInfo {
  startedAt: number
  firstByteAt?: number | null
  completedAt?: number | null
  /** 收到首字节前的等待耗时 */
  waitMs?: number | null
  /** 接收响应体耗时 */
  receiveMs?: number | null
  totalMs?: number | null
}

/** 详情面板数据（按需请求，避免列表事件数据过大） */
export interface EntryDetail extends EntryMeta {
  requestHeaders: HeaderPair[]
  responseHeaders: HeaderPair[]
  requestHeaderSize: number
  responseHeaderSize: number
  requestBody: BodyView
  responseBody: BodyView
  timings: TimingInfo
  httpVersion: string
}

export interface ProxyStatus {
  running: boolean
  /** 供用户配置的代理地址（本机或局域网 IP） */
  host: string
  port: number
  recording: boolean
  error?: string | null
  startedAt?: number | null
}

/** 界面主题模式 */
export type ThemeMode = 'light' | 'dark'

export interface AppSettings {
  proxyPort: number
  autoStartProxy: boolean
  /** 允许局域网设备连接（绑定 0.0.0.0） */
  allowLan: boolean
  /** 启动代理时自动接管 Windows 系统代理，退出时恢复 */
  autoSystemProxy: boolean
  /** 是否已就「自动接管系统代理」征求过用户同意 */
  systemProxyPrompted: boolean
  /** 是否启用本地 CA，对 HTTPS 做 TLS 中间人解密 */
  decryptHttps: boolean
  /** 是否忽略代理到上游服务器的 TLS 证书错误 */
  allowInsecureUpstream: boolean
  /** 单个请求/响应体最大捕获字节数 */
  maxBodySize: number
  /** 会话最大保留条目数 */
  maxEntries: number
  /** 是否记录 HTTPS CONNECT 隧道（只显示提示，不抓内容） */
  recordTunnels: boolean
  /** 界面主题：浅色或暗黑 */
  theme: ThemeMode
}

/** Windows 系统代理自动配置状态 */
export interface SystemProxyState {
  supported: boolean
  active: boolean
  /** 当前接管的地址，例如 127.0.0.1:8888 */
  server: string
  error?: string | null
}

/** 本地 HTTPS 解密根证书状态 */
export interface HttpsCaInfo {
  /** 当前平台是否支持生成解密证书 */
  supported: boolean
  /** 是否支持一键安装到系统信任存储 */
  installable: boolean
  generated: boolean
  certPath?: string
  certPem?: string
  platform: string
  message?: string
  error?: string
}

export interface ComposePayload {
  method: string
  url: string
  /** 多行 "Name: value" 文本 */
  headers: string
  body: string
}

export type AppCommand =
  | 'clear-session'
  | 'open-settings'
  | 'compose'
  | 'save-session'
  | 'open-session'
  | 'export-har'
  | 'toggle-recording'
  | 'toggle-proxy'
  | 'focus-filter'
  | 'copy-proxy'
  | 'quit-app'
  | 'about'
