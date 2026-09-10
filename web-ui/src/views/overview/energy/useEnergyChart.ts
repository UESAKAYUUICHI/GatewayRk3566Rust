import { nextTick, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue'
import * as echarts from 'echarts/core'

export function useEnergyChart(optionFactory: () => echarts.EChartsCoreOption, deps: Ref<unknown>[]) {
  const el = ref<HTMLDivElement>()
  let chart: echarts.ECharts | undefined
  let resizeObserver: ResizeObserver | undefined
  const render = () => chart?.setOption(optionFactory(), true)
  const ensure = () => {
    if (!el.value || chart) return
    chart = echarts.init(el.value)
    resizeObserver = new ResizeObserver(() => chart?.resize())
    resizeObserver.observe(el.value)
    render()
  }

  onMounted(async () => {
    await nextTick()
    ensure()
  })
  watch(el, () => nextTick().then(ensure))
  deps.forEach(dep => watch(dep, render, { deep: true }))
  onBeforeUnmount(() => { resizeObserver?.disconnect(); chart?.dispose() })
  return { el }
}
