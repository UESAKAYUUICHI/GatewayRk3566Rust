<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ChevronRight } from 'lucide-vue-next'

export interface SideSelectOption {
  label: string
  value: string | number
  disabled?: boolean
}

const props = withDefaults(defineProps<{
  modelValue: string | number
  options: SideSelectOption[]
  placeholder?: string
  disabled?: boolean
}>(), {
  placeholder: '请选择',
  disabled: false,
})

const emit = defineEmits<{ 'update:modelValue': [value: string | number] }>()
const root = ref<HTMLElement>()
const open = ref(false)
const selected = computed(() => props.options.find(option => option.value === props.modelValue))

function toggle() {
  if (props.disabled) return
  open.value = !open.value
}

function choose(option: SideSelectOption) {
  if (option.disabled) return
  emit('update:modelValue', option.value)
  open.value = false
}

function closeOutside(event: MouseEvent) {
  if (!root.value?.contains(event.target as Node)) open.value = false
}

onMounted(() => document.addEventListener('mousedown', closeOutside))
onBeforeUnmount(() => document.removeEventListener('mousedown', closeOutside))
</script>

<template>
  <div ref="root" class="side-select" :class="{ open, disabled }">
    <button type="button" class="side-select-trigger" :disabled="disabled" @click="toggle">
      <span>{{ selected?.label || placeholder }}</span>
      <ChevronRight />
    </button>
    <div v-if="open" class="side-select-menu">
      <button
        v-for="option in options"
        :key="String(option.value)"
        type="button"
        :disabled="option.disabled"
        :class="{ active: option.value === modelValue }"
        @click="choose(option)"
      >
        {{ option.label }}
      </button>
    </div>
  </div>
</template>
