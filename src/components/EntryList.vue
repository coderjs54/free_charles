<script setup lang="ts">
import { api } from '../lib/api'
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import type { EntryMeta } from '../types/models'
import { useSession } from '../stores/useSession'
import { useToast } from '../stores/useToast'
import { buildCurl } from '../utils/curl'
import { errorMessage } from '../utils/error'
import {
  formatDuration,
  formatTime,
  hostLabel,
  methodTone,
  sizeLabel,
  statusCode,
  statusTone
} from '../utils/format'
import type { ContextMenuItem } from '../types/ui'
import { buildStructureRows, entryLeafName, entryQuery, type EntryRow, type ListRow } from '../utils/tree'
import AppIcon from './AppIcon.vue'
import ContextMenu from './ContextMenu.vue'

const session = useSession()
const {
  view,
  filter,
  showHttps,
  entries,
  filteredEntries,
  selectedId,
  proxyAddress,
  stats,
  systemProxyState,
  setHttpsVisible
} = session
const { push } = useToast()

const ROW_HEIGHT = 29
const collapsed = ref<Set<string>>(new Set())
const scrollEl = ref<HTMLElement | null>(null)
const scrollTop = ref(0)
const viewportHeight = ref(600)
const menu = reactive<{ open: boolean; x: number; y: number; entry: EntryMeta | null }>({
  open: false,
  x: 0,
  y: 0,
  entry: null
})

let resizeObserver: ResizeObserver | null = null

const rows = computed<ListRow[]>(() => {
  if (view.value === 'structure') {
    return buildStructureRows(filteredEntries.value, collapsed.value, Boolean(filter.value.trim()))
  }
  return filteredEntries.value.map((entry) => ({ kind: 'entry', key: entry.id, depth: 0, entry }))
})

// 固定行高的轻量虚拟列表：只渲染可视区域附近的行，保证数千条请求时依然流畅
const startIndex = computed(() => Math.max(0, Math.floor(scrollTop.value / ROW_HEIGHT) - 6))
const endIndex = computed(() =>
  Math.min(rows.value.length, Math.ceil((scrollTop.value + viewportHeight.value) / ROW_HEIGHT) + 8)
)
const visibleRows = computed(() => rows.value.slice(startIndex.value, endIndex.value))
const windowOffset = computed(() => startIndex.value * ROW_HEIGHT)

const menuItems = computed<ContextMenuItem[]>(() => {
  const entry = menu.entry
  if (!entry) return []
  const isTunnel = entry.tunnel || entry.method === 'CONNECT'
  return [
    {
      id: 'repeat',
      label: '重放请求',
      icon: 'repeat',
      disabled: isTunnel || entry.state === 'pending' || Boolean(entry.upgrade)
    },
    { id: 'copy-url', label: '复制 URL', icon: 'copy' },
    { id: 'copy-curl', label: '复制为 cURL', icon: 'code', disabled: isTunnel },
    { id: 'copy-body', label: '复制响应体', icon: 'file', disabled: isTunnel || entry.state !== 'complete' },
    { id: 'save-body', label: '保存响应体…', icon: 'download', disabled: isTunnel || entry.state !== 'complete' },
    { id: 'separator', label: '', separator: true },
    { id: 'remove', label: '删除请求', icon: 'trash', danger: true }
  ]
})

