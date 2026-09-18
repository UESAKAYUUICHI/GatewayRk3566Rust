<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Activity, ArrowLeft, Cpu, Gauge, Radio, RefreshCw, ServerCog, Wifi, Zap, ChartNoAxesColumn } from 'lucide-vue-next'
import * as echarts from 'echarts/core'
import { GridComponent, TooltipComponent, type GridComponentOption, type TooltipComponentOption } from 'echarts/components'
import { LineChart, type LineSeriesOption } from 'echarts/charts'
import { CanvasRenderer } from 'echarts/renderers'
import PageHeader from '../../components/PageHeader.vue'
import SideSelect, { type SideSelectOption } from '../../components/SideSelect.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import { useGatewayStore } from '../../stores/gateway'
import { fetchChannels, fetchCollectHistory } from '../../services/gateway-api'
import type { CollectSample, GatewayEvent, Meter, Rs485Channel } from '../../types/gateway'

echarts.use([GridComponent, TooltipComponent, LineChart, CanvasRenderer])

type ChartOption = echarts.ComposeOption<GridComponentOption | TooltipComponentOption | LineSeriesOption>
type DeviceTab = 'overview' | 'points' | 'channel' | 'events'
type DetailPeriod = 'segment' | 'day' | 'week' | 'year'

const store = useGatewayStore()
const s = computed(() => store.snapshot)
const totalPower = computed(() => s.value?.meters.reduce((n, m) => n + m.power, 0).toFixed(2) ?? '0.00')
const selectedMeter = ref<Meter | null>(null)
const deviceTab = ref<DeviceTab>('overview')
const detailPeriod = ref<DetailPeriod>('segment')
const eventLevelFilter = ref('')
const eventStatusFilter = ref('')
const eventKeyword = ref('')
const eventPage = ref(1)
const detailChartEl = ref<HTMLDivElement>()
const deviceHistory = ref<CollectSample[]>([])
const channels = ref<Rs485Channel[]>([])
const eventLevelOptions: SideSelectOption[] = [
  { label: '全部级别', value: '' },
  { label: 'INFO', value: 'INFO' },
  { label: 'WARN', value: 'WARN' },
  { label: 'ERROR', value: 'ERROR' },
]
const eventStatusOptions: SideSelectOption[] = [
  { label: '全部状态', value: '' },
  { label: 'ACTIVE', value: 'ACTIVE' },
  { label: 'RECOVERED', value: 'RECOVERED' },
  { label: 'LOG', value: 'LOG' },
]
const activeEvents = computed(() => s.value?.events.filter(event => event.status === 'ACTIVE') ?? [])
const selectedEvents = computed(() => {
  const meter = selectedMeter.value
  if (!meter) return []
  return [...(s.value?.events ?? [])]
    .filter(event => eventMatchesMeter(event, meter))
    .filter(event => !eventLevelFilter.value || event.level === eventLevelFilter.value)
    .filter(event => !eventStatusFilter.value || event.status === eventStatusFilter.value)
    .filter(event => {
      const keyword = eventKeyword.value.trim().toLowerCase()
      if (!keyword) return true
      return [event.time, event.level, event.source, event.message, event.status].some(value => value.toLowerCase().includes(keyword))
    })
    .sort((a, b) => timeValue(b.time) - timeValue(a.time))
})
const pagedSelectedEvents = computed(() => selectedEvents.value.slice((eventPage.value - 1) * 6, eventPage.value * 6))
const eventPageCount = computed(() => Math.max(1, Math.ceil(selectedEvents.value.length / 6)))
const selectedChannel = computed<Rs485Channel | null>(() => {
  const meter = selectedMeter.value
  if (!meter) return null
  return channels.value.find(item => item.id === meter.channelId) ?? null
})
const deviceTabs: Array<{ key: DeviceTab; label: string; icon: typeof Gauge }> = [
  { key: 'overview', label: '运行概况', icon: Gauge },
  { key: 'points', label: '实时测点', icon: Zap },
  { key: 'channel', label: '接入配置', icon: Radio },
  { key: 'events', label: '告警事件', icon: Activity },
]
const detailPeriods: Array<{ key: DetailPeriod; label: string }> = [
  { key: 'segment', label: '时段' },
  { key: 'day', label: '日' },
  { key: 'week', label: '周' },
  { key: 'year', label: '年' },
]
let detailChart: echarts.ECharts | undefined
let detailResizeObserver: ResizeObserver | undefined

