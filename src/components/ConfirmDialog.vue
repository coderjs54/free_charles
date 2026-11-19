<script setup lang="ts">
import AppModal from './AppModal.vue'

withDefaults(
  defineProps<{
    title: string
    message: string
    confirmLabel?: string
    cancelLabel?: string
    danger?: boolean
  }>(),
  { confirmLabel: '确定', cancelLabel: '取消', danger: false }
)

const emit = defineEmits<{
  (event: 'confirm'): void
  (event: 'close'): void
}>()
</script>

<template>
  <AppModal :title="title" width="420px" @close="emit('close')">
    <p class="fc-confirm-message">{{ message }}</p>
    <template #footer>
      <button class="fc-button" type="button" @click="emit('close')">{{ cancelLabel }}</button>
      <button
        class="fc-button"
        :class="{ 'fc-button--danger': danger, 'fc-button--primary': !danger }"
        type="button"
        @click="emit('confirm')"
      >
        {{ confirmLabel }}
      </button>
    </template>
  </AppModal>
</template>

<style scoped lang="scss">
.fc-confirm-message {
  color: $color-text-secondary;
  font-size: $font-size-sm;
  line-height: 1.75;
}

.fc-button {
  height: 30px;
  padding: 0 14px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text-secondary;
  font-size: 12.5px;
  font-weight: 500;
}

.fc-button:hover {
  border-color: $color-border-strong;
  background: $color-surface-muted;
}

.fc-button--primary {
  border-color: $color-primary;
  background: $color-primary;
  color: #fff;
}

.fc-button--primary:hover {
  border-color: $color-primary-hover;
  background: $color-primary-hover;
}

.fc-button--danger {
  border-color: $color-danger;
  background: $color-danger;
  color: #fff;
}

.fc-button--danger:hover {
  border-color: $color-danger-strong;
  background: $color-danger-strong;
}
</style>
