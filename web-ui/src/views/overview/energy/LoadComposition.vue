<script setup lang="ts">
import { computed } from 'vue'
import * as echarts from 'echarts/core'
import { GridComponent, LegendComponent, TooltipComponent } from 'echarts/components'
import { GaugeChart, PieChart, RadarChart } from 'echarts/charts'
import { CanvasRenderer } from 'echarts/renderers'
import { palette } from './chartTheme'
import { useEnergyChart } from './useEnergyChart'
import { useEnergyData } from './useEnergyData'
import EnergyDeviceSelect from './EnergyDeviceSelect.vue'
import EnergyEmpty from './EnergyEmpty.vue'

echarts.use([GridComponent, TooltipComponent, LegendComponent, PieChart, RadarChart, GaugeChart, CanvasRenderer])

const energy = useEnergyData()
const values = computed(() => energy.devicePoints.value.map(p => Math.max(0, p.value)))
const latest = computed(() => values.value.at(-1) ?? 0)
const peak = computed(() => Math.max(...values.value, 0))
const avg = computed(() => values.value.length ? values.value.reduce((s, v) => s + v, 0) / values.value.length : 0)
const buckets = computed(() => {
  const rows = [{ name: '低负荷', value: 0 }, { name: '平稳负荷', value: 0 }, { name: '高负荷', value: 0 }]
  const max = Math.max(peak.value, 0.01)
  values.value.forEach(v => rows[v > max * .72 ? 2 : v > max * .38 ? 1 : 0].value++)
  return rows.filter(row => row.value > 0)
})
const { el } = useEnergyChart(() => ({
  backgroundColor: 'transparent',
  color: palette,
  tooltip: { trigger: 'item', backgroundColor: 'rgba(7,24,39,.96)', borderColor: '#255878', textStyle: { color: '#d9ecff' } },
  legend: { top: 6, left: 8, textStyle: { color: '#87a9c5' } },
  radar: {
    center: ['73%', '56%'],
    radius: '54%',
    indicator: [
      { name: '当前', max: Math.max(1, peak.value * 1.2) },
      { name: '峰值', max: Math.max(1, peak.value * 1.2) },
      { name: '平均', max: Math.max(1, peak.value * 1.2) },
      { name: '样本', max: Math.max(1, values.value.length) },
      { name: '在线', max: 1 },
    ],
    axisName: { color: '#87a9c5' },
    splitLine: { lineStyle: { color: '#173b57' } },
    splitArea: { areaStyle: { color: ['rgba(30,213,220,.04)', 'rgba(47,128,255,.02)'] } },
  },
  series: [
    { name: '负荷时段占比', type: 'pie', radius: ['34%', '58%'], center: ['29%', '56%'], roseType: 'radius', label: { color: '#d9ecff', formatter: '{b}\\n{d}%' }, data: buckets.value },
    { name: '负荷画像', type: 'radar', areaStyle: { opacity: .18 }, data: [{ name: '当前设备', value: [latest.value, peak.value, avg.value, values.value.length, energy.selectedMeter.value?.online ? 1 : 0] }] },
  ],
}), [values])
void el
</script>

<template>
  <section class="energy-chart-shell">
    <article class="panel energy-chart-card">
      <div class="energy-chart-head"><h2>负荷构成</h2><EnergyDeviceSelect v-model="energy.selectedSn.value" :meters="energy.meters.value" /></div>
      <div v-if="energy.hasData.value" ref="el" class="energy-chart"></div>
      <EnergyEmpty v-else />
    </article>
  </section>
</template>
