<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Activity, ArrowLeft, Cpu, Eye, Gauge, Radio, RefreshCw, ServerCog, Wifi, X, Zap, ChartNoAxesColumn } from 'lucide-vue-next'
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
const selectedMeterId = ref<number | null>(null)
const selectedMeter = computed(() => s.value?.meters.find(meter => meter.id === selectedMeterId.value) ?? null)
const deviceTab = ref<DeviceTab>('overview')
const detailPeriod = ref<DetailPeriod>('segment')
const eventLevelFilter = ref('')
const eventStatusFilter = ref('')
const eventKeyword = ref('')
const eventPage = ref(1)
const detailChartEl = ref<HTMLDivElement>()
const deviceHistory = ref<CollectSample[]>([])
const channels = ref<Rs485Channel[]>([])
const eventDetail = ref<GatewayEvent | null>(null)
const STALE_POINT_MS = 65_000
const eventLevelOptions: SideSelectOption[] = [
  { label: '全部级别', value: '' },
  { label: '提示', value: 'INFO' },
  { label: '警告', value: 'WARN' },
  { label: '严重', value: 'ERROR' },
]
const eventStatusOptions: SideSelectOption[] = [
  { label: '全部状态', value: '' },
  { label: '生效中', value: 'ACTIVE' },
  { label: '已恢复', value: 'RECOVERED' },
  { label: '已确认', value: 'ACKED' },
  { label: '日志', value: 'LOG' },
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
  { key: 'channel', label: '采集健康', icon: Radio },
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
  selectedMeterId.value = meter.id
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
  selectedMeterId.value = null
  eventDetail.value = null
  detailResizeObserver?.disconnect()
  detailResizeObserver = undefined
  detailChart?.dispose()
  detailChart = undefined
}

function valueText(value: number | null | undefined, unit = '') {
  if (value === null || value === undefined || Number.isNaN(Number(value))) return '--'
  return `${Number(value).toFixed(2)}${unit ? ` ${unit}` : ''}`
}

function pointStale(point: Meter['points'][number]) {
  const valueOk = point.value !== null && point.value !== undefined && !Number.isNaN(Number(point.value))
  const collectTime = Number(point.collectTime)
  if (!valueOk || !Number.isFinite(collectTime) || collectTime <= 0) return true
  return Date.now() - collectTime > STALE_POINT_MS
}

function displayPointValue(point: Meter['points'][number]) {
  if (pointStale(point)) return '--'
  return valueText(point.value, point.unit)
}

