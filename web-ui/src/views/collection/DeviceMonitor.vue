<script setup lang="ts">
import {computed,onMounted,reactive,ref} from 'vue'
import {AlertTriangle,CloudDownload,Eye,Link2,Play,Plus,Square,Trash2,X} from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'
import SideSelect from '../../components/SideSelect.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import {useGatewayStore} from '../../stores/gateway'
import {bindMeter,deleteMeter,fetchChannels,fetchThingModels,saveMeter,setContinuousPull,syncPlatformConfig} from '../../services/gateway-api'
import type {Rs485Channel,ThingModel} from '../../types/gateway'
import {useDialog} from '../../composables/useDialog'
import {useToast} from '../../composables/useToast'

const store=useGatewayStore(),selected=ref<number|null>(null),editingId=ref<number|null>(null),editing=ref(false),saving=ref(false),syncing=ref(false),syncLocked=ref(false),deleting=ref(false),deleteTarget=ref<number|null>(null),detailOpen=ref(false),detailTarget=ref<number|null>(null),binding=ref(false),bindChannelId=ref(''),bindSourceId=ref<number|null>(null),pulling=ref(false)
const channels=ref<Rs485Channel[]>([]),models=ref<ThingModel[]>([])
const {show}=useDialog()
const toast=useToast()
const meter=computed(()=>store.snapshot?.meters.find(m=>m.id===(selected.value??store.snapshot?.meters[0]?.id)))
const detailMeter=computed(()=>store.snapshot?.meters.find(m=>m.id===detailTarget.value))
const form=reactive({deviceSn:'',deviceName:'',modbusAddr:1,profile:'',channelId:'rs485-1',collectIntervalS:60,enabled:true,uploadEnabled:false})
const channelIds=computed(()=>new Set(channels.value.map(c=>c.id)))
const stagedMeters=computed(()=>(store.snapshot?.meters??[]).filter(m=>!channelIds.value.has(m.channelId)))
const grouped=computed(()=>channels.value.map(channel=>({channel,meters:(store.snapshot?.meters??[]).filter(m=>m.channelId===channel.id)})))
const enabledChannels=computed(()=>channels.value.filter(c=>c.enabled))
const modelOptions=computed(()=>models.value.map(m=>({label:`${m.name} · ${m.version}`,value:m.profile})))
const channelOptions=computed(()=>channels.value.map(c=>({label:`${c.name} · ${c.port}${c.enabled?'':' · 已停用'}`,value:c.id,disabled:!c.enabled})))
const addressConflict=computed(()=>new Map((store.snapshot?.meters??[]).filter(m=>m.enabled&&m.id!==editingId.value).map(m=>[`${m.channelId}:${m.address}`,m])))
const hasEnabledChannel=computed(()=>enabledChannels.value.length>0)
const displayPointValue=(value:number|null|undefined)=>{
  if(value===null||value===undefined||Number.isNaN(value))return '-'
  return value.toFixed(2)
}
const pointValueStyle=(value:number|null|undefined)=>{
  const text=displayPointValue(value)
  const length=text.length
  if(text==='-')return {fontSize:'clamp(32px,4vw,54px)'}
  if(length<=5)return {fontSize:'clamp(26px,3.1vw,42px)'}
  if(length<=8)return {fontSize:'clamp(22px,2.6vw,34px)'}
  return {fontSize:'clamp(18px,2.1vw,28px)'}
}

