import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { connectSnapshotSocket, fetchSnapshot } from '../services/gateway-api'
import type { GatewaySnapshot } from '../types/gateway'

export const useGatewayStore=defineStore('gateway',()=>{
  const snapshot=ref<GatewaySnapshot|null>(null);const loading=ref(false);const lastError=ref('');const realtimeConnected=ref(false);let stopRealtime:(()=>void)|undefined
const normalize=(value:GatewaySnapshot):GatewaySnapshot=>({
  ...value,
  runtime:value.runtime??{production:true,transport:'modbus-rtu',cloudLink:'mqtt-qos1',network:'networkmanager-nmcli'},
    meters:(value.meters??[]).map(m=>({...m,modelVersion:m.modelVersion??'',channelId:m.channelId??'rs485-1',configSource:m.configSource??'LOCAL',uploadEnabled:m.uploadEnabled??false,continuousPull:m.continuousPull??false,points:m.points??[]})),
    events:value.events??[],
    uploads:value.uploads??[],
    commands:value.commands??[],
    networks:value.networks??[],
  })
  const onlineMeters=computed(()=>snapshot.value?.meters?.filter(x=>x.online).length??0)
  async function refresh(){loading.value=true;lastError.value='';try{snapshot.value=normalize(await fetchSnapshot())}catch(e){lastError.value=e instanceof Error?e.message:'读取失败'}finally{loading.value=false}}
  function connectRealtime(){if(stopRealtime)return;stopRealtime=connectSnapshotSocket(value=>{snapshot.value=normalize(value);lastError.value=''},connected=>{realtimeConnected.value=connected})}
  return{snapshot,loading,lastError,realtimeConnected,onlineMeters,refresh,connectRealtime}
})
