import { ref } from 'vue'

export type ToastTone = 'info' | 'success' | 'error'

export interface ToastItem {
  id: number
  text: string
  tone: ToastTone
}

const items = ref<ToastItem[]>([])
let seed = 0

/** 全局轻提示，自动消失 */
export function useToast() {
  function dismiss(id: number): void {
    items.value = items.value.filter((item) => item.id !== id)
  }

  function push(text: string, tone: ToastTone = 'info'): void {
    const id = ++seed
    items.value = [...items.value, { id, text, tone }]
    window.setTimeout(() => dismiss(id), tone === 'error' ? 4600 : 2600)
  }

  return { items, push, dismiss }
}
