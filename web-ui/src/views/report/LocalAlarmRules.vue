<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { Eye, Pencil, Plus, Power, RefreshCw, Trash2, X } from 'lucide-vue-next'
import SideSelect, { type SideSelectOption } from '../../components/SideSelect.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import { deleteAlarmRule, fetchAlarmRules, fetchMeters, saveAlarmRule } from '../../services/gateway-api'
import type { AlarmRule, AlarmRuleConfig, Meter, PointValue } from '../../types/gateway'
import { useDialog } from '../../composables/useDialog'
import { useToast } from '../../composables/useToast'

const { show } = useDialog()
const toast = useToast()
const config = ref<AlarmRuleConfig>({ syncIntervalS: 300, lastSync: '--', rules: [] })
const meters = ref<Meter[]>([])
const busy = ref(false)
const editing = ref(false)
const editingId = ref<number | null>(null)
const deleting = ref<AlarmRule | null>(null)
const detail = ref<AlarmRule | null>(null)
const form = reactive({ name: '', level: 'WARN', targetDeviceSn: '', pointCode: '', operator: '>', threshold: 0, durationS: 60 })

const localRules = computed(() => config.value.rules.filter(rule => rule.source === 'LOCAL'))
const selectedMeter = computed(() => meters.value.find(meter => meter.sn === form.targetDeviceSn) ?? null)
const selectedPoints = computed<PointValue[]>(() => selectedMeter.value?.points ?? [])
const selectedPoint = computed(() => selectedPoints.value.find(point => point.code === form.pointCode) ?? null)
const meterOptions = computed<SideSelectOption[]>(() => [
  { label: '请选择设备', value: '', disabled: true },
  ...meters.value.map(meter => ({ label: `${meter.name} · ${meter.sn}`, value: meter.sn })),
])
const pointOptions = computed<SideSelectOption[]>(() => [
  { label: '请选择测点', value: '', disabled: true },
  ...selectedPoints.value.map(point => ({ label: `${point.name} · ${point.code}`, value: point.code })),
])
const operatorOptions: SideSelectOption[] = [
  { label: '>', value: '>' },
  { label: '>=', value: '>=' },
  { label: '<', value: '<' },
  { label: '<=', value: '<=' },
  { label: '==', value: '==' },
  { label: '!=', value: '!=' },
]
const levelOptions: SideSelectOption[] = [
  { label: 'WARN', value: 'WARN' },
  { label: 'ERROR', value: 'ERROR' },
  { label: 'INFO', value: 'INFO' },
]

async function load() {
  const [ruleConfig, meterList] = await Promise.all([fetchAlarmRules(), fetchMeters()])
  config.value = ruleConfig
  meters.value = meterList.filter(meter => meter.channelId !== '__staging__')
}

function openCreate() {
  editingId.value = null
  const meter = meters.value[0]
  const point = meter?.points?.[0]
  Object.assign(form, { name: '', level: 'WARN', targetDeviceSn: meter?.sn ?? '', pointCode: point?.code ?? '', operator: '>', threshold: 0, durationS: 60 })
  editing.value = true
}

function openEdit(rule: AlarmRule) {
  if (rule.locked) return
  editingId.value = rule.id
  Object.assign(form, { name: rule.name, level: rule.level, targetDeviceSn: rule.targetDeviceSn ?? '', pointCode: rule.pointCode, operator: rule.operator, threshold: rule.threshold, durationS: rule.durationS })
  editing.value = true
}

async function submit() {
  const existing = editingId.value ? localRules.value.find(rule => rule.id === editingId.value) : null
  if (!form.name.trim()) return show('保存失败', '请填写本地告警名称', true)
  if (!form.targetDeviceSn) return show('保存失败', '请选择需要监测的设备', true)
  if (!form.pointCode || !selectedPoint.value) return show('保存失败', '请选择该设备下的测点编码', true)
  busy.value = true
  try {
    const savedName = form.name
    const wasEditing = !!editingId.value
    await saveAlarmRule(editingId.value, {
      ...form,
      unit: selectedPoint.value.unit,
      enabled: existing?.enabled ?? false,
    })
    editing.value = false
    editingId.value = null
    await load()
    toast.success(wasEditing ? `本地告警「${savedName}」已更新` : `本地告警「${savedName}」已创建，默认停用`)
  } catch (e) {
    show('保存失败', e instanceof Error ? e.message : '本地告警规则保存失败', true)
  } finally {
    busy.value = false
  }
}

async function confirmDelete() {
  if (!deleting.value) return
  const rule = deleting.value
  busy.value = true
  try {
    await deleteAlarmRule(rule.id)
    deleting.value = null
    await load()
    toast.success(`本地告警规则「${rule.name}」已删除`)
  } catch (e) {
    show('删除失败', e instanceof Error ? e.message : '平台规则锁定，或本地规则不存在', true)
  } finally {
    busy.value = false
  }
}

async function toggleRule(rule: AlarmRule) {
  if (rule.locked) return
  busy.value = true
  try {
    await saveAlarmRule(rule.id, { ...rule, enabled: !rule.enabled })
    await load()
    toast.success(`本地告警「${rule.name}」已${rule.enabled ? '停用' : '启用'}`)
  } catch (e) {
    show('操作失败', e instanceof Error ? e.message : '本地告警规则状态更新失败', true)
  } finally {
    busy.value = false
  }
}

function deviceLabel(sn: string | null) {
  if (!sn) return '未指定'
  const meter = meters.value.find(item => item.sn === sn)
  return meter ? `${meter.name} · ${meter.sn}` : sn
}

function pointLabel(rule: AlarmRule) {
  const meter = meters.value.find(item => item.sn === rule.targetDeviceSn)
  const point = meter?.points.find(item => item.code === rule.pointCode)
  return point ? `${point.name} · ${point.code}` : rule.pointCode
}

