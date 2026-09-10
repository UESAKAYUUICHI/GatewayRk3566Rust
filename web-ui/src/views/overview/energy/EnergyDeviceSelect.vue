<script setup lang="ts">
import { computed } from 'vue'
import SideSelect from '../../../components/SideSelect.vue'
import type { Meter } from '../../../types/gateway'

const props = defineProps<{ meters: Meter[] }>()
const selectedSn = defineModel<string>({ required: true })
const meterOptions = computed(() => props.meters.map(meter => ({
  label: `${meter.name || meter.sn} · ${meter.channelId}`,
  value: meter.sn,
})))
</script>

<template>
  <label class="energy-device-select">
    <span>当前设备</span>
    <SideSelect v-model="selectedSn" :options="meterOptions" />
  </label>
</template>