async function loadChannels() {
  try {
    channels.value = await fetchChannels()
  } catch {
    channels.value = []
  }
}

function eventMatchesMeter(event: GatewayEvent, meter: Meter) {
  return event.source.includes(meter.sn) || event.source.includes(meter.name) || event.message.includes(meter.sn) || event.message.includes(meter.name)
}

function timeValue(time: string) {
  const value = new Date(time).getTime()
  return Number.isFinite(value) ? value : 0
}

function deviceEvent(meter: Meter) {
  return activeEvents.value.find(event => eventMatchesMeter(event, meter))
}

function deviceStatusTone(meter: Meter) {
  const event = deviceEvent(meter)
  if (event?.level === 'ERROR') return 'fault'
  if (event?.level === 'WARN') return 'warn'
  return meter.online ? 'online' : 'offline'
}

function deviceStatusText(meter: Meter) {
  const tone = deviceStatusTone(meter)
  if (tone === 'fault') return '故障'
  if (tone === 'warn') return '警告'
  if (tone === 'online') return '在线'
  return '离线'
}

async function openDevice(meter: Meter) {
  selectedMeter.value = meter
  deviceTab.value = 'overview'
  detailPeriod.value = 'segment'
  eventPage.value = 1
  eventLevelFilter.value = ''
  eventStatusFilter.value = ''
  eventKeyword.value = ''
  await nextTick()
  ensureDetailChart()
  await loadDeviceHistory(meter)
  renderDetailChart()
}

function closeDevice() {
  selectedMeter.value = null
  detailResizeObserver?.disconnect()
  detailResizeObserver = undefined
  detailChart?.dispose()
  detailChart = undefined
}

function valueText(value: number | null | undefined, unit = '') {
  if (value === null || value === undefined || Number.isNaN(Number(value))) return '--'
  return `${Number(value).toFixed(2)}${unit ? ` ${unit}` : ''}`
}

function qualityText(quality: number) {
  if (quality === 0) return '有效'
  if (quality === 1) return '可疑'
  if (quality === 2) return '缺失'
  if (quality === 3) return '异常'
  return '待确认'
}

function formatCollectTime(value: number | string | null | undefined) {
  const timestamp = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(timestamp) || timestamp <= 0) return '未采集'
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return '未采集'
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

function shortDeviceName(name: string) {
  const compact = name.replace(/\s+/g, '')
  return compact || '现场设备'
}

function ensureDetailChart() {
  if (!detailChartEl.value || detailChart) return
  detailChart = echarts.init(detailChartEl.value)
  detailResizeObserver = new ResizeObserver(() => detailChart?.resize())
  detailResizeObserver.observe(detailChartEl.value)
}

async function loadDeviceHistory(meter: Meter) {
  try {
    const result = await fetchCollectHistory(1, 500)
    deviceHistory.value = result.rows.filter(row => row.deviceSn === meter.sn).sort((a, b) => a.timestampMs - b.timestampMs)
  } catch {
    deviceHistory.value = []
  }
}

function bucketLabel(timestampMs: number, period: DetailPeriod) {
  const date = new Date(timestampMs)
  if (period === 'segment') return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
  if (period === 'day') return `${String(date.getHours()).padStart(2, '0')}:00`
  if (period === 'week') return `${date.getMonth() + 1}/${date.getDate()}`
  return `${date.getFullYear()}/${date.getMonth() + 1}`
}

