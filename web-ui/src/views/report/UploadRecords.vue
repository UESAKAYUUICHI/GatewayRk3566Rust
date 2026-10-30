<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { RefreshCw } from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'
import SideSelect, { type SideSelectOption } from '../../components/SideSelect.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import { fetchUploadRecords, gatewayAction } from '../../services/gateway-api'
import { useGatewayStore } from '../../stores/gateway'
import { useDialog } from '../../composables/useDialog'
import type { UploadRecord } from '../../types/gateway'

const store = useGatewayStore()
const { show } = useDialog()
const busy = ref(false)
const loading = ref(false)
const records = ref<UploadRecord[]>([])
const page = ref(1)
const pageSize = ref(80)
const total = ref(0)
const pageSizeOptions: SideSelectOption[] = [
  { label: '50', value: 50 },
  { label: '80', value: 80 },
  { label: '120', value: 120 },
  { label: '200', value: 200 },
]
const pageCount = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)))

function csvCell(value: unknown) {
  return `"${String(value ?? '').replace(/"/g, '""')}"`
}

function exportRecords() {
  const rows = records.value
  const header = ['时间', '设备', '测点', '类型', '本地发送', '云端结果', '耗时']
  const csv = [
    header.map(csvCell).join(','),
    ...rows.map(record => [record.time, record.deviceSn, record.points, record.type, record.accessStatus, record.dataStatus, record.latency ? `${record.latency} ms` : '--'].map(csvCell).join(',')),
  ].join('\n')
  const blob = new Blob([`\uFEFF${csv}`], { type: 'text/csv;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `upload-records-${new Date().toISOString().slice(0, 10)}.csv`
  document.body.appendChild(link)
  link.click()
  link.remove()
  URL.revokeObjectURL(url)
  show('导出完成', `已导出当前页 ${rows.length} 条上报记录。`)
}

async function loadRecords() {
  loading.value = true
  try {
    const result = await fetchUploadRecords(page.value, pageSize.value)
    records.value = result.rows
    total.value = result.total
  } catch (error) {
    records.value = store.snapshot?.uploads ?? []
    total.value = records.value.length
    show('读取失败', error instanceof Error ? error.message : '上报记录读取失败', true)
  } finally {
    loading.value = false
  }
}

async function diagnoseLink() {
  busy.value = true
  try {
    await gatewayAction('/v1/platform/diagnose')
    await Promise.all([store.refresh(), loadRecords()])
    show('诊断已发起', '链路诊断命令已发送，结果会写入事件与指令记录。')
  } catch (error) {
    show('诊断失败', error instanceof Error ? error.message : '链路诊断失败', true)
  } finally {
    busy.value = false
  }
}

function accessText(value: string) {
  if (value === 'FORWARDED') return '已发送'
  if (value === 'PENDING') return '待发送'
  if (value === 'DEFERRED') return '暂存'
  return '失败'
}

function dataText(value: string) {
  if (value === 'CONSUMED') return '已消费'
  if (value === 'WAITING') return '等待'
  return value === 'FAILED' ? '失败' : value
}

watch(pageSize, () => { page.value = 1; loadRecords() })
watch(page, () => loadRecords())
watch(pageCount, value => { if (page.value > value) page.value = value })
onMounted(loadRecords)
</script>

<template>
  <div class="page report-page">
    <PageHeader title="数据上报记录">
      <label class="pager-size">显示<SideSelect v-model="pageSize" :options="pageSizeOptions" /></label>
      <button class="icon-only refresh-action" title="刷新记录" :disabled="loading" @click="loadRecords"><RefreshCw :class="{spin:loading}" /></button>
      <button class="btn" @click="exportRecords">导出当前页</button>
      <button class="btn primary" :disabled="busy" @click="diagnoseLink">{{busy ? '诊断中' : '链路诊断'}}</button>
    </PageHeader>
    <section class="metric-grid">
      <article class="metric"><span>待上报</span><strong>{{store.snapshot?.pending}} 条</strong><small>Outbox</small></article>
      <article class="metric"><span>累计发布</span><strong>{{store.snapshot?.sent}} 条</strong><small>MQTT PUBACK</small></article>
      <article class="metric"><span>MQTT 链路</span><strong>{{store.snapshot?.cloudOnline ? '在线' : '离线'}}</strong><small>{{store.snapshot?.mqttLabel}}</small></article>
      <article class="metric"><span>云端业务回执</span><strong>未接入</strong><small>--</small></article>
    </section>
    <article class="panel table-panel upload-record-panel">
      <div class="table seven fill-table upload-record-table">
        <div class="tr th"><span>时间</span><span>设备</span><span>测点</span><span>类型</span><span>本地发送</span><span>云端结果</span><span>耗时</span></div>
        <div v-for="record in records" :key="record.id" class="tr">
          <span>{{record.time}}</span><span>{{record.deviceSn}}</span><span>{{record.points}}</span><span>{{record.type}}</span>
          <span><StatusBadge :ok="record.accessStatus === 'FORWARDED'" :text="accessText(record.accessStatus)"/></span>
          <span><StatusBadge :ok="record.dataStatus === 'CONSUMED'" :text="dataText(record.dataStatus)"/></span>
          <span>{{record.latency ? `${record.latency} ms` : '--'}}</span>
        </div>
        <div v-if="!records.length" class="empty-row">暂无上报批次。设备需要开启“允许上报”，且平台链路在线后才会产生记录。</div>
      </div>
      <footer class="pagination">
        <span>共 {{ total }} 条 · 第 {{ page }} / {{ pageCount }} 页</span>
        <div>
          <button class="btn" :disabled="page<=1 || loading" @click="page--">上一页</button>
          <button class="btn" :disabled="page>=pageCount || loading" @click="page++">下一页</button>
        </div>
      </footer>
    </article>
  </div>
</template>
