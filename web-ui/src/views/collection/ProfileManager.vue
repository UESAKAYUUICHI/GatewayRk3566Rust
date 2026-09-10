<script setup lang="ts">
import {computed,onMounted,ref} from 'vue'
import {Trash2,X} from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import {deleteThingModel,fetchThingModels,syncPlatformConfig} from '../../services/gateway-api'
import type {ThingModel} from '../../types/gateway'
import {useDialog} from '../../composables/useDialog'
import {useToast} from '../../composables/useToast'
const models=ref<ThingModel[]>([]),selected=ref(''),syncing=ref(false),deleting=ref(false),deleteTarget=ref<ThingModel|null>(null),{show}=useDialog()
const toast=useToast()
const current=computed(()=>models.value.find(m=>m.profile===(selected.value||models.value[0]?.profile)))
async function load(){models.value=await fetchThingModels()}
async function sync(){syncing.value=true;try{const result=await syncPlatformConfig();await load();toast.success(`物模型同步完成：已应用 ${result.models} 个`)}catch(e){show('同步失败',e instanceof Error?e.message:'无法同步物模型',true)}finally{syncing.value=false}}
function askDelete(model:ThingModel){deleteTarget.value=model;deleting.value=true}
async function confirmDelete(){if(!deleteTarget.value)return;const profile=deleteTarget.value.profile;syncing.value=true;try{await deleteThingModel(profile);deleting.value=false;deleteTarget.value=null;selected.value='';await load();toast.success(`物模型 ${profile} 已删除`)}catch(e){show('删除失败',e instanceof Error?e.message:'物模型无法删除，可能仍被设备占用',true)}finally{syncing.value=false}}
onMounted(()=>load().catch(e=>show('读取失败',e instanceof Error?e.message:'无法读取物模型',true)))
</script>
<template><div class="page model-page"><PageHeader title="物模型"><button class="btn primary" :disabled="syncing" @click="sync">{{syncing?'同步中':'从平台同步'}}</button></PageHeader>
  <section class="split-layout model-workspace"><article class="panel model-list"><button v-for="m in models" :key="m.profile" class="model-item" :class="{active:current?.profile===m.profile}" @click="selected=m.profile"><div class="model-item-main"><strong>{{m.name}}</strong><small>{{m.profile}} · {{m.version}}</small><div class="model-item-meta"><StatusBadge :ok="m.pointCount>0" :text="m.source==='PLATFORM'?'线上':'本地'"/><span>{{m.pointCount}} 点 · {{m.deviceCount}} 台</span></div></div><span class="icon-btn danger" title="删除物模型" @click.stop="askDelete(m)"><Trash2/></span></button></article>
  <article class="panel model-detail"><div class="panel-title"><div><h2>{{current?.name??'暂无物模型'}}</h2><p v-if="current">{{current.profile}} · {{current.version}}</p></div><StatusBadge v-if="current" :ok="!!current.pointCount" :text="current.pointCount?'可采集':'缺少采集点'"/></div>
    <div class="table-wrap fill-table model-point-table"><table><thead><tr><th>测点</th><th>编码</th><th>寄存器</th><th>类型</th><th>换算</th></tr></thead><tbody><tr v-for="p in current?.points" :key="p.code"><td>{{p.name}} <small>{{p.unit}}</small></td><td>{{p.code}}</td><td>FC{{p.functionCode}} · {{p.address}} / {{p.quantity}}</td><td>{{p.dataType}}</td><td>× {{p.scale}} + {{p.offset}}</td></tr></tbody></table></div>
  </article></section>
  <Teleport to="body"><div v-if="deleting" class="dialog-mask" @click.self="deleting=false"><section class="dialog confirm-dialog"><header><Trash2/><strong>删除物模型</strong><button @click="deleting=false"><X/></button></header><p>确认删除 {{deleteTarget?.name}}？如果已有设备正在使用该物模型，系统会拒绝删除，避免采集测点丢失。</p><footer><button class="btn" @click="deleting=false">取消</button><button class="btn danger" :disabled="syncing" @click="confirmDelete">{{syncing?'删除中':'确认删除'}}</button></footer></section></div></Teleport>
</div></template>