function periodStart(period: DetailPeriod) {
  const now = new Date()
  if (period === 'segment') return Date.now() - 24 * 60 * 60 * 1000
  if (period === 'day') return new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  if (period === 'week') return Date.now() - 7 * 24 * 60 * 60 * 1000
  return new Date(now.getFullYear() - 1, now.getMonth(), 1).getTime()
}

function renderDetailChart() {
  if (!detailChart) return
  const meter = selectedMeter.value
  const from = periodStart(detailPeriod.value)
  const rows = deviceHistory.value.filter(row => row.timestampMs >= from)
  const labels = [...new Set(rows.map(row => bucketLabel(row.timestampMs, detailPeriod.value)))]
  const pointDefs = meter?.points ?? []
  const series = pointDefs.map((point, index) => {
    const valuesByLabel = new Map<string, number[]>()
    for (const row of rows) {
      const found = row.points.find(item => item.code === point.code || item.name === point.name)
      if (!found || found.value === null || found.value === undefined || Number.isNaN(Number(found.value))) continue
      const label = bucketLabel(row.timestampMs, detailPeriod.value)
      valuesByLabel.set(label, [...(valuesByLabel.get(label) ?? []), Number(found.value)])
    }
    const data = labels.map(label => {
      const values = valuesByLabel.get(label) ?? []
      if (!values.length) return null
      return Number((values.reduce((sum, value) => sum + value, 0) / values.length).toFixed(3))
    })
    return {
      name: point.name || point.code,
      type: 'line' as const,
      smooth: true,
      showSymbol: labels.length <= 16,
      symbolSize: 7,
      lineStyle: { width: 3, color: ['#1ed5dc', '#6fe190', '#ffd15c', '#ff6b7a', '#8ab7ff'][index % 5] },
      itemStyle: { color: ['#1ed5dc', '#6fe190', '#ffd15c', '#ff6b7a', '#8ab7ff'][index % 5] },
      data,
    }
  }).filter(item => item.data.some(value => value !== null))
  const option: ChartOption = {
    backgroundColor: 'transparent',
    tooltip: {
      trigger: 'axis',
      backgroundColor: '#0b2238',
      borderColor: '#1d4c70',
      textStyle: { color: '#edf7ff' },
    },
    grid: { left: 48, right: 18, top: 26, bottom: 34 },
    xAxis: {
      type: 'category',
      data: labels,
      boundaryGap: false,
      axisLine: { lineStyle: { color: '#2a5878' } },
      axisLabel: { color: '#91abc2' },
    },
    yAxis: {
      type: 'value',
      splitLine: { lineStyle: { color: '#173b57' } },
      axisLabel: { color: '#91abc2' },
    },
    series,
  }
  detailChart.setOption(option, true)
}

function switchPeriod(period: DetailPeriod) {
  detailPeriod.value = period
  renderDetailChart()
}

function switchDeviceTab(tab: DeviceTab) {
  deviceTab.value = tab
  if (tab === 'overview') {
    nextTick(() => {
      ensureDetailChart()
      renderDetailChart()
    })
  }
}

onMounted(async () => {
  await loadChannels()
})

watch([eventLevelFilter, eventStatusFilter, eventKeyword], () => { eventPage.value = 1 })
onBeforeUnmount(() => { detailResizeObserver?.disconnect(); detailChart?.dispose() })
</script>