function displayCollectTime(point: Meter['points'][number]) {
  if (pointStale(point)) return '连续未采集'
  return formatCollectTime(point.collectTime)
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

function formatReadTime(value: string | null | undefined) {
  if (!value) return '未读取'
  const parsed = Number(value)
  if (Number.isFinite(parsed) && parsed > 0) return formatCollectTime(parsed)
  return value
}

function accessStateText(meter: Meter) {
  if (!meter.enabled) return '未加入采集'
  if (!selectedChannel.value) return '通道未配置'
  if (!selectedChannel.value.enabled) return '通道停用'
  return meter.online ? '链路在线' : '等待设备响应'
}

function accessStateTone(meter: Meter) {
  if (!meter.enabled || !selectedChannel.value || !selectedChannel.value.enabled) return 'danger'
  return meter.online ? 'success' : 'warn'
}

function shortDeviceName(name: string) {
  const compact = name.replace(/\s+/g, '')
  return compact || '现场设备'
}

function ensureDetailChart() {
  if (!detailChartEl.value) return
  if (detailChart && detailChart.getDom() !== detailChartEl.value) {
    detailResizeObserver?.disconnect()
    detailResizeObserver = undefined
    detailChart.dispose()
    detailChart = undefined
  }
  if (detailChart) return
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
  detailChart.resize()
}

function switchPeriod(period: DetailPeriod) {
  detailPeriod.value = period
  renderDetailChart()
}

function switchDeviceTab(tab: DeviceTab) {
  if (deviceTab.value === 'overview' && tab !== 'overview') {
    detailResizeObserver?.disconnect()
    detailResizeObserver = undefined
    detailChart?.dispose()
    detailChart = undefined
  }
  deviceTab.value = tab
  if (tab === 'overview') {
    nextTick(() => {
      ensureDetailChart()
      renderDetailChart()
    })
  }
}

function eventLevelText(level: string) {
  const map: Record<string, string> = { INFO: '提示', WARN: '警告', ERROR: '严重' }
  return map[level] ?? level
}

function eventStatusText(status: string) {
  const map: Record<string, string> = { ACTIVE: '生效中', RECOVERED: '已恢复', ACKED: '已确认', LOG: '日志' }
  return map[status] ?? status
}

function eventLevelTone(level: string) {
  if (level === 'ERROR') return 'danger'
  if (level === 'WARN') return 'warn'
  return 'info'
}

function eventStatusTone(status: string) {
  if (status === 'ACTIVE') return 'danger'
  if (status === 'RECOVERED') return 'success'
  if (status === 'ACKED') return 'warn'
  return 'muted'
}

function eventDate(time: string) {
  if (!time) return '--'
  const matched = time.match(/^(\d{4}-\d{2}-\d{2})/)
  return matched ? matched[1] : '--'
}

function eventTime(time: string) {
  if (!time) return '--'
  const matched = time.match(/(?:^|[T ])(\d{2}:\d{2}(?::\d{2})?)/)
  return matched ? matched[1] : '--'
}

function showEventDetail(event: GatewayEvent) {
  eventDetail.value = event
}

onMounted(async () => {
  await loadChannels()
})

watch([eventLevelFilter, eventStatusFilter, eventKeyword], () => { eventPage.value = 1 })
watch(selectedMeter, async meter => {
  if (!meter) return
  if (deviceTab.value === 'overview') {
    await nextTick()
    ensureDetailChart()
    renderDetailChart()
  }
}, { deep: true, immediate: true })
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
                <b>{{displayPointValue(point)}}</b>
                <span>{{qualityText(point.quality)}} · {{displayCollectTime(point)}}</span>
              </article>
              <div v-if="!selectedMeter.points.length" class="empty-row">暂无实时测点。</div>
            </section>

            <section v-else-if="deviceTab==='channel'" class="device-access-pane">
              <div class="health-strip">
                <article>
                  <small>链路状态</small>
                  <strong><b class="event-pill" :class="accessStateTone(selectedMeter)">{{accessStateText(selectedMeter)}}</b></strong>
                  <span>{{selectedMeter.enabled ? '已加入轮询队列' : '未启用采集'}}</span>
                </article>
                <article>
                  <small>最近采样</small>
                  <strong>{{formatReadTime(selectedMeter.lastRead)}}</strong>
                  <span>{{selectedMeter.points.length}} 个测点 · {{qualityText(selectedMeter.points[0]?.quality ?? 2)}}</span>
                </article>
                <article>
                  <small>上报队列</small>
                  <strong>{{s?.pending ?? 0}} 批</strong>
                  <span>{{s?.cloudOnline ? '平台链路正常' : '平台链路异常'}}</span>
                </article>
              </div>
              <dl class="access-config-list">
                <div><dt>绑定通道</dt><dd>{{selectedChannel?.name || selectedMeter.channelId}}</dd><small>{{selectedChannel?.port || '串口未读取'}} · {{selectedChannel?.enabled ? '通道启用' : '通道停用'}}</small></div>
                <div><dt>串口参数</dt><dd>{{selectedChannel ? `${selectedChannel.baud} bps · ${selectedChannel.dataBits}${selectedChannel.parity}/${selectedChannel.stopBits}` : '--'}}</dd><small>需与现场仪表保持一致</small></div>
                <div><dt>Modbus 从站</dt><dd>{{selectedMeter.address}}</dd><small>{{selectedMeter.channelId}} 下地址不可重复</small></div>
                <div><dt>采集策略</dt><dd>{{selectedMeter.collectIntervalS}} 秒 / {{selectedMeter.continuousPull ? '连续轮询' : '周期轮询'}}</dd><small>{{selectedMeter.points.length}} 个实时测点 · {{selectedMeter.profile}}</small></div>
                <div><dt>设备状态</dt><dd>{{deviceStatusText(selectedMeter)}}</dd><small>{{selectedMeter.online ? '最近采集有响应' : '等待新的采集响应'}}</small></div>
                <div><dt>上报策略</dt><dd>{{selectedMeter.uploadEnabled ? '允许上报平台' : '仅本地保留'}}</dd><small>{{selectedMeter.configSource === 'PLATFORM' ? '由平台同步配置' : '本地配置'}}</small></div>
                <div><dt>模型版本</dt><dd>{{selectedMeter.modelVersion || '--'}}</dd><small>{{selectedMeter.profile}}</small></div>
              </dl>
            </section>

            <section v-else class="device-event-pane">
              <div class="device-event-filters">
                <input v-model.trim="eventKeyword" placeholder="搜索时间、来源或内容">
                <SideSelect v-model="eventLevelFilter" :options="eventLevelOptions" />
                <SideSelect v-model="eventStatusFilter" :options="eventStatusOptions" />
              </div>
              <div class="device-detail-table event-table">
                <div class="device-detail-head event-detail-head"><span>时间</span><span>日期</span><span>级别</span><span>状态</span><span>内容</span><span>操作</span></div>
                <div v-for="event in pagedSelectedEvents" :key="event.id" class="device-detail-row event-detail-row">
                  <span>{{eventTime(event.time)}}</span>
                  <span>{{eventDate(event.time)}}</span>
                  <span><b class="event-pill" :class="eventLevelTone(event.level)">{{eventLevelText(event.level)}}</b></span>
                  <span><b class="event-pill" :class="eventStatusTone(event.status)">{{eventStatusText(event.status)}}</b></span>
                  <span class="event-message" :title="event.message">{{event.message}}</span>
                  <span><button class="icon-only event-detail-action" title="查看详情" @click="showEventDetail(event)"><Eye/></button></span>
                </div>
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

    <Teleport to="body">
      <div v-if="eventDetail" class="dialog-mask" @click.self="eventDetail=null">
        <section class="dialog event-detail-dialog">
          <header><Eye/><strong>告警事件详情</strong><button @click="eventDetail=null"><X/></button></header>
          <dl class="event-detail-grid">
            <div><dt>日期</dt><dd>{{eventDate(eventDetail.time)}}</dd></div>
            <div><dt>时间</dt><dd>{{eventTime(eventDetail.time)}}</dd></div>
            <div><dt>级别</dt><dd><b class="event-pill" :class="eventLevelTone(eventDetail.level)">{{eventLevelText(eventDetail.level)}}</b></dd></div>
            <div><dt>状态</dt><dd><b class="event-pill" :class="eventStatusTone(eventDetail.status)">{{eventStatusText(eventDetail.status)}}</b></dd></div>
            <div><dt>来源</dt><dd>{{eventDetail.source}}</dd></div>
            <div><dt>投递</dt><dd>{{eventDetail.delivery || '--'}}</dd></div>
          </dl>
          <section class="event-detail-block">
            <h3>内容</h3>
            <p>{{eventDetail.message}}</p>
          </section>
          <footer><button class="btn primary" @click="eventDetail=null">关闭</button></footer>
        </section>
      </div>
    </Teleport>

  </div>
</template>
