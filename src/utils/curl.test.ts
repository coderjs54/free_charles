import { describe, expect, it } from 'vitest'
import { parseCurlCommand } from './curl'

describe('parseCurlCommand', () => {
  it('解析 JSON POST 的 bash 格式', () => {
    const result = parseCurlCommand(
      `curl 'http://example.com/api/users' -X POST -H 'Content-Type: application/json' --data-raw '{"name":"demo"}'`
    )

    expect(result.ok).toBe(true)
    expect(result.parsed.method).toBe('POST')
    expect(result.parsed.url).toBe('http://example.com/api/users')
    expect(result.parsed.headers).toEqual([{ name: 'Content-Type', value: 'application/json' }])
    expect(result.parsed.body).toBe('{"name":"demo"}')
    expect(result.parsed.warnings).toEqual([])
  })

  it('支持 cmd 的 ^ 续行与 ^" 包裹', () => {
    const result = parseCurlCommand(
      'curl ^"http://example.com/api?x=1^" ^\n  -H ^"Accept: */*^" ^\n  --compressed'
    )

    expect(result.ok).toBe(true)
    expect(result.parsed.url).toBe('http://example.com/api?x=1')
    expect(result.parsed.headers).toContainEqual({ name: 'Accept', value: '*/*' })
    expect(result.parsed.headers).toContainEqual({ name: 'accept-encoding', value: 'gzip, deflate, br' })
  })

  it('-G 会把请求体挪到查询串', () => {
    const result = parseCurlCommand('curl -G http://example.com/search -d q=hello -d page=2')

    expect(result.ok).toBe(true)
    expect(result.parsed.method).toBe('GET')
    expect(result.parsed.url).toBe('http://example.com/search?q=hello&page=2')
    expect(result.parsed.body).toBe('')
  })

  it('处理 Basic Auth、Cookie 与 User-Agent', () => {
    const result = parseCurlCommand(
      `curl -u user:pass http://example.com -b 'a=1; b=2' -A 'TestAgent'`
    )

    expect(result.ok).toBe(true)
    expect(result.parsed.headers).toEqual([
      { name: 'authorization', value: 'Basic dXNlcjpwYXNz' },
      { name: 'cookie', value: 'a=1; b=2' },
      { name: 'user-agent', value: 'TestAgent' }
    ])
  })

  it('--json 会补充 JSON 相关 Header', () => {
    const result = parseCurlCommand(`curl --json '{"ok":true}' http://example.com/api`)

    expect(result.ok).toBe(true)
    expect(result.parsed.method).toBe('POST')
    expect(result.parsed.body).toBe('{"ok":true}')
    expect(result.parsed.headers).toContainEqual({ name: 'content-type', value: 'application/json' })
    expect(result.parsed.headers).toContainEqual({ name: 'accept', value: 'application/json' })
  })

  it('对不支持的 multipart 参数给出 warning', () => {
    const result = parseCurlCommand(`curl http://example.com/upload -F 'file=@a.txt'`)

    expect(result.ok).toBe(true)
    expect(result.parsed.warnings.some((warning) => warning.includes('multipart'))).toBe(true)
  })

  it('没有 URL 时返回失败原因', () => {
    const result = parseCurlCommand('curl -X POST -H "X-Test: 1"')

    expect(result.ok).toBe(false)
    expect(result.error).toContain('未找到请求 URL')
  })
})
