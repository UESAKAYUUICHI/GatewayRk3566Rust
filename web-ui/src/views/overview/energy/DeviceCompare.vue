<script setup lang="ts">
import { computed } from 'vue'
import * as echarts from 'echarts/core'
import { GridComponent, LegendComponent, TooltipComponent } from 'echarts/components'
import { BarChart, CustomChart, PictorialBarChart } from 'echarts/charts'
import { CanvasRenderer } from 'echarts/renderers'
import { commonChart, palette } from './chartTheme'
import { useEnergyChart } from './useEnergyChart'
import { useEnergyData } from './useEnergyData'
import EnergyDeviceSelect from './EnergyDeviceSelect.vue'
import EnergyEmpty from './EnergyEmpty.vue'

echarts.use([GridComponent, TooltipComponent, LegendComponent, BarChart, PictorialBarChart, CustomChart, CanvasRenderer])

const energy = useEnergyData()
const grouped = computed(() => {
  const map = new Map<string, number[]>()
  energy.devicePoints.value.forEach(point => {
    const hour = point.time.slice(0, 2) + ':00'
    map.set(hour, [...(map.get(hour) ?? []), point.value])
  })
  return [...map.entries()].map(([hour, rows]) => ({ hour, max: Math.max(...rows), avg: rows.reduce((s, v) => s + v, 0) / rows.length, min: Math.min(...rows) }))
})
const { el } = useEnergyChart(() => ({
  ...commonChart,
  color: palette,
  legend: { top: 8, textStyle: { color: '#87a9c5' } },
  grid: { left: 68, right: 34, top: 58, bottom: 42 },
  xAxis: { type: 'category', data: grouped.value.map(row => row.hour), axisLabel: { color: '#87a9c5' }, axisLine: { lineStyle: { color: '#255878' } } },
  yAxis: { type: 'value', name: 'kW', splitLine: { lineStyle: { color: '#173b57' } } },
  series: [
    { name: '峰值功率', type: 'bar', barWidth: 16, data: grouped.value.map(row => Number(row.max.toFixed(2))) },
    { name: '平均功率', type: 'bar', barWidth: 16, data: grouped.value.map(row => Number(row.avg.toFixed(2))) },
    { name: '低谷标记', type: 'pictorialBar', symbol: 'roundRect', symbolRepeat: true, symbolSize: [8, 14], symbolMargin: 3, data: grouped.value.map(row => Math.max(.2, row.min)), itemStyle: { color: '#35d7a6' } },
  ],
}), [grouped])
void el
</script>

<template>
  <section class="energy-chart-shell">
    <article class="panel energy-chart-card">
      <div class="energy-chart-head"><h2>分时对比</h2><EnergyDeviceSelect v-model="energy.selectedSn.value" :meters="energy.meters.value" /></div>
      <div v-if="energy.hasData.value" ref="el" class="energy-chart"></div>
      <EnergyEmpty v-else />
    </article>
  </section>
</template>
