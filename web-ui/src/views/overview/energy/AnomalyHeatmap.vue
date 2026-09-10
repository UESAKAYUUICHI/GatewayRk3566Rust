<script setup lang="ts">
import { computed } from 'vue'
import * as echarts from 'echarts/core'
import { CalendarComponent, GridComponent, LegendComponent, TooltipComponent, VisualMapComponent } from 'echarts/components'
import { HeatmapChart, ScatterChart } from 'echarts/charts'
import { CanvasRenderer } from 'echarts/renderers'
import { useEnergyChart } from './useEnergyChart'
import { useEnergyData } from './useEnergyData'
import EnergyDeviceSelect from './EnergyDeviceSelect.vue'
import EnergyEmpty from './EnergyEmpty.vue'

echarts.use([GridComponent, TooltipComponent, LegendComponent, VisualMapComponent, CalendarComponent, HeatmapChart, ScatterChart, CanvasRenderer])

const energy = useEnergyData()
const rows = computed(() => ['00-06', '06-12', '12-18', '18-24'])
const cols = computed(() => Array.from({ length: 6 }, (_, i) => `${String(i * 4).padStart(2, '0')}:00`))
const data = computed(() => {
  const values = energy.devicePoints.value.map(p => p.value)
  const avg = values.length ? values.reduce((s, v) => s + v, 0) / values.length : 0
  const peak = Math.max(...values, avg, 0.01)
  return rows.value.flatMap((_, row) => cols.value.map((__, col) => {
    const index = row * cols.value.length + col
    const value = values[index % Math.max(values.length, 1)] ?? 0
    const heat = Math.min(100, Math.round(Math.abs(value - avg) / peak * 100))
    return [col, row, heat]
  }))
})
const { el } = useEnergyChart(() => ({
  backgroundColor: 'transparent',
  tooltip: { position: 'top', backgroundColor: 'rgba(7,24,39,.96)', borderColor: '#255878', textStyle: { color: '#d9ecff' } },
  grid: { left: 92, right: 30, top: 66, bottom: 62 },
  xAxis: { type: 'category', data: cols.value, splitArea: { show: true }, axisLabel: { color: '#87a9c5' } },
  yAxis: { type: 'category', data: rows.value, splitArea: { show: true }, axisLabel: { color: '#d9ecff' } },
  visualMap: { min: 0, max: 100, calculable: true, orient: 'horizontal', left: 'center', bottom: 12, textStyle: { color: '#87a9c5' }, inRange: { color: ['#0d2944', '#1ed5dc', '#ffcc66', '#ff667d'] } },
  series: [
    { name: '负荷波动热度', type: 'heatmap', data: data.value, label: { show: false }, emphasis: { itemStyle: { borderColor: '#fff', borderWidth: 1 } } },
    { name: '离线标记', type: 'scatter', symbolSize: 16, data: energy.selectedMeter.value?.online ? [] : [[cols.value.length - 1, 0, 100]] },
  ],
}), [data])
void el
</script>

<template>
  <section class="energy-chart-shell">
    <article class="panel energy-chart-card">
      <div class="energy-chart-head"><h2>异常热力</h2><EnergyDeviceSelect v-model="energy.selectedSn.value" :meters="energy.meters.value" /></div>
      <div v-if="energy.hasData.value" ref="el" class="energy-chart"></div>
      <EnergyEmpty v-else />
    </article>
  </section>
</template>
