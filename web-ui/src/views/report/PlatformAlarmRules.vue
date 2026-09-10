<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Lock, RefreshCw, Save } from 'lucide-vue-next'
import StatusBadge from '../../components/StatusBadge.vue'
import { fetchAlarmRules, saveAlarmRuleSyncConfig, syncAlarmRules } from '../../services/gateway-api'
import type { AlarmRuleConfig } from '../../types/gateway'
import { useDialog } from '../../composables/useDialog'
import { useToast } from '../../composables/useToast'

const { show } = useDialog()
const toast = useToast()
const config = ref<AlarmRuleConfig>({ syncIntervalS: 300, lastSync: '--', rules: [] })
const busy = ref(false)
const intervalS = ref(300)
const platformRules = computed(() => config.value.rules.filter(rule => rule.source === 'PLATFORM'))

async function load() {
  config.value = await fetchAlarmRules()
  intervalS.value = config.value.syncIntervalS
}

async function sync() {
  busy.value = true
  try {
    config.value = await syncAlarmRules()
    intervalS.value = config.value.syncIntervalS
    toast.success(`平台告警规则同步完成：${platformRules.value.length} 条锁定规则`)
  } catch (e) {
    show('同步失败', e instanceof Error ? e.message : '平台告警规则同步失败', true)
  } finally {
    busy.value = false
  }
}

async function saveInterval() {
  busy.value = true
  try {
    await saveAlarmRuleSyncConfig(intervalS.value)
    await load()
    toast.success(`平台告警规则同步周期已设为 ${intervalS.value} 秒`)
  } catch (e) {
    show('保存失败', e instanceof Error ? e.message : '同步周期保存失败', true)
  } finally {
    busy.value = false
  }
}

onMounted(() => load().catch(e => show('读取失败', e instanceof Error ? e.message : '平台告警规则读取失败', true)))
</script>

<template>
  <div class="alarm-rule-subpage">
    <section class="alarm-rule-config panel">
      <div>
        <h2>平台规则同步</h2>
        <p>平台自动化规则下发到网关后锁定只读，网关只负责同步、展示和本地执行。</p>
      </div>
      <label><span>同步间隔（秒）</span><input v-model.number="intervalS" type="number" min="30" step="30"></label>
      <button class="btn" :disabled="busy" @click="saveInterval"><Save />保存周期</button>
      <button class="btn primary" :disabled="busy" @click="sync"><RefreshCw />立即同步</button>
      <StatusBadge :ok="platformRules.length > 0" :text="`上次同步 ${config.lastSync}`" />
    </section>

    <article class="panel table-panel alarm-rule-full-panel">
      <div class="panel-title"><div><h2>平台同步规则</h2></div><StatusBadge :ok="true" text="锁定只读" /></div>
      <div class="table platform-alarm-table fill-table alarm-rule-table">
        <div class="tr th"><span>规则</span><span>设备</span><span>测点</span><span>条件</span><span>级别</span><span>持续</span><span>状态</span></div>
        <div v-for="rule in platformRules" :key="rule.id" class="tr locked-row">
          <span><Lock />{{ rule.name }}</span><span>{{ rule.targetDeviceSn || '全部设备' }}</span><span>{{ rule.pointCode }}</span><span>{{ rule.operator }} {{ rule.threshold }} {{ rule.unit }}</span><span>{{ rule.level }}</span><span>{{ rule.durationS }}s</span><span><StatusBadge :ok="rule.enabled" :text="rule.enabled ? '启用' : '停用'" /></span>
        </div>
        <div v-if="!platformRules.length" class="empty-row">暂无平台同步规则</div>
      </div>
    </article>
  </div>
</template>
