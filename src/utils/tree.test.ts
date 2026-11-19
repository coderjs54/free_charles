import { describe, expect, it } from 'vitest'
import type { EntryMeta } from '../types/models'
import { buildStructureRows, entryLeafName } from './tree'

let seq = 0

function entry(overrides: Partial<EntryMeta>): EntryMeta {
  seq += 1
  return {
    id: `entry-${seq}`,
    seq,
    startTime: Date.now(),
    method: 'GET',
    url: 'http://example.com/',
    host: 'example.com',
    port: 80,
    path: '/',
    protocol: 'http',
    clientIp: '127.0.0.1',
    state: 'complete',
    requestSize: 0,
    responseSize: 0,
    requestMime: '',
    responseMime: '',
    tunnel: false,
    ...overrides
  }
}

describe('buildStructureRows', () => {
  const entries = [
    entry({ path: '/api/users', url: 'http://example.com/api/users' }),
    entry({ path: '/api/posts', url: 'http://example.com/api/posts' }),
    entry({ path: '/static/app.js', url: 'http://example.com/static/app.js' }),
    entry({ host: 'other.com', url: 'http://other.com/', path: '/' })
  ]

  it('按 Host 和路径分段构建树，并按序号排列请求', () => {
    const rows = buildStructureRows(entries, new Set())

    expect(rows.map((row) => (row.kind === 'folder' ? row.label : entryLeafName(row.entry)))).toEqual([
      'example.com',
      'api',
      'users',
      'posts',
      'static',
      'app.js',
      'other.com',
      '/'
    ])
    expect(rows[0]).toMatchObject({ kind: 'folder', count: 3, depth: 0 })
    expect(rows[1]).toMatchObject({ kind: 'folder', count: 2, depth: 1 })
  })

  it('折叠目录后隐藏子节点', () => {
    const rows = buildStructureRows(entries, new Set(['http://example.com/api']))

    expect(rows).toHaveLength(6)
    expect(rows[1]).toMatchObject({ kind: 'folder', label: 'api', collapsed: true })
  })

  it('过滤时忽略折叠状态', () => {
    const rows = buildStructureRows(entries, new Set(['http://example.com/api']), true)

    expect(rows).toHaveLength(8)
    expect(rows[1]).toMatchObject({ kind: 'folder', label: 'api', collapsed: false })
  })

  it('HTTPS 与 HTTP 的同名 Host 会分成不同目录', () => {
    const rows = buildStructureRows(
      [
        entry({ protocol: 'http', url: 'http://example.com/a', path: '/a' }),
        entry({ protocol: 'https', port: 443, url: 'https://example.com/b', path: '/b' })
      ],
      new Set()
    )

    expect(rows.filter((row) => row.kind === 'folder')).toHaveLength(2)
  })
})

describe('entryLeafName', () => {
  it('隧道条目显示固定名称', () => {
    expect(entryLeafName(entry({ method: 'CONNECT', tunnel: true }))).toBe('HTTPS 隧道')
  })

  it('根路径显示 /', () => {
    expect(entryLeafName(entry({ path: '/' }))).toBe('/')
  })
})
