<script setup lang="ts">
import {computed,onMounted,ref} from 'vue'
import {Layers,Network,RefreshCw} from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import {fetchThingModels,syncPlatformConfig} from '../../services/gateway-api'
import type {ThingModel,ThingModelPoint} from '../../types/gateway'
import {useDialog} from '../../composables/useDialog'
import {useToast} from '../../composables/useToast'
type DetailMode='product'|'protocol'
const models=ref<ThingModel[]>([]),selected=ref(''),syncing=ref(false),detailMode=ref<DetailMode>('product'),{show}=useDialog()
const toast=useToast()
const platformModels=computed(()=>models.value.filter(model=>model.source==='PLATFORM'))
const current=computed(()=>platformModels.value.find(m=>m.profile===(selected.value||platformModels.value[0]?.profile)))
async function load(){models.value=await fetchThingModels();if(!platformModels.value.some(model=>model.profile===selected.value))selected.value=platformModels.value[0]?.profile??''}
async function sync(){syncing.value=true;try{const result=await syncPlatformConfig();await load();toast.success(`物模型同步完成：已应用 ${result.models} 个`)}catch(e){show('同步失败',e instanceof Error?e.message:'无法同步物模型',true)}finally{syncing.value=false}}
function mappingRange(point:ThingModelPoint){return `FC${point.functionCode} @${point.address}+${point.fieldOffset} / ${point.fieldQuantity} of ${point.quantity}`}
function bitRange(point:ThingModelPoint){return point.bitOffset==null?'--':`${point.bitOffset}:${point.bitLength??1}`}
onMounted(()=>load().catch(e=>show('读取失败',e instanceof Error?e.message:'无法读取物模型',true)))
</script>
<template><div class="page model-page"><PageHeader title="物模型"><button class="btn primary" :disabled="syncing" @click="sync"><RefreshCw :class="{spin:syncing}"/>{{syncing?'同步中':'从平台同步'}}</button></PageHeader>
  <section class="split-layout model-workspace"><article class="panel model-list"><button v-for="m in platformModels" :key="m.profile" class="model-item" :class="{active:current?.profile===m.profile}" @click="selected=m.profile"><div class="model-item-main"><strong>{{m.name}}</strong><small>{{m.version}}</small><div class="model-item-meta"><StatusBadge :ok="m.pointCount>0" text="产品模板"/><span>{{m.pointCount}} 点 · {{m.deviceCount}} 台</span></div></div></button><div v-if="!platformModels.length" class="empty-panel">暂无平台产品模板</div></article>
  <article class="panel model-detail"><div class="panel-title model-detail-title"><div><h2>{{current?.name??'暂无产品模板'}}</h2><p v-if="current">{{current.profile}} · {{current.version}}</p></div><div class="model-mode-tabs"><button :class="{active:detailMode==='product'}" @click="detailMode='product'"><Layers/>产品模板</button><button :class="{active:detailMode==='protocol'}" @click="detailMode='protocol'"><Network/>协议映射</button></div><StatusBadge v-if="current" :ok="!!current.pointCount" :text="current.pointCount?'可采集':'缺少采集点'"/></div>
    <div v-if="detailMode==='product'" class="table-wrap fill-table model-point-table product-point-table"><table><thead><tr><th>标准测点</th><th>测点编码</th><th>单位</th><th>数据类型</th><th>展示换算</th></tr></thead><tbody><tr v-for="p in current?.points" :key="p.code"><td>{{p.name||p.code}}</td><td>{{p.code}}</td><td>{{p.unit||'--'}}</td><td>{{p.dataType}}</td><td>× {{p.scale}} + {{p.offset}}</td></tr></tbody></table></div>
    <div v-else class="table-wrap fill-table model-point-table protocol-point-table"><table><thead><tr><th>协议字段</th><th>标准测点</th><th>读块</th><th>类型/字节序</th><th>位段</th></tr></thead><tbody><tr v-for="p in current?.points" :key="p.code"><td>{{p.name||p.code}}</td><td>{{p.code}} <small>{{p.unit||'--'}}</small></td><td>{{mappingRange(p)}}</td><td>{{p.dataType}} / {{p.byteOrder}}</td><td>{{bitRange(p)}}</td></tr></tbody></table></div>
  </article></section>
</div></template>
