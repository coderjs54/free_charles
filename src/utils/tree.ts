import type { EntryMeta } from '../types/models'
import { hostLabel } from './format'

interface TreeNode {
  key: string
  label: string
  children: Map<string, TreeNode>
  entries: EntryMeta[]
  count: number
}

export interface FolderRow {
  kind: 'folder'
  key: string
  label: string
  depth: number
  count: number
  collapsed: boolean
}

export interface EntryRow {
  kind: 'entry'
  key: string
  depth: number
  entry: EntryMeta
}

export type ListRow = FolderRow | EntryRow

/** 按 Host -> 路径分段构建 Charles 风格的 Structure 树 */
export function buildStructureRows(
  entries: EntryMeta[],
  collapsed: Set<string>,
  ignoreCollapsed = false
): ListRow[] {
  const hosts = new Map<string, TreeNode>()

  for (const entry of entries) {
    const key = `${entry.protocol}://${hostLabel(entry)}`
    let hostNode = hosts.get(key)
    if (!hostNode) {
      hostNode = { key, label: hostLabel(entry), children: new Map(), entries: [], count: 0 }
      hosts.set(key, hostNode)
    }
    hostNode.count += 1

    let node = hostNode
    const pathname = entry.path.split('?')[0]
    const segments = pathname.split('/').filter(Boolean)
    // 最后一段作为请求行直接挂在父目录下，避免出现「同名目录 + 同名请求行」的重复层级
    const folders = segments.slice(0, -1)
    for (const segment of folders) {
      let child = node.children.get(segment)
      if (!child) {
        child = {
          key: `${node.key}/${segment}`,
          label: segment,
          children: new Map(),
          entries: [],
          count: 0
        }
        node.children.set(segment, child)
      }
      node = child
      node.count += 1
    }
    node.entries.push(entry)
  }

  const rows: ListRow[] = []
  const sortedHosts = [...hosts.values()].sort((a, b) => a.label.localeCompare(b.label, 'zh-CN'))

  const pushNode = (node: TreeNode, depth: number): void => {
    const isCollapsed = collapsed.has(node.key) && !ignoreCollapsed
    rows.push({
      kind: 'folder',
      key: node.key,
      label: node.label,
      depth,
      count: node.count,
      collapsed: isCollapsed
    })
    if (isCollapsed) return

    const children = [...node.children.values()].sort((a, b) => a.label.localeCompare(b.label, 'zh-CN'))
    for (const child of children) pushNode(child, depth + 1)

    const sorted = node.entries.slice().sort((a, b) => a.seq - b.seq)
    for (const entry of sorted) {
      rows.push({ kind: 'entry', key: entry.id, depth: depth + 1, entry })
    }
  }

  for (const host of sortedHosts) pushNode(host, 0)
  return rows
}

/** Structure 行显示的最后一段路径名 */
export function entryLeafName(entry: EntryMeta): string {
  if (entry.tunnel || entry.method === 'CONNECT') return 'HTTPS 隧道'
  const pathname = entry.path.split('?')[0]
  const segments = pathname.split('/').filter(Boolean)
  return segments.length > 0 ? segments[segments.length - 1] : '/'
}

export function entryQuery(entry: EntryMeta): string {
  const index = entry.path.indexOf('?')
  return index >= 0 ? entry.path.slice(index) : ''
}