watch(() => form.targetDeviceSn, () => {
  const firstPoint = selectedPoints.value[0]
  if (!selectedPoints.value.some(point => point.code === form.pointCode)) {
    form.pointCode = firstPoint?.code ?? ''
  }
})

onMounted(() => load().catch(e => show('读取失败', e instanceof Error ? e.message : '本地告警规则读取失败', true)))
</script>

<template>
  <div class="alarm-rule-subpage local-alarm-subpage">
    <article class="panel table-panel alarm-rule-full-panel">
      <div class="panel-title">
        <div><h2>本地告警规则</h2><p>本地规则只在网关侧维护，新增后默认停用，需手动启用后才生效</p></div>
        <div class="button-row"><button class="icon-only refresh-action" title="刷新" :disabled="busy" @click="load"><RefreshCw /></button><button class="btn primary" @click="openCreate"><Plus />新增本地告警</button></div>
      </div>
      <div class="table local-alarm-table fill-table alarm-rule-table">
        <div class="tr th"><span>规则</span><span>设备</span><span>测点</span><span>条件</span><span>级别</span><span>持续</span><span>状态</span><span>操作</span></div>
        <div v-for="rule in localRules" :key="rule.id" class="tr">
          <span>{{ rule.name }}</span><span>{{ deviceLabel(rule.targetDeviceSn) }}</span><span>{{ pointLabel(rule) }}</span><span>{{ rule.operator }} {{ rule.threshold }} {{ rule.unit }}</span><span>{{ rule.level }}</span><span>{{ rule.durationS }}s</span><span><StatusBadge :ok="rule.enabled" :text="rule.enabled ? '启用' : '停用'" /></span>
          <span class="row-actions">
            <button class="icon-btn" title="查看详情" @click="detail = rule"><Eye /></button>
            <button class="icon-btn" title="编辑本地告警" @click="openEdit(rule)"><Pencil /></button>
            <button class="icon-btn" :class="{active:rule.enabled}" :title="rule.enabled ? '停用规则' : '启用规则'" :disabled="busy" @click="toggleRule(rule)"><Power /></button>
            <button class="icon-btn danger" title="删除本地告警" @click="deleting = rule"><Trash2 /></button>
          </span>
        </div>
        <div v-if="!localRules.length" class="empty-row">暂无本地告警规则</div>
      </div>
    </article>

    <Teleport to="body"><div v-if="editing" class="dialog-mask" @click.self="editing=false"><section class="dialog editor-dialog"><header><strong>{{ editingId ? '编辑本地告警' : '新增本地告警' }}</strong><button @click="editing=false"><X /></button></header><div class="form-grid">
      <label><span>告警名称</span><input v-model.trim="form.name" placeholder="例如：1号表总有功过高"></label>
      <label><span>选择设备</span><SideSelect v-model="form.targetDeviceSn" :options="meterOptions" /></label>
      <label><span>测点编码</span><SideSelect v-model="form.pointCode" :options="pointOptions" :disabled="!selectedPoints.length" /></label>
      <label><span>单位</span><input :value="selectedPoint?.unit || '-'" disabled></label>
      <label><span>比较符</span><SideSelect v-model="form.operator" :options="operatorOptions" /></label>
      <label><span>阈值</span><input v-model.number="form.threshold" type="number"></label>
      <label><span>告警级别</span><SideSelect v-model="form.level" :options="levelOptions" /></label>
      <label><span>持续时间（秒）</span><input v-model.number="form.durationS" type="number" min="1"></label>
    </div><footer><button class="btn" @click="editing=false">取消</button><button class="btn primary" :disabled="busy" @click="submit">{{ busy ? '保存中' : '保存规则' }}</button></footer></section></div></Teleport>

    <Teleport to="body"><div v-if="detail" class="dialog-mask" @click.self="detail=null"><section class="dialog detail-dialog alarm-detail-dialog"><header><Eye /><strong>本地告警详情</strong><button @click="detail=null"><X /></button></header><dl class="detail-grid">
      <div><dt>告警名称</dt><dd>{{ detail.name }}</dd></div>
      <div><dt>规则编码</dt><dd>{{ detail.ruleCode }}</dd></div>
      <div><dt>监测设备</dt><dd>{{ deviceLabel(detail.targetDeviceSn) }}</dd></div>
      <div><dt>测点</dt><dd>{{ pointLabel(detail) }}</dd></div>
      <div><dt>触发条件</dt><dd>{{ detail.operator }} {{ detail.threshold }} {{ detail.unit }}</dd></div>
      <div><dt>持续时间</dt><dd>{{ detail.durationS }} 秒</dd></div>
      <div><dt>告警级别</dt><dd>{{ detail.level }}</dd></div>
      <div><dt>当前状态</dt><dd>{{ detail.enabled ? '已启用' : '已停用' }}</dd></div>
      <div><dt>来源</dt><dd>网关本地</dd></div>
      <div><dt>更新时间</dt><dd>{{ detail.updated }}</dd></div>
    </dl><footer><button class="btn primary" @click="detail=null">关闭</button></footer></section></div></Teleport>

    <Teleport to="body"><div v-if="deleting" class="dialog-mask" @click.self="deleting=null"><section class="dialog confirm-dialog"><header><Trash2 /><strong>删除本地告警规则</strong><button @click="deleting=null"><X /></button></header><p>确认删除「{{ deleting?.name }}」？平台同步规则不会被删除。</p><footer><button class="btn" @click="deleting=null">取消</button><button class="btn danger" :disabled="busy" @click="confirmDelete">确认删除</button></footer></section></div></Teleport>
  </div>
</template>
