<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { ContextMenuItem } from '../types/ui'
import AppIcon from './AppIcon.vue'

const props = defineProps<{
  open: boolean
  x: number
  y: number
  items: ContextMenuItem[]
}>()

const emit = defineEmits<{
  (event: 'select', id: string): void
  (event: 'close'): void
}>()

const menuEl = ref<HTMLElement | null>(null)
let previousActive: HTMLElement | null = null

const menuHeight = computed(() => props.items.reduce((sum, item) => sum + (item.separator ? 9 : 30), 12))
const style = computed(() => {
  const width = 220
  const margin = 8
  const left = Math.min(props.x, window.innerWidth - width - margin)
  const top = Math.min(props.y, window.innerHeight - menuHeight.value - margin)
  return { left: `${Math.max(margin, left)}px`, top: `${Math.max(margin, top)}px`, width: `${width}px` }
})

/** 打开时聚焦首个可用项，关闭时恢复原焦点 */
watch(
  () => props.open,
  (open) => {
    if (open) {
      previousActive = document.activeElement instanceof HTMLElement ? document.activeElement : null
      void nextTick(() => {
        menuEl.value?.querySelector<HTMLButtonElement>('button:not([disabled])')?.focus()
      })
    } else if (previousActive?.isConnected) {
      previousActive.focus()
    }
  }
)

function getMenuButtons(): HTMLButtonElement[] {
  return Array.from(menuEl.value?.querySelectorAll<HTMLButtonElement>('button:not([disabled])') ?? [])
}

/** 支持上下方向键与 Home / End 在菜单项之间移动 */
function onMenuKeydown(event: KeyboardEvent): void {
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
  const buttons = getMenuButtons()
  if (buttons.length === 0) return
  event.preventDefault()
  const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
  let next = 0
  if (event.key === 'ArrowDown') next = index < 0 ? 0 : (index + 1) % buttons.length
  else if (event.key === 'ArrowUp') next = index < 0 ? buttons.length - 1 : (index - 1 + buttons.length) % buttons.length
  else if (event.key === 'End') next = buttons.length - 1
  buttons[next]?.focus()
}

function onDocumentDown(event: MouseEvent): void {
  if (!props.open) return
  const target = event.target as HTMLElement | null
  if (target?.closest('.fc-context-menu')) return
  emit('close')
}

function onKeydown(event: KeyboardEvent): void {
  if (props.open && event.key === 'Escape') emit('close')
}

function onWindowResize(): void {
  if (props.open) emit('close')
}

function onWindowScroll(): void {
  if (props.open) emit('close')
}

function selectItem(item: ContextMenuItem): void {
  if (item.disabled || item.separator) return
  emit('select', item.id)
}

onMounted(() => {
  document.addEventListener('mousedown', onDocumentDown)
  document.addEventListener('keydown', onKeydown)
  window.addEventListener('resize', onWindowResize)
  window.addEventListener('scroll', onWindowScroll, true)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onDocumentDown)
  document.removeEventListener('keydown', onKeydown)
  window.removeEventListener('resize', onWindowResize)
  window.removeEventListener('scroll', onWindowScroll, true)
})
</script>

<template>
  <Teleport to="body">
    <div v-if="open" ref="menuEl" class="fc-context-menu" :style="style" role="menu" @keydown="onMenuKeydown">
      <template v-for="item in items" :key="item.id">
        <div v-if="item.separator" class="fc-context-separator" role="separator" />
        <button
          v-else
          class="fc-context-item"
          :class="{ 'fc-context-item--danger': item.danger, 'fc-context-item--disabled': item.disabled }"
          type="button"
          role="menuitem"
          :disabled="item.disabled"
          @click="selectItem(item)"
        >
          <span class="fc-context-icon">
            <AppIcon v-if="item.icon" :name="item.icon" :size="14" />
          </span>
          <span class="fc-context-label">{{ item.label }}</span>
          <span v-if="item.shortcut" class="fc-context-shortcut">{{ item.shortcut }}</span>
        </button>
      </template>
    </div>
  </Teleport>
</template>

<style scoped lang="scss">
.fc-context-menu {
  position: fixed;
  z-index: $z-context-menu;
  padding: 6px;
  border: 1px solid $color-border;
  border-radius: $radius-md;
  background: $color-surface;
  box-shadow: $shadow-md;
  animation: fc-context-in 90ms ease-out;
}

.fc-context-item {
  display: flex;
  align-items: center;
  gap: $spacing-md;
  width: 100%;
  height: 30px;
  padding: 0 $spacing-md;
  border-radius: $radius-sm;
  color: $color-text-secondary;
  font-size: $font-size-sm;
  text-align: left;
}

.fc-context-item:hover {
  background: $color-hover;
  color: $color-text;
}

.fc-context-item--danger {
  color: $color-danger;
}

.fc-context-item--danger:hover {
  background: $color-danger-soft;
  color: $color-danger;
}

.fc-context-item--disabled {
  color: $color-text-tertiary;
  opacity: 0.55;
  cursor: not-allowed;
}

.fc-context-item--disabled:hover {
  background: transparent;
  color: $color-text-tertiary;
}

.fc-context-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  color: currentColor;
}

.fc-context-label {
  flex: 1;
  white-space: nowrap;
}

.fc-context-shortcut {
  color: $color-text-tertiary;
  font-family: $font-mono;
  font-size: 11px;
}

.fc-context-separator {
  height: 1px;
  margin: 4px 6px;
  background: $color-divider;
}

@keyframes fc-context-in {
  from {
    opacity: 0;
    transform: translateY(-3px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
</style>
