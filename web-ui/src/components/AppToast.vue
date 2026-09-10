<script setup lang="ts">
import { CheckCircle2 } from 'lucide-vue-next'
import { useToast } from '../composables/useToast'

const { state } = useToast()
</script>

<template>
  <Teleport to="body">
    <div class="toast-stack">
      <div v-for="item in state.items" :key="item.id" class="toast" :class="{ leaving: item.leaving }">
        <CheckCircle2 />
        <div>
          <strong>{{ item.title }}</strong>
        </div>
      </div>
    </div>
  </Teleport>
</template>
