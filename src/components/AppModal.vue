<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, useId } from 'vue'
import AppIcon from './AppIcon.vue'

const props = withDefaults(
  defineProps<{
    title: string
    width?: string
    closeOnOverlay?: boolean
  }>(),
  { width: '560px', closeOnOverlay: true }
)

const emit = defineEmits<{ (event: 'close'): void }>()

const modalEl = ref<HTMLElement | null>(null)
const titleId = useId()
let previousActive: HTMLElement | null = null

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'

/** 取当前弹窗内可见、可聚焦的元素，用于 Tab 焦点循环 */
function getFocusable(): HTMLElement[] {
  if (!modalEl.value) return []
  return Array.from(modalEl.value.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (element) => element.getClientRects().length > 0
  )
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape') {
    emit('close')
    return
  }
  if (event.key !== 'Tab') return

  const focusable = getFocusable()
  if (focusable.length === 0) {
    event.preventDefault()
    modalEl.value?.focus()
    return
  }

  const first = focusable[0]
  const last = focusable[focusable.length - 1]
  const active = document.activeElement
  if (event.shiftKey && (active === first || !modalEl.value?.contains(active))) {
    event.preventDefault()
    last.focus()
  } else if (!event.shiftKey && (active === last || !modalEl.value?.contains(active))) {
    event.preventDefault()
    first.focus()
  }
}

function onOverlayDown(): void {
  if (props.closeOnOverlay) emit('close')
}

onMounted(() => {
  previousActive = document.activeElement instanceof HTMLElement ? document.activeElement : null
  document.addEventListener('keydown', onKeydown)
  // 子组件通常在自身 onMounted 里聚焦输入框，这里只处理没有初始焦点的情况
  void nextTick(() => {
    if (!modalEl.value?.contains(document.activeElement)) {
      const target = getFocusable()[0] ?? modalEl.value
      target?.focus()
    }
  })
})

onBeforeUnmount(() => {
  document.removeEventListener('keydown', onKeydown)
  if (previousActive?.isConnected) previousActive.focus()
})
</script>

<template>
  <Teleport to="body">
    <div class="fc-modal-overlay" @mousedown.self="onOverlayDown">
      <div
        ref="modalEl"
        class="fc-modal"
        :style="{ width }"
        role="dialog"
        aria-modal="true"
        :aria-labelledby="titleId"
        tabindex="-1"
      >
        <header class="fc-modal-head">
          <h2 :id="titleId" class="fc-modal-title">{{ title }}</h2>
          <button class="fc-modal-close" type="button" title="关闭" aria-label="关闭" @click="emit('close')">
            <AppIcon name="x" :size="15" />
          </button>
        </header>
        <div class="fc-modal-body">
          <slot />
        </div>
        <footer v-if="$slots.footer" class="fc-modal-foot">
          <slot name="footer" />
        </footer>
      </div>
    </div>
  </Teleport>
</template>

<style scoped lang="scss">
.fc-modal-overlay {
  position: fixed;
  inset: 0;
  z-index: $z-modal;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: $spacing-xxl;
  background: $color-modal-overlay;
  backdrop-filter: blur(2px);
  animation: fc-fade-in 120ms ease-out;
}

.fc-modal {
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 64px);
  display: flex;
  flex-direction: column;
  border-radius: $radius-lg;
  background: $color-surface;
  box-shadow: $shadow-lg;
  animation: fc-modal-in 160ms ease-out;
}

.fc-modal-head {
  display: flex;
  align-items: center;
  gap: $spacing-lg;
  padding: 18px $spacing-xxl 14px;
  border-bottom: 1px solid $color-divider;
}

.fc-modal-title {
  flex: 1;
  font-size: $font-size-lg;
  font-weight: 600;
  letter-spacing: -0.01em;
}

.fc-modal-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: $radius-sm;
  color: $color-text-tertiary;
  transition: background $transition-fast, color $transition-fast;
}

.fc-modal-close:hover {
  background: $color-hover;
  color: $color-text;
}

.fc-modal-body {
  flex: 1;
  overflow: auto;
  padding: $spacing-xxl;
}

.fc-modal-foot {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: $spacing-md;
  padding: 14px $spacing-xxl;
  border-top: 1px solid $color-divider;
  background: $color-surface-muted;
  border-radius: 0 0 $radius-lg $radius-lg;
}

@keyframes fc-fade-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

@keyframes fc-modal-in {
  from {
    opacity: 0;
    transform: translateY(8px) scale(0.985);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}
</style>
