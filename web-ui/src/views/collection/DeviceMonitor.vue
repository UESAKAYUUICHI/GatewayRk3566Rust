<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { AlertTriangle, CloudDownload, Eye, Play, Square, Trash2, X } from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import { useGatewayStore } from '../../stores/gateway'
import { deleteMeter, fetchChannels, setContinuousPull, syncPlatformConfig } from '../../services/gateway-api'
import type { Rs485Channel } from '../../types/gateway'
import { useDialog } from '../../composables/useDialog'
import { useToast } from '../../composables/useToast'

const store = useGatewayStore()
const selected = ref<number | null>(null)
const syncing = ref(false)
const syncLocked = ref(false)
const deleting = ref(false)
const deleteTarget = ref<number | null>(null)
const detailOpen = ref(false)
const detailTarget = ref<number | null>(null)
const pulling = ref(false)
const saving = ref(false)
const channels = ref<Rs485Channel[]>([])
const { show } = useDialog()
const toast = useToast()

const meter = computed(() => store.snapshot?.meters.find(m => m.id === (selected.value ?? store.snapshot?.meters[0]?.id)))
const detailMeter = computed(() => store.snapshot?.meters.find(m => m.id === detailTarget.value))
const grouped = computed(() => channels.value.map(channel => ({ channel, meters: (store.snapshot?.meters ?? []).filter(m => m.channelId === channel.id) })))
const unboundMeters = computed(() => (store.snapshot?.meters ?? []).filter(m => !channels.value.some(channel => channel.id === m.channelId)))

const displayPointValue = (value: number | null | undefined) => {
  if (value === null || value === undefined || Number.isNaN(value)) return '-'
  return value.toFixed(2)
}
const pointValueStyle = (value: number | null | undefined) => {
  const text = displayPointValue(value)
  const length = text.length
  if (text === '-') return { fontSize: 'clamp(32px,4vw,54px)' }
  if (length <= 5) return { fontSize: 'clamp(26px,3.1vw,42px)' }
  if (length <= 8) return { fontSize: 'clamp(22px,2.6vw,34px)' }
  return { fontSize: 'clamp(18px,2.1vw,28px)' }
}

async function loadMeta() {
  try {
    channels.value = await fetchChannels()
  } catch (error) {
    show('读取失败', error instanceof Error ? error.message : '无法读取通道', true)
  }
}

async function syncOnline() {
  if (syncLocked.value) return
  syncing.value = true
  syncLocked.value = true
  window.setTimeout(() => { syncLocked.value = false }, 3000)
  try {
    const result = await syncPlatformConfig()
    await Promise.all([store.refresh(), loadMeta()])
    toast.success(`线上同步完成：${result.devices} 台设备、${result.models} 个物模型`)
  } catch (error) {
    show('同步失败', error instanceof Error ? error.message : '平台配置同步失败', true)
  } finally {
    syncing.value = false
  }
}

function askDelete(id: number) {
  deleteTarget.value = id
  deleting.value = true
}

function showDetail(id: number) {
  detailTarget.value = id
  detailOpen.value = true
}

async function confirmDelete() {
  if (deleteTarget.value == null) return
  const id = deleteTarget.value
  saving.value = true
  try {
    await deleteMeter(id)
    deleting.value = false
    deleteTarget.value = null
    if (selected.value === id) selected.value = null
    await store.refresh()
    toast.success(`设备 ID ${id} 已删除，采集任务已停止`)
  } catch (error) {
    show('删除失败', error instanceof Error ? error.message : '设备无法删除', true)
  } finally {
    saving.value = false
  }
}

async function togglePull() {
  if (!meter.value || pulling.value) return
  pulling.value = true
  const enabled = !meter.value.continuousPull
  try {
    await setContinuousPull(meter.value.id, enabled)
    await store.refresh()
    toast.success(enabled ? `${meter.value.name} 已开始每 5 秒拉取` : `${meter.value.name} 已停止持续拉取`)
  } catch (error) {
    show('拉取切换失败', error instanceof Error ? error.message : '无法切换串口拉取状态', true)
  } finally {
    pulling.value = false
  }
}

onMounted(loadMeta)
</script>

