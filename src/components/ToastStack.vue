<script setup lang="ts">
import { useToast } from '../stores/useToast'
import AppIcon from './AppIcon.vue'

const { items, dismiss } = useToast()
</script>

<template>
  <Teleport to="body">
    <div class="fc-toast-stack">
      <TransitionGroup name="fc-toast">
        <div
          v-for="item in items"
          :key="item.id"
          class="fc-toast"
          :class="`fc-toast--${item.tone}`"
          @click="dismiss(item.id)"
        >
          <span class="fc-toast-icon">
            <AppIcon :name="item.tone === 'error' ? 'alert' : item.tone === 'success' ? 'check' : 'info'" :size="14" />
          </span>
          <span class="fc-toast-text">{{ item.text }}</span>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<style scoped lang="scss">
.fc-toast-stack {
  position: fixed;
  right: 18px;
  bottom: calc(#{$statusbar-height} + 16px);
  z-index: $z-toast;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: $spacing-md;
  pointer-events: none;
}

.fc-toast {
  display: flex;
  align-items: flex-start;
  gap: $spacing-md;
  max-width: 460px;
  padding: 9px 14px;
  border: 1px solid $color-border;
  border-radius: $radius-md;
  background: $color-surface;
  box-shadow: $shadow-md;
  color: $color-text-secondary;
  font-size: $font-size-sm;
  line-height: 1.5;
  pointer-events: auto;
  cursor: default;
}

.fc-toast-icon {
  margin-top: 2px;
  color: $color-info;
}

.fc-toast--success .fc-toast-icon {
  color: $color-success;
}

.fc-toast--error .fc-toast-icon {
  color: $color-danger;
}

.fc-toast--error {
  border-color: rgba(192, 43, 43, 0.28);
}

.fc-toast-text {
  word-break: break-word;
}

.fc-toast-enter-active,
.fc-toast-leave-active {
  transition: opacity 160ms ease, transform 160ms ease;
}

.fc-toast-enter-from,
.fc-toast-leave-to {
  opacity: 0;
  transform: translateY(6px);
}
</style>