function toggleFolder(key: string): void {
  const next = new Set(collapsed.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  collapsed.value = next
}

function expandAll(): void {
  collapsed.value = new Set()
}

function collapseAll(): void {
  const keys = rows.value.filter((row) => row.kind === 'folder').map((row) => row.key)
  collapsed.value = new Set(keys)
}

function onScroll(): void {
  if (!scrollEl.value) return
  scrollTop.value = scrollEl.value.scrollTop
}

function scrollToRow(id: string): void {
  const index = rows.value.findIndex((row) => row.kind === 'entry' && row.entry.id === id)
  const el = scrollEl.value
  if (index < 0 || !el) return
  const top = index * ROW_HEIGHT
  if (top < el.scrollTop) el.scrollTop = top
  else if (top + ROW_HEIGHT > el.scrollTop + el.clientHeight) {
    el.scrollTop = top + ROW_HEIGHT - el.clientHeight + 6
  }
  scrollTop.value = el.scrollTop
}

function moveSelection(delta: number): void {
  const entryRows = rows.value.filter((row): row is EntryRow => row.kind === 'entry')
  if (entryRows.length === 0) return
  const current = entryRows.findIndex((row) => row.entry.id === selectedId.value)
  const nextIndex = Math.min(entryRows.length - 1, Math.max(0, current + delta))
  const target = entryRows[nextIndex]
  if (!target) return
  void session.select(target.entry.id)
  scrollToRow(target.entry.id)
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'ArrowDown') {
    event.preventDefault()
    moveSelection(1)
    return
  }
  if (event.key === 'ArrowUp') {
    event.preventDefault()
    moveSelection(-1)
    return
  }
  if ((event.key === 'Delete' || event.key === 'Backspace') && selectedId.value) {
    event.preventDefault()
    void session.removeEntry(selectedId.value)
  }
}

function selectRow(entry: EntryMeta): void {
  // 让列表容器获得焦点，方便继续使用上下键和 Delete 操作
  scrollEl.value?.focus({ preventScroll: true })
  void session.select(entry.id)
}

function openMenu(event: MouseEvent, entry: EntryMeta): void {
  void session.select(entry.id)
  menu.entry = entry
  menu.x = event.clientX
  menu.y = event.clientY
  menu.open = true
}

