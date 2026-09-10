<script setup lang="ts">
import { computed } from 'vue'
import { Database, HardDrive, RotateCcw } from 'lucide-vue-next'
import StatusBadge from '../../../components/StatusBadge.vue'
import { useGatewayStore } from '../../../stores/gateway'

const store = useGatewayStore()
const items = computed(() => [
  ['归批周期', '300 秒 / 5 分钟'],
  ['上报队列', `${store.snapshot?.pending ?? 0} 批待补发`],
  ['补发策略', '网络恢复后按顺序补发'],
  ['本地读数', 'meter_reading 保存最新值'],
  ['历史样本', 'meter_sample 保存中间样本'],
  ['上报缓存', 'outbox_message 保存待发批次'],
  ['缓存保护', '断网持久化、不丢样本'],
  ['清理策略', '已发送 Outbox 定期清理'],
])
</script>

<template>
  <section class="cache-layout">
    <article class="panel channel-config-card cache-main">
      <div class="channel-head"><span><Database /></span><div><h2>边缘缓存</h2><p>负责离线留存、归批上报、断网补发</p></div><StatusBadge :ok="(store.snapshot?.pending ?? 0) < 100" :text="(store.snapshot?.pending ?? 0) < 100 ? '正常' : '积压'" /></div>
      <dl><div v-for="item in items" :key="item[0]"><dt>{{ item[0] }}</dt><dd>{{ item[1] }}</dd></div></dl>
      <section class="cache-metrics">
        <article class="metric"><HardDrive /><span>本地队列</span><strong>{{ store.snapshot?.pending ?? 0 }}</strong><small>待上报批次</small></article>
        <article class="metric"><RotateCcw /><span>累计发布</span><strong>{{ store.snapshot?.sent ?? 0 }}</strong><small>MQTT PUBACK 成功</small></article>
      </section>
    </article>
  </section>
</template>
