<script setup lang="ts">
import { computed, ref } from 'vue'
import { Check, EthernetPort, Lock, RefreshCw, Wifi, X } from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import { useGatewayStore } from '../../stores/gateway'
import { gatewayAction } from '../../services/gateway-api'
import type { WifiNetwork } from '../../types/gateway'
import { useDialog } from '../../composables/useDialog'

const store = useGatewayStore()
const { show } = useDialog()
const busy = ref(false)
const connectOpen = ref(false)
const selectedNetwork = ref<WifiNetwork | null>(null)
const password = ref('')
const networkFeedback = ref('')
const wifi = computed(() => store.snapshot?.wifi)
const networks = computed(() => store.snapshot?.networks ?? [])
const sortedNetworks = computed(() => [...networks.value].sort((a, b) => Number(b.connected) - Number(a.connected) || b.signal - a.signal))

async function run(success: string, action: () => Promise<void>, feedback = '正在处理网络操作...') {
  busy.value = true
  networkFeedback.value = feedback
  try {
    await action()
    await store.refresh()
    networkFeedback.value = '网络状态已更新'
    show(success, '网络状态已更新。')
  } catch (error) {
    networkFeedback.value = error instanceof Error ? error.message : '网络操作失败'
    show('操作失败', networkFeedback.value, true)
  } finally {
    busy.value = false
  }
}

function openConnect(network: WifiNetwork) {
  selectedNetwork.value = network
  password.value = ''
  networkFeedback.value = ''
  connectOpen.value = true
}

const scan = () => run('Wi‑Fi 扫描完成', () => gatewayAction('/v1/network/scan'), '正在扫描附近 Wi-Fi...')
const connect = async () => {
  if (!selectedNetwork.value) return
  await run(`正在连接 ${selectedNetwork.value.ssid}`, () => gatewayAction('/v1/network/connect', { ssid: selectedNetwork.value?.ssid, password: password.value }), `正在连接 ${selectedNetwork.value.ssid}...`)
  if (networkFeedback.value === '网络状态已更新') connectOpen.value = false
}
const disconnect = () => run('Wi‑Fi 已断开', () => gatewayAction('/v1/network/disconnect'), '正在断开当前 Wi-Fi...')
const forget = async (network: WifiNetwork) => {
  await run(`已忘记 ${network.ssid}`, () => gatewayAction('/v1/network/forget', { ssid: network.ssid }), `正在忘记 ${network.ssid}...`)
  if (networkFeedback.value === '网络状态已更新' && selectedNetwork.value?.ssid === network.ssid) connectOpen.value = false
}
</script>

<template>
  <div class="page network-page">
    <PageHeader title="网络配置" subtitle="管理网关 Wi‑Fi、LAN 地址与本机网络状态">
      <button class="icon-only refresh-action" title="刷新网络" :disabled="busy" @click="store.refresh"><RefreshCw /></button>
    </PageHeader>

    <section class="network-apple-layout">
      <article class="panel network-hero-card">
        <div class="network-orb" :class="{online:wifi?.connected}"><Wifi /></div>
        <div class="network-hero-main">
          <span>{{ wifi?.interfaceName || 'wlan0' }}</span>
          <h2>{{ wifi?.connected ? wifi.ssid : '未连接 Wi‑Fi' }}</h2>
          <p>{{ wifi?.connected ? '无线网络已连接，平台上报链路可用' : '请选择一个可用网络进行连接' }}</p>
        </div>
        <StatusBadge :ok="wifi?.connected" :text="wifi?.connected ? '在线' : '离线'" />
        <div class="network-info-grid">
          <div><small>信号强度</small><strong>{{ wifi?.signal ?? 0 }}%</strong></div>
          <div><small>IPv4 地址</small><strong>{{ wifi?.ipv4 || '--' }}</strong></div>
          <div><small>默认网关</small><strong>{{ wifi?.gateway || '--' }}</strong></div>
          <div><small>DNS</small><strong>{{ wifi?.dns || '--' }}</strong></div>
        </div>
        <div class="network-primary-actions">
          <button class="btn primary" :disabled="busy" @click="scan"><RefreshCw />扫描 Wi‑Fi</button>
          <button class="btn" :disabled="busy || !wifi?.connected" @click="disconnect">断开当前网络</button>
        </div>
      </article>

      <article class="panel network-list-card">
        <div class="panel-title">
          <div><h2>可用 Wi‑Fi</h2><p>选择网络后输入密码连接</p></div>
          <StatusBadge :ok="networks.length > 0" :text="`${networks.length} 个`" />
        </div>
        <div class="apple-wifi-list">
          <button v-for="network in sortedNetworks" :key="network.ssid" class="apple-wifi-row" :class="{connected:network.connected}" @click="openConnect(network)">
            <span class="wifi-row-icon"><Wifi /></span>
            <span class="wifi-row-main"><strong>{{ network.ssid }}</strong><small>{{ network.security || '开放网络' }}</small></span>
            <span class="wifi-signal">{{ network.signal }}%</span>
            <Lock v-if="network.security && network.security !== 'OPEN'" class="wifi-lock" />
            <Check v-if="network.connected" class="wifi-check" />
          </button>
          <div v-if="!sortedNetworks.length" class="empty-row">暂无可用 Wi‑Fi，请点击扫描</div>
        </div>
      </article>

      <article class="panel network-lan-card">
        <div class="panel-title"><div><h2>LAN / 本机地址</h2><p>RK3568/RK3566 有线网络状态</p></div><EthernetPort /></div>
        <dl class="network-kv apple">
          <div><dt>接口</dt><dd>eth0</dd></div>
          <div><dt>IPv4</dt><dd>{{ wifi?.ipv4 || '--' }}</dd></div>
          <div><dt>网关</dt><dd>{{ wifi?.gateway || '--' }}</dd></div>
          <div><dt>DNS</dt><dd>{{ wifi?.dns || '--' }}</dd></div>
        </dl>
      </article>
    </section>

    <Teleport to="body">
      <div v-if="connectOpen" class="dialog-mask" @click.self="connectOpen=false">
        <section class="dialog wifi-dialog">
          <header><Wifi /><strong>连接 {{ selectedNetwork?.ssid }}</strong><button @click="connectOpen=false"><X /></button></header>
          <div class="dialog-form">
            <p class="wifi-connect-meta">{{ selectedNetwork?.signal }}% · {{ selectedNetwork?.security || '开放网络' }}</p>
            <label>Wi‑Fi 密码<input v-model="password" type="password" placeholder="请输入网络密码"></label>
            <div v-if="networkFeedback" class="wifi-action-feedback" :class="{busy}">{{networkFeedback}}</div>
          </div>
          <footer>
            <button class="btn" @click="connectOpen=false">取消</button>
            <button class="btn danger" :disabled="busy || !selectedNetwork" @click="selectedNetwork && forget(selectedNetwork)">忘记网络</button>
            <button class="btn primary" :disabled="busy || !selectedNetwork" @click="connect">{{busy ? '连接中...' : '连接'}}</button>
          </footer>
        </section>
      </div>
    </Teleport>
  </div>
</template>
