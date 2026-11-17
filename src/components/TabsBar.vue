<script setup lang="ts">
import type { TabItem } from '../types/ui'

withDefaults(
  defineProps<{
    tabs: TabItem[]
    modelValue: string
    variant?: 'main' | 'sub'
    testidPrefix?: string
  }>(),
  { variant: 'main', testidPrefix: 'tab' }
)

const emit = defineEmits<{ (event: 'update:modelValue', value: string): void }>()
</script>

<template>
  <nav class="fc-tabs" :class="`fc-tabs--${variant}`">
    <button
      v-for="tab in tabs"
      :key="tab.key"
      class="fc-tab"
      :class="{ 'fc-tab--active': tab.key === modelValue }"
      :data-testid="`${testidPrefix}-${tab.key}`"
      type="button"
      @click="emit('update:modelValue', tab.key)"
    >
      <span class="fc-tab-label">{{ tab.label }}</span>
      <span v-if="tab.badge !== undefined && tab.badge !== ''" class="fc-tab-badge">{{ tab.badge }}</span>
    </button>
  </nav>
</template>

<style scoped lang="scss">
.fc-tabs {
  display: flex;
  align-items: stretch;
  gap: 2px;
  padding: 0 $spacing-xl;
  border-bottom: 1px solid $color-border;
  background: $color-surface;
  flex: none;
}

.fc-tabs--sub {
  padding: 0 $spacing-lg;
  background: $color-surface-muted;
  border-bottom-color: $color-divider;
}

.fc-tab {
  position: relative;
  display: flex;
  align-items: center;
  gap: $spacing-sm;
  height: 38px;
  padding: 0 $spacing-lg;
  color: $color-text-tertiary;
  font-size: $font-size-sm;
  font-weight: 500;
  border-radius: $radius-sm;
  transition: color $transition-fast, background $transition-fast;
}

.fc-tab:hover {
  color: $color-text-secondary;
  background: $color-hover;
}

.fc-tab--active {
  color: $color-primary;
}

.fc-tabs--main .fc-tab--active::after {
  content: '';
  position: absolute;
  right: $spacing-md;
  bottom: -1px;
  left: $spacing-md;
  height: 2px;
  border-radius: 2px 2px 0 0;
  background: $color-primary;
}

.fc-tabs--sub .fc-tab {
  height: 32px;
  font-size: $font-size-xs;
}

.fc-tabs--sub .fc-tab--active {
  background: $color-surface;
  box-shadow: $shadow-sm;
}

.fc-tab-label {
  white-space: nowrap;
}

.fc-tab-badge {
  min-width: 18px;
  padding: 0 5px;
  border-radius: $radius-pill;
  background: $color-primary-soft;
  color: $color-primary;
  font-family: $font-mono;
  font-size: 11px;
  line-height: 16px;
  text-align: center;
}
</style>
