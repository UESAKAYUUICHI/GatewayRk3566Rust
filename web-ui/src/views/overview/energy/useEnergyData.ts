import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { fetchPower24h, type PowerPoint } from '../../../services/gateway-api'
import { useGatewayStore } from '../../../stores/gateway'

export function useEnergyData() {
  const store = useGatewayStore()
  const points = ref<PowerPoint[]>([])
  const selectedSn = ref('')
  const loading = ref(false)
  let timer: number | undefined

  const meters = computed(() => (store.snapshot?.meters ?? []).filter(m => m.channelId.startsWith('rs485-')))
  const selectedMeter = computed(() => meters.value.find(m => m.sn === selectedSn.value) ?? meters.value[0])
  const devicePoints = computed(() => points.value.filter(p => p.deviceSn === selectedMeter.value?.sn).sort((a, b) => a.timestampMs - b.timestampMs))
  const hasData = computed(() => devicePoints.value.length > 0)

  async function load() {
    loading.value = true
    try {
      points.value = await fetchPower24h()
    } finally {
      loading.value = false
    }
  }

  watch(meters, value => {
    if (!selectedSn.value && value[0]) selectedSn.value = value[0].sn
    if (selectedSn.value && !value.some(m => m.sn === selectedSn.value)) selectedSn.value = value[0]?.sn ?? ''
  }, { immediate: true })

  onMounted(() => {
    load().catch(() => { points.value = [] })
    timer = window.setInterval(() => load().catch(() => { points.value = [] }), 30_000)
  })
  onBeforeUnmount(() => { if (timer) window.clearInterval(timer) })

  return { meters, selectedSn, selectedMeter, devicePoints, hasData, loading, load }
}
