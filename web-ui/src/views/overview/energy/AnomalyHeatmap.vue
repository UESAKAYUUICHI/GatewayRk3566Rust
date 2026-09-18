<script setup lang="ts">
import { computed } from 'vue'
import * as echarts from 'echarts/core'
import { GridComponent, LegendComponent, RadarComponent, TooltipComponent } from 'echarts/components'
import { BarChart, RadarChart } from 'echarts/charts'
import { CanvasRenderer } from 'echarts/renderers'
import { useEnergyChart } from './useEnergyChart'
import { useEnergyData } from './useEnergyData'
import EnergyDeviceSelect from './EnergyDeviceSelect.vue'
import EnergyEmpty from './EnergyEmpty.vue'

echarts.use([GridComponent, TooltipComponent, LegendComponent, RadarComponent, RadarChart, BarChart, CanvasRenderer])

const energy = useEnergyData()
const risk = computed(() => {
  const values = energy.devicePoints.value.map(p => p.value)
  const avg = values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : 0
  const max = Math.max(...values, avg, 0.01)
  const min = Math.min(...values, avg)
  const variance = values.length ? values.reduce((sum, value) => sum + Math.abs(value - avg), 0) / values.length : 0
  const spread = Math.min(100, Math.round((max - min) / Math.max(max, 0.01) * 100))
  const impact = Math.min(100, Math.round(variance / Math.max(avg, 0.01) * 130))
  const onlinePenalty = energy.selectedMeter.value?.online ? 0 : 80
  const loadPressure = Math.min(100, Math.round(max * 5))
  const stability = Math.max(0, 100 - Math.max(spread, impact))
  return [
    { name: '电压波动', value: spread },
    { name: '电流不平衡', value: Math.min(100, Math.round(spread * 0.72 + onlinePenalty * 0.2)) },
    { name: '负荷冲击', value: impact },
    { name: '负荷压力', value: loadPressure },
    { name: '通信连续性', value: onlinePenalty },
    { name: '运行稳定度', value: 100 - stability },
  ]
})
const { el } = useEnergyChart(() => ({
  backgroundColor: 'transparent',
  animation: false,
  color: ['#1ed5dc', '#ffcc66'],
  tooltip: { trigger: 'item', backgroundColor: 'rgba(7,24,39,.96)', borderColor: '#255878', textStyle: { color: '#d9ecff' } },
  legend: { top: 8, textStyle: { color: '#87a9c5' } },
  radar: {
    center: ['32%', '54%'],
    radius: '68%',
    splitNumber: 4,
    indicator: risk.value.map(item => ({ name: item.name, max: 100 })),
    axisName: { color: '#d9ecff', fontSize: 12 },
    axisLine: { lineStyle: { color: '#255878' } },
    splitLine: { lineStyle: { color: '#173b57' } },
    splitArea: { areaStyle: { color: ['rgba(30,213,220,.04)', 'rgba(255,255,255,.02)'] } },
  },
  grid: { left: '62%', right: 20, top: 62, bottom: 26 },
  xAxis: { type: 'value', max: 100, axisLabel: { color: '#87a9c5' }, splitLine: { lineStyle: { color: '#173b57' } } },
  yAxis: { type: 'category', data: risk.value.map(item => item.name), axisLabel: { color: '#d9ecff' } },
  series: [
    { name: '异常风险画像', type: 'radar', data: [{ value: risk.value.map(item => item.value), name: '风险分' }], areaStyle: { opacity: .22 }, lineStyle: { width: 3 } },
    { name: '分项风险', type: 'bar', data: risk.value.map(item => item.value), barWidth: 12, itemStyle: { borderRadius: 6 } },
  ],
}), [risk])
void el
</script>

<template>
  <section class="energy-chart-shell">
    <article class="panel energy-chart-card">
      <div class="energy-chart-head"><h2>异常风险画像</h2><EnergyDeviceSelect v-model="energy.selectedSn.value" :meters="energy.meters.value" /></div>
      <div v-if="energy.hasData.value" ref="el" class="energy-chart"></div>
      <EnergyEmpty v-else />
    </article>
  </section>
</template>
