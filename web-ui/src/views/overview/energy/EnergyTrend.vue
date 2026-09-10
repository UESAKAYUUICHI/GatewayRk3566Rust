<script setup lang="ts">
import { computed } from 'vue'
import * as echarts from 'echarts/core'
import { DataZoomComponent, GridComponent, LegendComponent, MarkLineComponent, MarkPointComponent, TooltipComponent } from 'echarts/components'
import { BarChart, LineChart } from 'echarts/charts'
import { CanvasRenderer } from 'echarts/renderers'
import { commonChart, palette } from './chartTheme'
import { useEnergyChart } from './useEnergyChart'
import { useEnergyData } from './useEnergyData'
import EnergyDeviceSelect from './EnergyDeviceSelect.vue'
import EnergyEmpty from './EnergyEmpty.vue'

echarts.use([GridComponent, TooltipComponent, LegendComponent, DataZoomComponent, MarkLineComponent, MarkPointComponent, LineChart, BarChart, CanvasRenderer])

const energy = useEnergyData()
const x = computed(() => energy.devicePoints.value.map(p => p.time))
const y = computed(() => energy.devicePoints.value.map(p => Number(p.value.toFixed(2))))
const avg = computed(() => y.value.length ? Number((y.value.reduce((s, v) => s + v, 0) / y.value.length).toFixed(2)) : 0)
const { el } = useEnergyChart(() => ({
  ...commonChart,
  color: palette,
  legend: { top: 8, textStyle: { color: '#87a9c5' } },
  xAxis: { type: 'category', boundaryGap: false, data: x.value, axisLine: { lineStyle: { color: '#255878' } } },
  yAxis: { type: 'value', name: 'kW', splitLine: { lineStyle: { color: '#173b57' } } },
  dataZoom: [{ type: 'inside' }, { height: 18, bottom: 8 }],
  series: [
    { name: '有功功率', type: 'line', smooth: true, showSymbol: false, areaStyle: { opacity: .12 }, data: y.value, markPoint: { symbolSize: 46, data: [{ type: 'max', name: '峰值' }, { type: 'min', name: '低谷' }] } },
    { name: '平均功率', type: 'line', symbol: 'none', data: y.value.map(() => avg.value), lineStyle: { type: 'dashed', color: '#ffcc66' }, markLine: { data: [{ yAxis: avg.value, name: '平均' }] } },
  ],
}), [x, y])
void el
</script>

<template>
  <section class="energy-chart-shell">
    <article class="panel energy-chart-card">
      <div class="energy-chart-head"><h2>24 小时有功趋势</h2><EnergyDeviceSelect v-model="energy.selectedSn.value" :meters="energy.meters.value" /></div>
      <div v-if="energy.hasData.value" ref="el" class="energy-chart"></div>
      <EnergyEmpty v-else />
    </article>
  </section>
</template>
