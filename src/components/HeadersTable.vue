<script setup lang="ts">
import type { HeaderPair } from '../types/models'

withDefaults(defineProps<{ headers: HeaderPair[]; emptyText?: string }>(), {
  emptyText: '没有请求头'
})
</script>

<template>
  <div class="fc-headers">
    <div v-if="headers.length > 0" class="fc-headers-table">
      <div class="fc-headers-row fc-headers-row--head">
        <span class="fc-header-cell">Name</span>
        <span class="fc-header-cell">Value</span>
      </div>
      <div v-for="(pair, index) in headers" :key="`${pair[0]}-${index}`" class="fc-headers-row">
        <span class="fc-header-name">{{ pair[0] }}</span>
        <span class="fc-header-value">{{ pair[1] }}</span>
      </div>
    </div>
    <div v-else class="fc-headers-empty">{{ emptyText }}</div>
  </div>
</template>

<style scoped lang="scss">
.fc-headers {
  min-height: 100%;
}

.fc-headers-row {
  display: grid;
  grid-template-columns: minmax(140px, 28%) minmax(0, 1fr);
  gap: $spacing-lg;
  align-items: start;
  padding: 6px $spacing-xl;
  border-bottom: 1px solid $color-divider;
}

.fc-headers-row--head {
  position: sticky;
  top: 0;
  z-index: 1;
  padding-top: 5px;
  padding-bottom: 5px;
  background: $color-surface-muted;
  color: $color-text-tertiary;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  border-bottom-color: $color-border;
}

.fc-headers-row:not(.fc-headers-row--head):hover {
  background: $color-surface-muted;
}

.fc-header-cell {
  overflow: hidden;
  white-space: nowrap;
}

.fc-header-name {
  color: $color-text-secondary;
  font-family: $font-mono;
  font-size: 12px;
  font-weight: 600;
  word-break: break-all;
}

.fc-header-value {
  color: $color-text;
  font-family: $font-mono;
  font-size: 12px;
  line-height: 1.55;
  overflow-wrap: anywhere;
  word-break: break-word;
}

.fc-headers-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 160px;
  color: $color-text-tertiary;
  font-size: $font-size-sm;
}
</style>