async function loadMeta(){try{[channels.value,models.value]=await Promise.all([fetchChannels(),fetchThingModels()])}catch(e){show('读取失败',e instanceof Error?e.message:'无法读取物模型和通道',true)}}
function firstEnabledChannel(){return enabledChannels.value[0]?.id??channels.value[0]?.id??'rs485-1'}
function openCreate(){if(!hasEnabledChannel.value){show('无法新增设备','请先启用或新增一条 RS485 通道。',true);return}editingId.value=null;selected.value=null;Object.assign(form,{deviceSn:'',deviceName:'',modbusAddr:1,profile:models.value[0]?.profile??'',channelId:firstEnabledChannel(),collectIntervalS:60,enabled:true,uploadEnabled:false});editing.value=true}
function openEdit(){if(!meter.value)return;editingId.value=meter.value.id;Object.assign(form,{deviceSn:meter.value.sn,deviceName:meter.value.name,modbusAddr:meter.value.address,profile:meter.value.profile,channelId:meter.value.channelId,collectIntervalS:meter.value.collectIntervalS,enabled:meter.value.enabled,uploadEnabled:meter.value.uploadEnabled});editing.value=true}
async function submit(){saving.value=true;try{const savedName=form.deviceName||form.deviceSn;const savedChannel=form.channelId;await saveMeter(editingId.value,form);editing.value=false;editingId.value=null;await store.refresh();toast.success(`设备 ${savedName} 已保存到 ${savedChannel}`)}catch(e){show('保存失败',e instanceof Error?e.message:'设备配置无法保存',true)}finally{saving.value=false}}
async function syncOnline(){if(syncLocked.value)return;syncing.value=true;syncLocked.value=true;window.setTimeout(()=>{syncLocked.value=false},3000);try{const r=await syncPlatformConfig();await Promise.all([store.refresh(),loadMeta()]);toast.success(`线上同步完成：${r.devices} 台设备、${r.models} 个物模型`)}catch(e){show('同步失败',e instanceof Error?e.message:'平台配置同步失败',true)}finally{syncing.value=false}}
function askDelete(id:number){deleteTarget.value=id;deleting.value=true}
function showDetail(id:number){detailTarget.value=id;detailOpen.value=true}
async function confirmDelete(){if(deleteTarget.value==null)return;const id=deleteTarget.value;saving.value=true;try{await deleteMeter(id);deleting.value=false;deleteTarget.value=null;if(selected.value===id)selected.value=null;await store.refresh();toast.success(`设备 ID ${id} 已删除，采集任务已停止`)}catch(e){show('删除失败',e instanceof Error?e.message:'设备无法删除',true)}finally{saving.value=false}}
function askBind(channelId:string){const source=stagedMeters.value.find(m=>m.id===selected.value)??(stagedMeters.value.length===1?stagedMeters.value[0]:null);if(!source){show('请选择暂存设备','先在暂存区点选一台设备，再点击 RS485 行右侧的绑定图标。',true);return}bindChannelId.value=channelId;bindSourceId.value=source.id;binding.value=true}
async function confirmBind(){if(bindSourceId.value==null)return;saving.value=true;try{const targetId=bindSourceId.value;const channelId=bindChannelId.value;await bindMeter(targetId,channelId);binding.value=false;selected.value=targetId;bindSourceId.value=null;await store.refresh();toast.success(`设备已绑定到 ${channelId}，采集配置已刷新`)}catch(e){show('绑定失败',e instanceof Error?e.message:'设备无法绑定到该串口',true)}finally{saving.value=false}}
async function togglePull(){if(!meter.value||pulling.value)return;pulling.value=true;const enabled=!meter.value.continuousPull;try{await setContinuousPull(meter.value.id,enabled);await store.refresh();toast.success(enabled?`${meter.value.name} 已开始每 5 秒拉取`:`${meter.value.name} 已停止持续拉取`)}catch(e){show('拉取切换失败',e instanceof Error?e.message:'无法切换串口拉取状态',true)}finally{pulling.value=false}}
onMounted(loadMeta)
</script>

