<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { Activity, Cloud, Save } from 'lucide-vue-next'
import StatusBadge from '../../../components/StatusBadge.vue'
import { useGatewayStore } from '../../../stores/gateway'
import { fetchPlatformConfig, gatewayAction } from '../../../services/gateway-api'
import { useDialog } from '../../../composables/useDialog'
import { useToast } from '../../../composables/useToast'

const store = useGatewayStore()
const { show } = useDialog()
const toast = useToast()
const busy = ref(false)
const form = reactive({ gatewayId: '', gatewaySn: '', platformHttpUrl: '', mqttHost: '', mqttPort: 1883, mqttUsername: '', mqttPassword: '', heartbeatS: 30 })

async function load() {
  const value = await fetchPlatformConfig()
  Object.assign(form, { ...value, mqttPassword: '' })
}

async function submit(diagnose = false) {
  busy.value = true
  try {
    if (diagnose) await gatewayAction('/v1/platform/diagnose')
    else await gatewayAction('/v1/platform', form)
    toast.success(diagnose ? 'MQTT 链路诊断已发起' : `平台配置已保存：${form.gatewaySn} · ${form.mqttHost}:${form.mqttPort}`)
    await store.refresh()
    if (!diagnose) window.setTimeout(() => store.refresh(), 300)
  } catch (e) {
    show('操作失败', e instanceof Error ? e.message : '平台操作失败', true)
  } finally {
    busy.value = false
  }
}

onMounted(() => load().catch(e => show('配置读取失败', e instanceof Error ? e.message : '无法读取平台配置', true)))
</script>

<template>
  <section class="settings-layout channel-subpage mqtt-settings-page">
    <article class="panel form-panel mqtt-form-panel">
      <div class="panel-title"><div><h2>MQTT 平台</h2><p>配置 Broker、网关身份与心跳周期</p></div><StatusBadge :ok="store.snapshot?.cloudOnline" :text="store.snapshot?.mqttLabel ?? '未知'" /></div>
      <div class="form-grid mqtt-form-grid">
        <label>MQTT 地址<input v-model="form.mqttHost" placeholder="请输入 Broker 地址"></label>
        <label>端口<input v-model.number="form.mqttPort" type="number"></label>
        <label>平台地址<input v-model="form.platformHttpUrl" placeholder="请输入平台 HTTP 地址"></label>
        <label>网关编号<input v-model="form.gatewayId"></label>
        <label>网关序列号<input v-model="form.gatewaySn"></label>
        <label>用户名<input v-model="form.mqttUsername"></label>
        <label>密码<input v-model="form.mqttPassword" type="password" placeholder="留空则保留现有密码"></label>
        <label>心跳周期 (s)<input v-model.number="form.heartbeatS" type="number"></label>
      </div>
      <div class="button-row mqtt-actions"><button class="btn" :disabled="busy" @click="submit(true)"><Activity />链路诊断</button><button class="btn primary" :disabled="busy" @click="submit(false)"><Save />保存并应用</button></div>
    </article>
    <article class="panel info-panel mqtt-info-panel"><div class="channel-head"><span><Cloud /></span><div><h2>当前链路</h2><p>心跳 30 秒，平台 90 秒未收到则判离线</p></div></div><dl><div><dt>云平台</dt><dd>{{ store.snapshot?.mqttLabel }}</dd></div><div><dt>待上报</dt><dd>{{ store.snapshot?.pending }} 条</dd></div><div><dt>累计成功</dt><dd>{{ store.snapshot?.sent }} 条</dd></div><div><dt>最后发布</dt><dd>{{ store.snapshot?.lastPublish }}</dd></div></dl></article>
  </section>
</template>