async function copyText(text: string, message: string): Promise<void> {
  try {
    await api.copyText(text)
    push(message, 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

async function onMenuSelect(id: string): Promise<void> {
  menu.open = false
  const entry = menu.entry
  if (!entry) return
  try {
    if (id === 'repeat') {
      const meta = await api.repeatEntry(entry.id)
      await session.select(meta.id)
      push('已重放请求，新条目已添加到会话', 'success')
      return
    }
    if (id === 'copy-url') {
      await copyText(entry.url, '已复制 URL')
      return
    }
    if (id === 'remove') {
      await session.removeEntry(entry.id)
      push('已删除该请求')
      return
    }
    const detail = await api.getEntry(entry.id)
    if (!detail) {
      push('请求已不存在', 'error')
      return
    }
    if (id === 'copy-curl') {
      await copyText(buildCurl(detail), '已复制 cURL 命令')
      return
    }
    if (id === 'copy-body') {
      if (detail.responseBody.kind === 'text' && detail.responseBody.text) {
        await copyText(detail.responseBody.text, '已复制响应体')
      } else if (detail.responseBody.kind === 'binary') {
        push('二进制响应体无法直接复制，请使用“保存响应体”', 'error')
      } else {
        push('响应体为空', 'error')
      }
      return
    }
    if (id === 'save-body') {
      const result = await api.saveBody({ id: entry.id, side: 'response' })
      if (result) push('响应体已保存', 'success')
    }
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

watch(
  () => filter.value,
  async () => {
    await nextTick()
    if (scrollEl.value) {
      scrollEl.value.scrollTop = 0
      scrollTop.value = 0
    }
  }
)

watch(view, async (next) => {
  await nextTick()
  if (!scrollEl.value) return
  if (next === 'sequence') {
    scrollEl.value.scrollTop = scrollEl.value.scrollHeight
    scrollTop.value = scrollEl.value.scrollTop
  } else {
    scrollEl.value.scrollTop = 0
    scrollTop.value = 0
  }
})

watch(
  () => rows.value.length,
  async (next, previous) => {
    if (view.value !== 'sequence' || !scrollEl.value || next <= previous) return
    const el = scrollEl.value
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 12
    await nextTick()
    if (atBottom) {
      el.scrollTop = el.scrollHeight
      scrollTop.value = el.scrollTop
    }
  }
)

onMounted(() => {
  if (scrollEl.value) {
    resizeObserver = new ResizeObserver((entriesList) => {
      const rect = entriesList[0]?.contentRect
      if (rect) viewportHeight.value = rect.height
    })
    resizeObserver.observe(scrollEl.value)
    viewportHeight.value = scrollEl.value.clientHeight
  }
  if (view.value === 'sequence') {
    void nextTick(() => {
      if (scrollEl.value) scrollEl.value.scrollTop = scrollEl.value.scrollHeight
    })
  }
})

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
})
</script>

<template>
  <section class="fc-entry-list">
    <div class="fc-list-topbar">
      <span class="fc-list-title">{{ view === 'structure' ? 'Structure' : 'Sequence' }}</span>
      <span class="fc-list-count">
        {{ stats.filtered }}
        <template v-if="filter.trim() || !showHttps"> / {{ stats.total }}</template>
        requests
      </span>
      <div class="fc-list-topbar-spacer" />
      <div v-if="view === 'structure'" class="fc-list-tools">
        <button class="fc-list-tool" type="button" title="全部展开" aria-label="全部展开" @click="expandAll">
          <AppIcon name="expand" :size="13" />
        </button>
        <button class="fc-list-tool" type="button" title="全部折叠" aria-label="全部折叠" @click="collapseAll">
          <AppIcon name="collapse" :size="13" />
        </button>
      </div>
      <span v-if="stats.errors > 0" class="fc-list-errors">{{ stats.errors }} failed</span>
    </div>

    <div v-if="view === 'structure'" class="fc-list-header fc-list-header--structure">
      <span class="fc-list-header-cell fc-list-header-name">Name</span>
      <span class="fc-list-header-cell">Method</span>
      <span class="fc-list-header-cell">Status</span>
      <span class="fc-list-header-cell fc-list-align-right">Size</span>
      <span class="fc-list-header-cell fc-list-align-right">Time</span>
    </div>
    <div v-else class="fc-list-header fc-list-header--sequence">
      <span class="fc-list-header-cell">Time</span>
      <span class="fc-list-header-cell">Method</span>
      <span class="fc-list-header-cell">URL</span>
      <span class="fc-list-header-cell">Status</span>
      <span class="fc-list-header-cell fc-list-align-right">Size</span>
      <span class="fc-list-header-cell fc-list-align-right">Duration</span>
    </div>

    <div
      ref="scrollEl"
      class="fc-list-scroll"
      tabindex="0"
      :role="view === 'structure' ? 'tree' : 'list'"
      aria-label="请求列表"
      @scroll="onScroll"
      @keydown="onKeydown"
    >
      <div v-if="entries.length === 0" class="fc-list-empty">
        <div class="fc-list-empty-icon">
          <AppIcon name="globe" :size="24" />
        </div>
        <p class="fc-list-empty-title">还没有捕获到请求</p>
        <p v-if="systemProxyState.active" class="fc-list-empty-text">
          系统代理已自动接管，直接访问以
          <code class="fc-list-empty-code">http://</code>
          开头的页面即可。
        </p>
        <p v-else class="fc-list-empty-text">
          将浏览器或系统的 HTTP 代理设置为
          <code class="fc-list-empty-code">{{ proxyAddress }}</code>
          后访问任意 HTTP 页面。
        </p>
      </div>

      <div v-else-if="rows.length === 0" class="fc-list-empty">
        <div class="fc-list-empty-icon">
          <AppIcon name="search" :size="22" />
        </div>
        <p class="fc-list-empty-title">没有匹配的请求</p>
        <p class="fc-list-empty-text">
          {{ !showHttps && stats.https > 0 ? 'HTTPS 请求当前被隐藏，或没有匹配的关键字。' : '试试其他关键字，或清除过滤条件。' }}
        </p>
        <div class="fc-list-empty-actions">
          <button
            v-if="!showHttps && stats.https > 0"
            class="fc-empty-action"
            type="button"
            @click="setHttpsVisible(true)"
          >
            显示 HTTPS 请求
          </button>
          <button v-if="filter.trim()" class="fc-empty-action" type="button" @click="filter = ''">清除过滤</button>
        </div>
      </div>

      <div v-else class="fc-list-spacer" :style="{ height: `${rows.length * ROW_HEIGHT}px` }">
        <div class="fc-list-window" :style="{ transform: `translateY(${windowOffset}px)` }">
          <template v-for="row in visibleRows" :key="row.key">
            <div
              v-if="row.kind === 'folder'"
              class="fc-folder-row"
              :style="{ paddingLeft: `${10 + row.depth * 14}px` }"
              role="treeitem"
              :aria-expanded="!row.collapsed"
              tabindex="0"
              @click="toggleFolder(row.key)"
              @keydown.enter.prevent.stop="toggleFolder(row.key)"
              @keydown.space.prevent.stop="toggleFolder(row.key)"
            >
              <span class="fc-folder-chevron" :class="{ 'fc-folder-chevron--collapsed': row.collapsed }">
                <AppIcon name="chevron-down" :size="11" />
              </span>
              <span class="fc-folder-icon">
                <AppIcon name="folder" :size="13" />
              </span>
              <span class="fc-folder-name">{{ row.label }}</span>
              <span class="fc-folder-count">{{ row.count }}</span>
            </div>

            <div
              v-else
              class="fc-entry-row"
              :class="[
                view === 'structure' ? 'fc-entry-row--structure' : 'fc-entry-row--sequence',
                { 'fc-entry-row--selected': row.entry.id === selectedId, 'fc-entry-row--error': row.entry.state === 'error' }
              ]"
              :role="view === 'structure' ? 'treeitem' : 'listitem'"
              :aria-selected="view === 'structure' ? row.entry.id === selectedId : undefined"
              :aria-current="view === 'sequence' && row.entry.id === selectedId ? 'true' : undefined"
              :data-entry-id="row.entry.id"
              :title="row.entry.url"
              @click="selectRow(row.entry)"
              @contextmenu.prevent="openMenu($event, row.entry)"
            >
              <template v-if="view === 'structure'">
                <span class="fc-entry-name" :style="{ paddingLeft: `${row.depth * 14}px` }">
                  <span
                    class="fc-entry-icon"
                    :class="{ 'fc-entry-icon--tunnel': row.entry.tunnel || row.entry.method === 'CONNECT' }"
                  >
                    <AppIcon
                      :name="row.entry.protocol === 'https' ? 'lock' : 'file'"
                      :size="12"
                    />
                  </span>
                  <span class="fc-entry-label">{{ entryLeafName(row.entry) }}</span>
                  <span v-if="entryQuery(row.entry)" class="fc-entry-query">{{ entryQuery(row.entry) }}</span>
                </span>
                <span class="fc-method" :class="`fc-method--${methodTone(row.entry.method)}`">
                  {{ row.entry.method }}
                </span>
                <span class="fc-status" :class="`fc-status--${statusTone(row.entry)}`">
                  {{ statusCode(row.entry) }}
                </span>
                <span class="fc-cell-mono fc-align-right">{{ sizeLabel(row.entry) }}</span>
                <span class="fc-cell-mono fc-align-right">{{ formatDuration(row.entry.duration) }}</span>
              </template>

              <template v-else>
                <span class="fc-cell-mono fc-time">{{ formatTime(row.entry.startTime) }}</span>
                <span class="fc-method" :class="`fc-method--${methodTone(row.entry.method)}`">
                  {{ row.entry.method }}
                </span>
                <span class="fc-entry-url">
                  <span class="fc-entry-host">{{ hostLabel(row.entry) }}</span>
                  <span class="fc-entry-path">{{ row.entry.path }}</span>
                </span>
                <span class="fc-status" :class="`fc-status--${statusTone(row.entry)}`">
                  {{ statusCode(row.entry) }}
                </span>
                <span class="fc-cell-mono fc-align-right">{{ sizeLabel(row.entry) }}</span>
                <span class="fc-cell-mono fc-align-right">{{ formatDuration(row.entry.duration) }}</span>
              </template>
            </div>
          </template>
        </div>
      </div>
    </div>

    <ContextMenu
      :open="menu.open"
      :x="menu.x"
      :y="menu.y"
      :items="menuItems"
      @select="onMenuSelect"
      @close="menu.open = false"
    />
  </section>
</template>

<style scoped lang="scss">
.fc-entry-list {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-width: 0;
  background: $color-surface;
}

.fc-list-topbar {
  display: flex;
  align-items: center;
  gap: $spacing-md;
  height: $list-topbar-height;
  padding: 0 $spacing-lg;
  border-bottom: 1px solid $color-border;
  background: $color-surface;
  flex: none;
}

.fc-list-title {
  color: $color-text-secondary;
  font-size: 12.5px;
  font-weight: 700;
  letter-spacing: 0.07em;
  text-transform: uppercase;
}

.fc-list-count {
  color: $color-text-tertiary;
  font-size: 12.5px;
}

.fc-list-topbar-spacer {
  flex: 1;
}

.fc-list-tools {
  display: flex;
  gap: 2px;
}

.fc-list-tool {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: $radius-xs;
  color: $color-text-tertiary;
}

.fc-list-tool:hover {
  background: $color-hover;
  color: $color-text-secondary;
}

.fc-list-errors {
  padding: 1px 6px;
  border-radius: $radius-pill;
  background: $color-danger-soft;
  color: $color-danger;
  font-size: 11.5px;
  font-weight: 600;
}

.fc-list-header {
  display: grid;
  column-gap: 4px;
  align-items: center;
  height: 28px;
  padding: 0 10px;
  border-bottom: 1px solid $color-border;
  background: $color-surface-muted;
  color: $color-text-tertiary;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  flex: none;
}

.fc-list-header--structure {
  grid-template-columns: minmax(0, 1fr) 56px 48px 68px 66px;
}

.fc-list-header--sequence {
  grid-template-columns: 84px 56px minmax(0, 1fr) 48px 68px 66px;
}

.fc-list-header-name {
  padding-left: 2px;
}

.fc-list-header-cell {
  overflow: hidden;
  white-space: nowrap;
}

.fc-list-align-right {
  text-align: right;
}

.fc-list-scroll {
  position: relative;
  flex: 1;
  overflow-x: hidden;
  overflow-y: auto;
  outline: none;
}

.fc-list-spacer {
  position: relative;
}

.fc-list-window {
  position: absolute;
  top: 0;
  right: 0;
  left: 0;
  will-change: transform;
}

.fc-folder-row {
  display: flex;
  align-items: center;
  gap: 5px;
  height: $row-height;
  padding-right: 10px;
  border-bottom: 1px solid $color-divider;
  background: $color-surface-raised;
  color: $color-text-secondary;
  font-size: 12.5px;
  font-weight: 550;
  cursor: pointer;
  user-select: none;
}

.fc-folder-row:hover {
  background: $color-hover;
}

.fc-folder-chevron {
  display: flex;
  color: $color-text-tertiary;
  transition: transform $transition-fast;
}

.fc-folder-chevron--collapsed {
  transform: rotate(-90deg);
}

.fc-folder-icon {
  display: flex;
  color: $color-text-tertiary;
}

.fc-folder-name {
  max-width: 60%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-folder-count {
  color: $color-text-tertiary;
  font-family: $font-mono;
  font-size: 11.5px;
  font-weight: 500;
}

.fc-entry-row {
  display: grid;
  column-gap: 4px;
  align-items: center;
  height: $row-height;
  padding: 0 10px;
  border-bottom: 1px solid $color-divider;
  background: $color-surface;
  color: $color-text;
  font-size: 12.5px;
  cursor: default;
  user-select: none;
}

.fc-entry-row--structure {
  grid-template-columns: minmax(0, 1fr) 56px 48px 68px 66px;
}

.fc-entry-row--sequence {
  grid-template-columns: 84px 56px minmax(0, 1fr) 48px 68px 66px;
}

.fc-entry-row:hover {
  background: $color-hover;
}

.fc-entry-row--selected,
.fc-entry-row--selected:hover {
  background: $color-selected;
  box-shadow: inset 2px 0 0 $color-primary;
}

.fc-entry-row--error .fc-entry-label {
  color: $color-danger;
}

.fc-entry-name {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  padding-right: 4px;
}

.fc-entry-icon {
  display: flex;
  color: $color-text-tertiary;
}

.fc-entry-icon--tunnel {
  color: $color-warning;
}

.fc-entry-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-entry-query {
  max-width: 45%;
  overflow: hidden;
  color: $color-text-tertiary;
  font-family: $font-mono;
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-method {
  overflow: hidden;
  font-family: $font-mono;
  font-size: 11.5px;
  font-weight: 700;
  letter-spacing: 0.02em;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-method--info {
  color: $color-method-get;
}

.fc-method--success {
  color: $color-method-post;
}

.fc-method--warning {
  color: $color-method-put;
}

.fc-method--danger {
  color: $color-method-delete;
}

.fc-method--muted {
  color: $color-method-connect;
}

.fc-status {
  font-family: $font-mono;
  font-size: 11px;
  font-weight: 600;
}

.fc-status--success {
  color: $color-success;
}

.fc-status--info {
  color: $color-info;
}

.fc-status--warning {
  color: $color-warning;
}

.fc-status--danger {
  color: $color-danger;
}

.fc-status--muted {
  color: $color-text-tertiary;
}

.fc-cell-mono {
  color: $color-text-secondary;
  font-family: $font-mono;
  font-size: 11px;
  white-space: nowrap;
}

.fc-time {
  color: $color-text-tertiary;
  font-size: 11.5px;
}

.fc-entry-url {
  display: flex;
  align-items: baseline;
  gap: 5px;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
}

.fc-entry-host {
  max-width: 48%;
  overflow: hidden;
  color: $color-text-secondary;
  font-weight: 550;
  text-overflow: ellipsis;
}

.fc-entry-path {
  overflow: hidden;
  color: $color-text-tertiary;
  font-family: $font-mono;
  font-size: 11px;
  text-overflow: ellipsis;
}

.fc-list-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 320px;
  height: 100%;
  padding: $spacing-xxl;
  text-align: center;
}

.fc-list-empty-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 52px;
  height: 52px;
  margin-bottom: $spacing-lg;
  border: 1px solid $color-border;
  border-radius: 16px;
  background: $color-surface-muted;
  color: $color-text-tertiary;
}

.fc-list-empty-title {
  color: $color-text-secondary;
  font-size: $font-size-md;
  font-weight: 600;
}

.fc-list-empty-text {
  max-width: 340px;
  margin-top: $spacing-sm;
  color: $color-text-tertiary;
  font-size: $font-size-sm;
  line-height: 1.7;
}

.fc-list-empty-code {
  padding: 1px 5px;
  border-radius: $radius-xs;
  background: $color-surface-muted;
  color: $color-text-secondary;
  font-family: $font-mono;
  font-size: 12.5px;
}

.fc-list-empty-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: $spacing-md;
  margin-top: $spacing-lg;
}

.fc-empty-action {
  padding: 4px 10px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text-secondary;
  font-size: $font-size-xs;
}

.fc-empty-action:hover {
  border-color: $color-border-strong;
  background: $color-surface-muted;
}
</style>
