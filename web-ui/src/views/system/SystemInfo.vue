<script setup lang="ts">
import { onMounted, ref } from 'vue'
import PageHeader from '../../components/PageHeader.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import { fetchSystemDiagnostics, fetchSystemEnvironment, gatewayAction } from '../../services/gateway-api'
import { useGatewayStore } from '../../stores/gateway'
import type { SystemEnvironment } from '../../types/gateway'
import { useDialog } from '../../composables/useDialog'

const store = useGatewayStore()
const { show } = useDialog()
const environment = ref<SystemEnvironment | null>(null)
const envLoading = ref(false)
const actionBusy = ref(false)
const envError = ref('')
type PowerAction = 'shutdown' | 'reboot'
const pendingPowerAction = ref<PowerAction | null>(null)
const powerConfirmStep = ref(0)
const powerBusy = ref(false)
const powerActionText: Record<PowerAction, { title: string; api: string; running: string }> = {
  shutdown: { title: '关机', api: '/v1/system/shutdown-machine', running: '关机命令已发送，开发板会关闭电源。' },
  reboot: { title: '重启机器', api: '/v1/system/reboot-machine', running: '重启命令已发送，开发板会重新启动。' },
}

async function loadEnvironment() {
  envLoading.value = true
  envError.value = ''
  try {
    environment.value = await fetchSystemEnvironment()
  } catch (error) {
    envError.value = error instanceof Error ? error.message : '环境信息读取失败'
  } finally {
    envLoading.value = false
  }
}

async function exportDiagnostics() {
  actionBusy.value = true
  try {
    const blob = await fetchSystemDiagnostics()
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = `park-gateway-diagnostics-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-')}.txt`
    document.body.appendChild(link)
    link.click()
    link.remove()
    URL.revokeObjectURL(url)
    show('诊断包已生成', '系统诊断信息已导出。')
  } catch (error) {
    show('导出失败', error instanceof Error ? error.message : '诊断包导出失败', true)
  } finally {
    actionBusy.value = false
  }
}

async function restartService() {
  actionBusy.value = true
  try {
    await gatewayAction('/v1/system/restart-service')
    show('服务正在重启', '网关服务会短暂断开并自动恢复。')
    window.setTimeout(() => {
      store.refresh()
      loadEnvironment()
      actionBusy.value = false
    }, 3500)
  } catch (error) {
    actionBusy.value = false
    show('重启失败', error instanceof Error ? error.message : '服务重启失败', true)
  }
}

function openPowerConfirm(action: PowerAction) {
  pendingPowerAction.value = action
  powerConfirmStep.value = 1
}

function closePowerConfirm() {
  if (powerBusy.value) return
  pendingPowerAction.value = null
  powerConfirmStep.value = 0
}

async function confirmPowerAction() {
  const action = pendingPowerAction.value
  if (!action) return
  if (powerConfirmStep.value === 1) {
    powerConfirmStep.value = 2
    return
  }
  powerBusy.value = true
  actionBusy.value = true
  try {
    await gatewayAction(powerActionText[action].api)
    show(powerActionText[action].title, powerActionText[action].running, action === 'shutdown')
    pendingPowerAction.value = null
    powerConfirmStep.value = 0
  } catch (error) {
    show(`${powerActionText[action].title}失败`, error instanceof Error ? error.message : '系统命令执行失败', true)
  } finally {
    powerBusy.value = false
    actionBusy.value = false
  }
}

onMounted(loadEnvironment)
</script>