<template>
  <div class="page dashboard-page">
    <PageHeader title="网关运行总览">
      <button class="icon-only refresh-action" title="刷新状态" @click="store.refresh"><RefreshCw/></button>
    </PageHeader>

    <section class="metric-grid">
      <article class="metric"><Radio/><span>设备在线</span><strong>{{store.onlineMeters}} / {{s?.meters.length??0}}</strong></article>
      <article class="metric"><Activity/><span>当前总有功</span><strong>{{totalPower}} kW</strong></article>
      <article class="metric wifi-summary-metric">
        <span :class="['wifi-summary-icon', { online: s?.wifi.connected }]"><Wifi/></span>
        <div class="wifi-summary-main">
          <span>当前 Wi-Fi</span>
          <strong>{{s?.wifi.connected ? s.wifi.ssid : '未连接'}}</strong>
        </div>
      </article>
    </section>

    <section class="dashboard-grid">
      <article class="panel device-board-panel">
        <div class="panel-title">
          <div><h2>现场设备列表</h2></div>
          <StatusBadge :ok="!!s?.meters.length" :text="`${s?.meters.length??0} 台`"/>
        </div>
        <div class="device-card-grid">
          <button v-for="meter in s?.meters" :key="meter.id" class="gateway-device-card" @click="openDevice(meter)">
            <span class="device-card-visual">
              <span class="device-status-dot" :class="deviceStatusTone(meter)" :title="deviceStatusText(meter)"></span>
              <span class="device-illustration" aria-hidden="true">
                <span class="device-shadow"></span>
                <span class="device-side"></span>
                <span class="device-body">
                  <span class="device-screen"><Cpu /></span>
                  <span class="device-leds"><i></i><i></i><i></i></span>
                  <span class="device-terminal"><i></i><i></i><i></i><i></i></span>
                </span>
                <span class="device-port"></span>
              </span>
            </span>
            <span class="device-card-name">
              <strong>{{shortDeviceName(meter.name)}}</strong>
            </span>
          </button>
          <div v-if="!s?.meters.length" class="empty-row">暂无设备，请先完成 RS485 设备接入。</div>
        </div>
      </article>
    </section>

    <div v-if="selectedMeter" class="device-fullscreen-layer">
      <section class="device-detail-shell">
        <header class="device-detail-topbar">
          <button class="icon-only" title="返回" @click="closeDevice"><ArrowLeft /></button>
          <div>
            <h2>{{selectedMeter.name}}</h2>
            <p>{{selectedMeter.sn}} · {{selectedMeter.profile}} · 地址 {{selectedMeter.address}}</p>
          </div>
          <StatusBadge :ok="deviceStatusTone(selectedMeter)==='online'" :text="deviceStatusText(selectedMeter)" />
        </header>

        <div class="device-detail-layout">
          <aside class="device-detail-portrait">
            <div class="device-detail-icon">
              <span class="device-status-dot" :class="deviceStatusTone(selectedMeter)"></span>
              <span class="device-illustration big" aria-hidden="true">
                <span class="device-shadow"></span><span class="device-side"></span><span class="device-body"><span class="device-screen"><ServerCog /></span><span class="device-leds"><i></i><i></i><i></i></span><span class="device-terminal"><i></i><i></i><i></i><i></i></span></span><span class="device-port"></span>
              </span>
            </div>
            <dl class="device-detail-facts">
              <div><dt>设备名称</dt><dd>{{selectedMeter.name}}</dd></div>
              <div><dt>设备编号</dt><dd>{{selectedMeter.sn}}</dd></div>
              <div><dt>型号版本</dt><dd>{{selectedMeter.modelVersion || '--'}}</dd></div>
              <div><dt>配置来源</dt><dd>{{selectedMeter.configSource}}</dd></div>
              <div><dt>上报状态</dt><dd>{{selectedMeter.uploadEnabled ? '允许上报' : '本地采集'}}</dd></div>
            </dl>
          </aside>

          <article class="device-workspace-panel">
            <nav class="device-workspace-tabs">
              <button v-for="tab in deviceTabs" :key="tab.key" :class="{active:deviceTab===tab.key}" @click="switchDeviceTab(tab.key)"><component :is="tab.icon" />{{tab.label}}</button>
            </nav>

            <section v-if="deviceTab==='overview'" class="device-overview-pane">
              <div class="device-chart-toolbar">
                <div><h3>测点趋势</h3><small>默认显示全部有数据测点</small></div>
                <div class="device-period-tabs">
                  <button v-for="period in detailPeriods" :key="period.key" :class="{active:detailPeriod===period.key}" @click="switchPeriod(period.key)">{{period.label}}</button>
                </div>
              </div>
              <div class="device-detail-chart-wrap">
                <div ref="detailChartEl" class="device-detail-chart"></div>
                <div v-if="!deviceHistory.length" class="chart-empty"><ChartNoAxesColumn/></div>
              </div>
            </section>

            <section v-else-if="deviceTab==='points'" class="device-point-cards">
              <article v-for="point in selectedMeter.points" :key="point.code" class="device-point-card">
                <small>{{point.code}}</small>
                <strong>{{point.name || point.code}}</strong>
                <b>{{valueText(point.value, point.unit)}}</b>
                <span>{{qualityText(point.quality)}} · {{formatCollectTime(point.collectTime)}}</span>
              </article>
              <div v-if="!selectedMeter.points.length" class="empty-row">暂无实时测点。</div>
            </section>

            <section v-else-if="deviceTab==='channel'" class="device-access-pane">
              <div class="access-flow">
                <span>设备</span><i></i><span>RS485 总线</span><i></i><span>边缘采集</span><i></i><span>本地缓存/上报</span>
              </div>
              <dl class="access-config-list">
                <div><dt>绑定通道</dt><dd>{{selectedChannel?.name || selectedMeter.channelId}}</dd><small>{{selectedChannel?.port || '串口未读取'}} · {{selectedChannel?.enabled ? '通道启用' : '通道停用'}}</small></div>
                <div><dt>串口参数</dt><dd>{{selectedChannel ? `${selectedChannel.baud} bps · ${selectedChannel.dataBits}${selectedChannel.parity}/${selectedChannel.stopBits}` : '--'}}</dd><small>与现场仪表串口参数必须一致</small></div>
                <div><dt>Modbus 地址</dt><dd>{{selectedMeter.address}}</dd><small>同一 RS485 通道下地址不可重复</small></div>
                <div><dt>采集策略</dt><dd>{{selectedMeter.collectIntervalS}} 秒 / {{selectedMeter.continuousPull ? '连续轮询' : '周期轮询'}}</dd><small>{{selectedMeter.enabled ? '设备已加入采集队列' : '设备未启用采集'}}</small></div>
                <div><dt>物模型</dt><dd>{{selectedMeter.profile}}</dd><small>{{selectedMeter.modelVersion || '本地版本'}} · {{selectedMeter.points.length}} 个实时测点</small></div>
                <div><dt>上报链路</dt><dd>{{selectedMeter.uploadEnabled ? '允许上报平台' : '仅本地保留'}}</dd><small>{{s?.cloudOnline ? 'MQTT 链路正常' : '平台链路异常'}}</small></div>
              </dl>
            </section>

            <section v-else class="device-event-pane">
              <div class="device-event-filters">
                <input v-model.trim="eventKeyword" placeholder="搜索时间、来源或内容">
                <SideSelect v-model="eventLevelFilter" :options="eventLevelOptions" />
                <SideSelect v-model="eventStatusFilter" :options="eventStatusOptions" />
              </div>
              <div class="device-detail-table event-table">
                <div class="device-detail-head"><span>时间</span><span>级别</span><span>状态</span><span>内容</span></div>
                <div v-for="event in pagedSelectedEvents" :key="event.id" class="device-detail-row"><span>{{event.time}}</span><span>{{event.level}}</span><span>{{event.status}}</span><span>{{event.message}}</span></div>
                <div v-if="!pagedSelectedEvents.length" class="empty-row">暂无符合条件的事件。</div>
              </div>
              <footer class="event-pagination">
                <span>共 {{selectedEvents.length}} 条</span>
                <button class="btn" :disabled="eventPage<=1" @click="eventPage--">上一页</button>
                <strong>{{eventPage}} / {{eventPageCount}}</strong>
                <button class="btn" :disabled="eventPage>=eventPageCount" @click="eventPage++">下一页</button>
              </footer>
            </section>
          </article>
        </div>
      </section>
    </div>

  </div>
</template>