<template>
  <div class="page device-page">
    <PageHeader title="设备与实时测点">
      <button class="btn" :disabled="syncing||syncLocked" @click="syncOnline"><CloudDownload/>{{syncing?'同步中':syncLocked?'请稍候':'线上同步'}}</button>
    </PageHeader>
    <section class="split-layout device-workspace">
      <article class="panel device-list">
        <template v-for="group in grouped" :key="group.channel.id">
          <div class="channel-label">
            <strong>{{group.channel.name}}</strong>
            <span class="channel-port">{{group.channel.port}}</span>
            <span class="channel-meter-count">{{group.meters.length?`${group.meters.length} 台设备 · 地址 ${group.meters.map(m=>m.address).join('/')}`:'空闲总线'}}</span>
          </div>
          <div v-if="!group.meters.length" class="empty-slot">云端暂未下发该通道设备</div>
          <button v-for="m in group.meters" :key="m.id" class="device-item" :class="{active:meter?.id===m.id}" @click="selected=m.id">
            <div><strong>{{m.sn}}</strong><StatusBadge :ok="m.online" :text="m.online?'在线':'离线'"/></div>
            <span class="icon-btn" title="查看详情" @click.stop="showDetail(m.id)"><Eye/></span>
            <span class="icon-btn danger" title="删除设备" @click.stop="askDelete(m.id)"><Trash2/></span>
          </button>
        </template>
        <template v-if="unboundMeters.length">
          <div class="channel-label staging-label"><strong>未匹配通道</strong><span>{{unboundMeters.length}} 台</span></div>
          <button v-for="m in unboundMeters" :key="m.id" class="device-item" :class="{active:meter?.id===m.id}" @click="selected=m.id">
            <div><strong>{{m.sn}}</strong><StatusBadge :ok="false" text="待平台配置"/></div>
            <span class="icon-btn" title="查看详情" @click.stop="showDetail(m.id)"><Eye/></span>
          </button>
        </template>
      </article>
      <article class="panel point-panel">
        <div class="panel-title"><div><h2>{{meter?.name??'请选择设备'}}</h2><p v-if="meter">{{meter.sn}} · {{meter.profile}} · {{meter.channelId}}</p></div><div class="panel-actions"><button v-if="meter?.enabled" class="btn" :class="{primary:!meter.continuousPull,danger:meter.continuousPull}" :disabled="pulling" @click="togglePull"><component :is="meter.continuousPull?Square:Play"/>{{meter.continuousPull?'停止拉取':'开始拉取'}}</button></div></div>
        <div v-if="meter" class="device-meta-strip"><span>物模型 {{meter.modelVersion||'local-v1'}}</span><span>{{meter.configSource==='PLATFORM'?'线上同步':'本地遗留'}}</span><span>{{meter.uploadEnabled?'允许上报':'暂不上报'}}</span><span>最近采集 {{meter.lastRead}}</span></div>
        <section v-if="meter?.points.length" class="point-grid dynamic-points"><div v-for="p in meter.points" :key="p.code" class="point-card"><span class="point-name">{{p.name}}</span><strong class="point-value" :style="pointValueStyle(p.value)">{{displayPointValue(p.value)}}</strong><small class="point-unit">{{p.unit}}</small></div></section>
        <div v-else class="empty-state"><span>◇</span><strong>暂无实时测点</strong></div>
      </article>
    </section>

    <Teleport to="body"><div v-if="deleting" class="dialog-mask" @click.self="deleting=false"><section class="dialog danger confirm-dialog"><header><AlertTriangle/><strong>确认删除设备</strong><button @click="deleting=false"><X/></button></header><p>删除后网关会停止该设备采集。平台如果仍下发该设备，下一次同步可能会重新出现。</p><footer><button class="btn" @click="deleting=false">取消</button><button class="btn danger" :disabled="saving" @click="confirmDelete">{{saving?'删除中':'确认删除'}}</button></footer></section></div></Teleport>
    <Teleport to="body"><div v-if="detailOpen" class="dialog-mask" @click.self="detailOpen=false"><section class="dialog detail-dialog"><header><Eye/><strong>设备详情</strong><button @click="detailOpen=false"><X/></button></header><dl v-if="detailMeter" class="detail-grid"><div><dt>设备名称</dt><dd>{{detailMeter.name}}</dd></div><div><dt>设备 SN</dt><dd>{{detailMeter.sn}}</dd></div><div><dt>绑定通道</dt><dd>{{detailMeter.channelId}}</dd></div><div><dt>Modbus 地址</dt><dd>{{detailMeter.address}}</dd></div><div><dt>物模型</dt><dd>{{detailMeter.profile}}</dd></div><div><dt>配置来源</dt><dd>{{detailMeter.configSource==='PLATFORM'?'线上同步':'本地遗留'}}</dd></div><div><dt>上报状态</dt><dd>{{detailMeter.uploadEnabled?'允许上报':'暂不上报'}}</dd></div><div><dt>采集周期</dt><dd>{{detailMeter.collectIntervalS}} 秒</dd></div><div><dt>最近采集</dt><dd>{{detailMeter.lastRead}}</dd></div><div><dt>在线状态</dt><dd>{{detailMeter.online?'在线':'离线'}}</dd></div></dl><footer><button class="btn primary" @click="detailOpen=false">知道了</button></footer></section></div></Teleport>
  </div>
</template>
