<script setup lang="ts">
import { onMounted, ref } from 'vue'
import PageHeader from '../../components/PageHeader.vue'
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
  show('导出完成', `已导出 ${rows.length} 条上报记录。`)
}

async function loadRecords() {
  loading.value = true
  try {
    records.value = await fetchUploadRecords(300)
  } catch (error) {
    records.value = store.snapshot?.uploads ?? []
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

onMounted(loadRecords)
</script>

<template>
  <div class="page report-page">
    <PageHeader title="数据上报记录"><button class="btn" :disabled="loading" @click="loadRecords">{{loading ? '刷新中' : '刷新记录'}}</button><button class="btn" @click="exportRecords">导出记录</button><button class="btn primary" :disabled="busy" @click="diagnoseLink">{{busy ? '诊断中' : '链路诊断'}}</button></PageHeader>
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
          <span><StatusBadge :ok="record.accessStatus === 'FORWARDED'" :text="record.accessStatus"/></span>
          <span><StatusBadge :ok="record.dataStatus === 'CONSUMED'" :text="record.dataStatus"/></span>
          <span>{{record.latency ? `${record.latency} ms` : '--'}}</span>
        </div>
        <div v-if="!records.length" class="empty-row">暂无上报批次。设备需要开启“允许上报”，且平台链路在线后才会产生记录。</div>
      </div>
    </article>
  </div>
</template>
