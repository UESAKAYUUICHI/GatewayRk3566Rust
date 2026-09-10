<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { AlertTriangle, Cable, Eye, Plus, RefreshCw, Save, Trash2, X } from 'lucide-vue-next'
import SideSelect, { type SideSelectOption } from '../../../components/SideSelect.vue'
import StatusBadge from '../../../components/StatusBadge.vue'
import { useGatewayStore } from '../../../stores/gateway'
import { createChannel, deleteChannel, fetchChannels, saveChannel } from '../../../services/gateway-api'
import type { Rs485Channel } from '../../../types/gateway'
import { useDialog } from '../../../composables/useDialog'
import { useToast } from '../../../composables/useToast'

const store = useGatewayStore()
const channels = ref<Rs485Channel[]>([])
const selectedId = ref('rs485-1')
const saving = ref(false)
const detailOpen = ref(false)
const confirmingDisable = ref(false)
const confirmingDelete = ref(false)
const { show } = useDialog()
const toast = useToast()
const portPresets = ['/dev/ttyS0', '/dev/ttyS1', '/dev/ttyS2', '/dev/ttyS3', '/dev/ttyS4', '/dev/ttyS5', '/dev/ttyS6', '/dev/ttyS7', '/dev/ttyUSB0', '/dev/ttyUSB1', '/dev/ttyAMA0', '/dev/ttyAMA1', '/dev/ttyFIQ0', '/dev/rs485-usb']
const baudOptions: SideSelectOption[] = [{ label: '9600', value: 9600 }, { label: '19200', value: 19200 }, { label: '38400', value: 38400 }, { label: '115200', value: 115200 }]
const parityOptions: SideSelectOption[] = [{ label: 'NONE', value: 'NONE' }, { label: 'EVEN', value: 'EVEN' }, { label: 'ODD', value: 'ODD' }]
const dataBitsOptions: SideSelectOption[] = [{ label: '8', value: 8 }, { label: '7', value: 7 }]
const stopBitsOptions: SideSelectOption[] = [{ label: '1', value: 1 }, { label: '2', value: 2 }]
const form = reactive<Rs485Channel>({ id: 'rs485-1', name: '串口通道-1', port: '/dev/ttyS0', baud: 9600, dataBits: 8, stopBits: 1, parity: 'NONE', enabled: true })
const current = computed(() => channels.value.find(c => c.id === selectedId.value) ?? channels.value[0])
const boundMeters = computed(() => (store.snapshot?.meters ?? []).filter(m => m.channelId === form.id))
const onlineBoundMeters = computed(() => boundMeters.value.filter(m => m.online).length)
const channelStatus = computed(() => !form.enabled ? { ok: false, text: '已停用' } : boundMeters.value.length ? { ok: onlineBoundMeters.value > 0, text: `${onlineBoundMeters.value}/${boundMeters.value.length} 在线` } : { ok: true, text: '空闲总线' })

async function load(silent = true) {
  channels.value = await fetchChannels()
  if (!channels.value.find(c => c.id === selectedId.value)) selectedId.value = channels.value[0]?.id ?? 'rs485-1'
  select(current.value)
  await store.refresh()
  if (!silent) toast.success('串口通道状态已刷新')
}

function select(channel?: Rs485Channel) {
  if (!channel) return
  selectedId.value = channel.id
  Object.assign(form, channel)
}

function requestToggle() {
  if (form.enabled) {
    confirmingDisable.value = true
    return
  }
  form.enabled = true
  toast.success(`${form.name} 已切换为启用，保存后写入网关`)
}

function confirmDisable() {
  form.enabled = false
  confirmingDisable.value = false
  toast.success(`${form.name} 已切换为停用，保存后写入网关`)
}

async function submit() {
  saving.value = true
  try {
    const savedName = form.name
    const savedPort = form.port
    const savedBaud = form.baud
    const savedEnabled = form.enabled
    await saveChannel(form.id, form)
    await load()
    detailOpen.value = false
    toast.success(`${savedName} 已保存：${savedPort} · ${savedBaud} · ${savedEnabled ? '启用' : '停用'}`)
  } catch (e) {
    show('保存失败', e instanceof Error ? e.message : '通道保存失败', true)
  } finally {
    saving.value = false
  }
}

async function addChannel() {
  saving.value = true
  try {
    const nextIndex = channels.value.length + 1
    const created = await createChannel({
      name: `串口通道-${nextIndex}`,
      port: portPresets[Math.min(nextIndex - 1, portPresets.length - 1)],
      baud: 9600,
      dataBits: 8,
      stopBits: 1,
      parity: 'NONE',
      enabled: true,
    })
    await load()
    select(created)
    toast.success(`${created.name} 已新增，可在设备监控里绑定设备`)
  } catch (e) {
    show('新增失败', e instanceof Error ? e.message : '通道新增失败', true)
  } finally {
    saving.value = false
  }
}

async function confirmDelete() {
  saving.value = true
  try {
    const removedName = form.name
    const removedCount = boundMeters.value.length
    await deleteChannel(form.id)
    confirmingDelete.value = false
    await load()
    toast.success(`${removedName} 已删除${removedCount ? `，同步删除 ${removedCount} 台绑定设备` : ''}`)
  } catch (e) {
    show('删除失败', e instanceof Error ? e.message : '通道删除失败', true)
  } finally {
    saving.value = false
  }
}

