import { reactive } from 'vue'

type ToastItem = { id: number; title: string; message: string; leaving: boolean }

const state = reactive<{ items: ToastItem[] }>({ items: [] })
let seed = 1

export function useToast() {
  const success = (title: string, message = '') => {
    const item: ToastItem = { id: seed++, title, message, leaving: false }
    state.items.push(item)
    window.setTimeout(() => {
      item.leaving = true
      window.setTimeout(() => {
        const index = state.items.findIndex(row => row.id === item.id)
        if (index >= 0) state.items.splice(index, 1)
      }, 260)
    }, 2500)
  }
  return { state, success }
}