<template>
  <div class="page system-info-page">
    <PageHeader title="系统信息"><button class="btn" :disabled="envLoading" @click="loadEnvironment">{{envLoading ? '刷新中...' : '刷新环境'}}</button><button class="btn" :disabled="actionBusy" @click="exportDiagnostics">导出诊断包</button><button class="btn danger" :disabled="actionBusy" @click="restartService">重启服务</button><button class="btn danger" :disabled="actionBusy" @click="openPowerConfirm('shutdown')">关机</button><button class="btn danger power-danger" :disabled="actionBusy" @click="openPowerConfirm('reboot')">重启机器</button></PageHeader>
    <section class="system-grid">
      <article class="panel info-panel"><h2>基础信息</h2><dl><div><dt>网关 SN</dt><dd>{{store.snapshot?.gatewaySn}}</dd></div><div><dt>软件版本</dt><dd>v{{store.snapshot?.version}}</dd></div><div><dt>运行时长</dt><dd>{{store.snapshot?.uptime}}</dd></div><div><dt>系统时钟</dt><dd><StatusBadge :ok="store.snapshot?.clockTrusted" :text="store.snapshot?.clockTrusted ? '可信' : '未同步'"/></dd></div></dl></article>
      <article class="panel info-panel"><h2>资源状态</h2><dl><div><dt>CPU</dt><dd>{{environment ? `${environment.cpuCores} 核` : '--'}}</dd></div><div><dt>负载</dt><dd>{{environment?.loadAverage || '--'}}</dd></div><div><dt>内核</dt><dd>{{environment?.kernel || '--'}}</dd></div><div><dt>设备温度</dt><dd>{{environment?.cpuTemperature || '--'}}</dd></div></dl></article>
      <article class="panel info-panel"><h2>数据安全</h2><dl><div><dt>Outbox</dt><dd>{{store.snapshot?.pending}} 条</dd></div><div><dt>SQLite WAL</dt><dd>--</dd></div><div><dt>配置备份</dt><dd>--</dd></div><div><dt>密钥保护</dt><dd>--</dd></div></dl></article>
    </section>

    <section class="panel environment-panel">
      <div class="panel-title">
        <div><h2>环境信息</h2><p>{{environment?.hostname || 'RK3568 开发板'}} · {{environment?.osRelease || '读取真实系统环境'}}</p></div>
        <StatusBadge :ok="!envError" :text="envError ? '读取失败' : envLoading ? '刷新中' : '实时读取'" />
      </div>
      <div v-if="envError" class="empty-row">{{envError}}</div>
      <div v-else class="environment-grid">
        <article class="env-summary">
          <div><small>CPU 温度</small><strong>{{environment?.cpuTemperature || '--'}}</strong></div>
          <div><small>架构</small><strong>{{environment?.architecture || '--'}}</strong></div>
          <div><small>CPU</small><strong>{{environment?.cpuModel || '--'}}</strong><span>{{environment?.cpuCores || 0}} 核</span></div>
          <div><small>负载</small><strong>{{environment?.loadAverage || '--'}}</strong></div>
        </article>
        <article class="env-list-card"><h3>内存 / Swap</h3><dl><div v-for="item in environment?.memory" :key="item.key"><dt>{{item.key}}</dt><dd>{{item.value}}</dd></div></dl></article>
        <article class="env-list-card"><h3>存储挂载</h3><dl><div v-for="item in environment?.storage" :key="item.key"><dt>{{item.key}}</dt><dd>{{item.value}}</dd></div></dl></article>
        <article class="env-list-card"><h3>运行环境</h3><dl><div v-for="item in environment?.runtime" :key="item.key"><dt>{{item.key}}</dt><dd>{{item.value}}</dd></div></dl></article>
        <article class="env-list-card wide"><h3>内核参数</h3><dl><div v-for="item in environment?.kernelParams" :key="item.key"><dt>{{item.key}}</dt><dd>{{item.value}}</dd></div></dl></article>
        <article class="env-list-card"><h3>温度分区</h3><dl><div v-for="item in environment?.thermalZones" :key="item.key"><dt>{{item.key}}</dt><dd>{{item.value}}</dd></div></dl></article>
        <article class="env-list-card wide boot-card"><h3>启动参数</h3><p>{{environment?.bootParams || '--'}}</p></article>
      </div>
    </section>

    <Teleport to="body">
      <div v-if="pendingPowerAction" class="dialog-mask" @click.self="closePowerConfirm">
        <section class="dialog danger power-confirm-dialog">
          <header>
            <strong>{{powerConfirmStep === 1 ? `确认${powerActionText[pendingPowerAction].title}` : `最终确认${powerActionText[pendingPowerAction].title}`}}</strong>
          </header>
          <p v-if="powerConfirmStep === 1">
            这个操作会直接对开发板执行系统级{{powerActionText[pendingPowerAction].title}}命令，请确认当前现场允许这样操作。
          </p>
          <p v-else>
            第二次确认：点击确认后命令会立即发送，当前页面可能马上断开。
          </p>
          <footer>
            <button class="btn" :disabled="powerBusy" @click="closePowerConfirm">取消</button>
            <button class="btn danger power-danger" :disabled="powerBusy" @click="confirmPowerAction">{{powerBusy ? '发送中...' : '确认'}}</button>
          </footer>
        </section>
      </div>
    </Teleport>
  </div>
</template>