onMounted(() => load().catch(e => show('读取失败', e instanceof Error ? e.message : '无法读取通道', true)))
</script>

<template>
  <section class="channel-workspace single">
    <article class="panel serial-console">
      <div class="panel-title">
        <div>
          <h2>串口通道</h2>
          <p>串口设备路径可自定义，适配普通 UART、USB 转串口或 RS485 转换器</p>
        </div>
        <div class="button-row">
          <button class="icon-only refresh-action" title="新增通道" :disabled="saving" @click="addChannel"><Plus /></button>
          <button class="icon-only refresh-action" title="查看详情" :disabled="saving || !current" @click="detailOpen=true"><Eye /></button>
          <button class="icon-only refresh-action danger" title="删除当前通道" :disabled="saving || !current" @click="confirmingDelete=true"><Trash2 /></button>
          <button class="icon-only refresh-action" title="刷新" :disabled="saving" @click="load(false)"><RefreshCw /></button>
        </div>
      </div>
      <div class="serial-body serial-body-list-only">
        <aside class="serial-tabs">
          <button v-for="(channel, index) in channels" :key="channel.id" :class="{ active: form.id === channel.id }" @click="select(channel)">
            <span class="serial-tab-index">{{ String(index + 1).padStart(2, '0') }}</span>
            <div class="serial-tab-name"><strong>{{ channel.name }}</strong><small>{{ channel.port }}</small></div>
            <StatusBadge :ok="channel.id === form.id ? form.enabled : channel.enabled" :text="(channel.id === form.id ? form.enabled : channel.enabled) ? '启用' : '停用'" />
          </button>
        </aside>
      </div>
    </article>
    <Teleport to="body"><div v-if="detailOpen" class="dialog-mask" @click.self="detailOpen=false"><section class="dialog serial-detail-dialog"><header><Eye/><strong>串口通道详情</strong><button @click="detailOpen=false"><X/></button></header><div class="serial-dialog-body">
        <section class="serial-editor">
          <div class="channel-state-line">
            <Cable />
            <strong>{{ form.name }}</strong>
            <StatusBadge :ok="channelStatus.ok" :text="channelStatus.text" />
            <span>{{ boundMeters.length ? `${boundMeters.length} 台：${boundMeters.map(m => `${m.sn}@${m.address}`).join('、')}` : '未绑定设备' }}</span>
            <button class="toggle-switch" :class="{ on: form.enabled }" @click="requestToggle">
              <i />
              <b>{{ form.enabled ? '启用' : '停用' }}</b>
            </button>
          </div>
          <div class="serial-space">
            <div class="serial-bound-list">
              <strong>当前总线设备</strong>
              <span>{{ boundMeters.length ? boundMeters.map(m => `${m.sn}@${m.address}`).join('、') : '未绑定设备' }}</span>
            </div>
          </div>
          <div class="serial-config-dock">
            <div class="form-grid three">
              <label><span>通道名称</span><input v-model.trim="form.name" /></label>
              <label><span>串口设备</span><input v-model.trim="form.port" list="serial-port-presets" placeholder="/dev/ttyS0 或自定义路径" /><datalist id="serial-port-presets"><option v-for="p in portPresets" :key="p" :value="p" /></datalist></label>
              <label><span>波特率</span><SideSelect v-model="form.baud" :options="baudOptions" /></label>
              <label><span>校验位</span><SideSelect v-model="form.parity" :options="parityOptions" /></label>
              <label><span>数据位</span><SideSelect v-model="form.dataBits" :options="dataBitsOptions" /></label>
              <label><span>停止位</span><SideSelect v-model="form.stopBits" :options="stopBitsOptions" /></label>
            </div>
            <div class="serial-footer">
              <span class="serial-hint">可直接填写板端实际存在的 /dev/tty* 设备；保存后按当前路径重新建立真实串口连接。</span>
              <button class="btn primary" :disabled="saving" @click="submit"><Save />{{ saving ? '保存中' : '保存通道' }}</button>
            </div>
          </div>
        </section>
      </div></section></div></Teleport>
    <Teleport to="body"><div v-if="confirmingDisable" class="dialog-mask" @click.self="confirmingDisable=false"><section class="dialog danger confirm-dialog"><header><AlertTriangle/><strong>确认停用串口通道</strong><button @click="confirmingDisable=false"><X/></button></header><p>停用 {{form.name}} 后，绑定在该通道上的设备将无法继续通过该串口采集。该变更会先进入当前表单，点击“保存通道”后正式写入网关配置。</p><footer><button class="btn" @click="confirmingDisable=false">取消</button><button class="btn danger" @click="confirmDisable">确认停用</button></footer></section></div></Teleport>
    <Teleport to="body"><div v-if="confirmingDelete" class="dialog-mask" @click.self="confirmingDelete=false"><section class="dialog danger confirm-dialog"><header><AlertTriangle/><strong>确认删除串口通道</strong><button @click="confirmingDelete=false"><X/></button></header><p>删除 {{form.name}} 后，设备监控中绑定到该通道的 {{boundMeters.length}} 台设备会一起删除，采集任务也会停止。</p><footer><button class="btn" @click="confirmingDelete=false">取消</button><button class="btn danger" :disabled="saving" @click="confirmDelete">{{saving?'删除中':'确认删除'}}</button></footer></section></div></Teleport>
  </section>
</template>