<template><div class="page device-page">
  <PageHeader title="设备与实时测点">
    <button class="btn" :disabled="syncing||syncLocked" @click="syncOnline"><CloudDownload/>{{syncing?'同步中':syncLocked?'请稍候':'线上同步'}}</button>
    <button class="btn primary" :disabled="!hasEnabledChannel" :title="hasEnabledChannel?'手工新增设备':'请先启用 RS485 通道'" @click="openCreate"><Plus/>手工新增</button>
  </PageHeader>
  <section class="split-layout device-workspace">
    <article class="panel device-list">
      <div class="channel-label staging-label"><strong>暂存区</strong><span>{{stagedMeters.length}} 台待绑定</span></div>
      <div v-if="!stagedMeters.length" class="empty-slot">线上同步的新设备会先放在这里</div>
      <button v-for="m in stagedMeters" :key="m.id" class="device-item staged" :class="{active:meter?.id===m.id}" @click="selected=m.id">
        <div><strong>{{m.sn}}</strong><StatusBadge :ok="false" text="暂存"/></div>
        <span class="icon-btn" title="查看详情" @click.stop="showDetail(m.id)"><Eye/></span>
        <span class="icon-btn danger" title="删除设备" @click.stop="askDelete(m.id)"><Trash2/></span>
      </button>
      <template v-for="group in grouped" :key="group.channel.id">
        <div class="channel-label"><strong>{{group.channel.name}}</strong><span>{{group.channel.port}} · {{group.meters.length?`${group.meters.length} 台设备 · 地址 ${group.meters.map(m=>m.address).join('/')}`:'空闲总线'}}</span><button class="icon-btn bind" title="绑定暂存设备" @click="askBind(group.channel.id)"><Link2/></button></div>
        <div v-if="!group.meters.length" class="empty-slot">未绑定设备</div>
        <button v-for="m in group.meters" :key="m.id" class="device-item" :class="{active:meter?.id===m.id}" @click="selected=m.id">
          <div><strong>{{m.sn}}</strong><StatusBadge :ok="m.online" :text="m.online?'在线':'离线'"/></div>
          <span class="icon-btn" title="查看详情" @click.stop="showDetail(m.id)"><Eye/></span>
          <span class="icon-btn danger" title="删除设备" @click.stop="askDelete(m.id)"><Trash2/></span>
        </button>
      </template>
    </article>
    <article class="panel point-panel">
      <div class="panel-title"><div><h2>{{meter?.name??'请选择设备'}}</h2><p v-if="meter">{{meter.sn}} · {{meter.profile}} · {{meter.channelId}}</p></div><div class="panel-actions"><button v-if="meter?.enabled" class="btn" :class="{primary:!meter.continuousPull,danger:meter.continuousPull}" :disabled="pulling" @click="togglePull"><component :is="meter.continuousPull?Square:Play"/>{{meter.continuousPull?'停止拉取':'开始拉取'}}</button><button v-if="meter?.configSource==='LOCAL'" class="btn" @click="openEdit">编辑配置</button></div></div>
      <div v-if="meter" class="device-meta-strip"><span>物模型 {{meter.modelVersion||'local-v1'}}</span><span>{{meter.configSource==='PLATFORM'?'线上同步':'仅本地'}}</span><span>{{meter.uploadEnabled?'允许上报':'暂不上报'}}</span><span>最近采集 {{meter.lastRead}}</span></div>
      <section v-if="meter?.points.length" class="point-grid dynamic-points"><div v-for="p in meter.points" :key="p.code" class="point-card"><span class="point-name">{{p.name}}</span><strong class="point-value" :style="pointValueStyle(p.value)">{{displayPointValue(p.value)}}</strong><small class="point-unit">{{p.unit}}</small></div></section>
      <div v-else class="empty-state"><span>◇</span><strong>暂无实时测点</strong></div>
    </article>
  </section>

  <Teleport to="body"><div v-if="editing" class="dialog-mask" @click.self="editing=false"><section class="dialog editor-dialog manual-device-dialog"><header><strong>{{meter&&form.deviceSn===meter.sn?'编辑设备':'手工新增设备'}}</strong><button @click="editing=false"><X/></button></header><div class="manual-form">
    <section><h3>设备身份</h3><div class="form-grid"><label><span>设备名称</span><input v-model.trim="form.deviceName" placeholder="例如：1号进线电表"/></label><label><span>设备 SN</span><input v-model.trim="form.deviceSn" placeholder="请输入唯一设备编号"/></label></div></section>
    <section><h3>采集绑定</h3><div class="form-grid"><label><span>物模型</span><SideSelect v-model="form.profile" :options="modelOptions" /></label><label><span>RS485 通道</span><SideSelect v-model="form.channelId" :options="channelOptions" /></label></div></section>
    <section><h3>轮询参数</h3><div class="form-grid"><label><span>Modbus 地址</span><input v-model.number="form.modbusAddr" type="number" min="1" max="247"/><small v-if="addressConflict.get(`${form.channelId}:${form.modbusAddr}`)" class="field-error">该地址已被 {{addressConflict.get(`${form.channelId}:${form.modbusAddr}`)?.sn}} 使用</small></label><label><span>采集周期（秒）</span><input v-model.number="form.collectIntervalS" type="number" min="1"/></label></div></section>
  </div><footer><button class="btn" @click="editing=false">取消</button><button class="btn primary" :disabled="saving" @click="submit">{{saving?'保存中':'保存设备'}}</button></footer></section></div></Teleport>
  <Teleport to="body"><div v-if="deleting" class="dialog-mask" @click.self="deleting=false"><section class="dialog danger confirm-dialog"><header><AlertTriangle/><strong>确认删除设备</strong><button @click="deleting=false"><X/></button></header><p>删除后网关会停止该设备采集，并清理本地最新读数和历史样本。线上设备如果平台仍下发，下一次同步可能会重新出现。</p><footer><button class="btn" @click="deleting=false">取消</button><button class="btn danger" :disabled="saving" @click="confirmDelete">{{saving?'删除中':'确认删除'}}</button></footer></section></div></Teleport>
  <Teleport to="body"><div v-if="detailOpen" class="dialog-mask" @click.self="detailOpen=false"><section class="dialog detail-dialog"><header><Eye/><strong>设备详情</strong><button @click="detailOpen=false"><X/></button></header><dl v-if="detailMeter" class="detail-grid"><div><dt>设备名称</dt><dd>{{detailMeter.name}}</dd></div><div><dt>设备 SN</dt><dd>{{detailMeter.sn}}</dd></div><div><dt>绑定通道</dt><dd>{{detailMeter.channelId}}</dd></div><div><dt>Modbus 地址</dt><dd>{{detailMeter.address}}</dd></div><div><dt>物模型</dt><dd>{{detailMeter.profile}}</dd></div><div><dt>配置来源</dt><dd>{{detailMeter.configSource==='PLATFORM'?'线上同步':'本地手工'}}</dd></div><div><dt>上报状态</dt><dd>{{detailMeter.uploadEnabled?'允许上报':'暂不上报'}}</dd></div><div><dt>采集周期</dt><dd>{{detailMeter.collectIntervalS}} 秒</dd></div><div><dt>最近采集</dt><dd>{{detailMeter.lastRead}}</dd></div><div><dt>在线状态</dt><dd>{{detailMeter.online?'在线':'离线'}}</dd></div></dl><footer><button class="btn primary" @click="detailOpen=false">知道了</button></footer></section></div></Teleport>
  <Teleport to="body"><div v-if="binding" class="dialog-mask" @click.self="binding=false"><section class="dialog confirm-dialog"><header><Link2/><strong>绑定到 {{bindChannelId}}</strong><button @click="binding=false"><X/></button></header><p>确认后，选中的暂存设备会加入这条 RS485 总线；同一总线允许多台设备，但 Modbus 地址不能重复。</p><footer><button class="btn" @click="binding=false">取消</button><button class="btn primary" :disabled="saving" @click="confirmBind">{{saving?'绑定中':'确认绑定'}}</button></footer></section></div></Teleport>
</div></template>
