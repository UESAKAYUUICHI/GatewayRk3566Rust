import type { AlarmRuleConfig, CameraDevice, CollectHistoryPage, GatewaySnapshot, Meter, Rs485Channel, SystemEnvironment, ThingModel, UploadRecord } from '../types/gateway'

const API_BASE=import.meta.env.VITE_GATEWAY_API_URL??'/api'
export interface PlatformConfig {gatewayId:string;gatewaySn:string;platformHttpUrl:string;mqttHost:string;mqttPort:number;mqttUsername:string;passwordConfigured:boolean;heartbeatS:number}
export interface PowerPoint {time:string;timestampMs:number;deviceSn:string;deviceName:string;point:string;value:number;unit:string}
export async function fetchSnapshot():Promise<GatewaySnapshot>{
  const response=await fetch(`${API_BASE}/v1/snapshot`,{signal:AbortSignal.timeout(5000)})
  if(!response.ok)throw await apiError(response)
  return await response.json()
}
export async function fetchPower24h():Promise<PowerPoint[]>{
  const response=await fetch(`${API_BASE}/v1/metrics/power24h`,{signal:AbortSignal.timeout(5000)})
  if(!response.ok)throw await apiError(response)
  return await response.json()
}
export async function fetchSystemEnvironment():Promise<SystemEnvironment>{
  const response=await fetch(`${API_BASE}/v1/system/environment`,{signal:AbortSignal.timeout(5000)})
  if(!response.ok)throw await apiError(response)
  return await response.json()
}
export async function fetchSystemDiagnostics():Promise<Blob>{
  const response=await fetch(`${API_BASE}/v1/system/diagnostics`,{signal:AbortSignal.timeout(10000)})
  if(!response.ok)throw await apiError(response)
  return await response.blob()
}
export async function fetchCameraDevices():Promise<CameraDevice[]>{
  const response=await fetch(`${API_BASE}/v1/camera/devices`,{signal:AbortSignal.timeout(5000)})
  if(!response.ok)throw await apiError(response)
  return await response.json()
}
export function cameraStreamUrl(device?:string):string{
  const params = new URLSearchParams()
  if(device)params.set('device',device)
  params.set('width','640')
  params.set('height','480')
  params.set('fps','10')
  return `${API_BASE}/v1/camera/stream?${params.toString()}`
}
export function cameraSnapshotUrl(device?:string):string{
  const params = new URLSearchParams()
  if(device)params.set('device',device)
  params.set('width','640')
  params.set('height','480')
  params.set('fps','10')
  return `${API_BASE}/v1/camera/snapshot?${params.toString()}`
}
export async function fetchAlarmRules():Promise<AlarmRuleConfig>{const r=await fetch(`${API_BASE}/v1/alarm-rules`);if(!r.ok)throw await apiError(r);return r.json()}
export async function fetchCollectHistory(pageNum=1,pageSize=80):Promise<CollectHistoryPage>{const r=await fetch(`${API_BASE}/v1/collection/history?pageNum=${pageNum}&pageSize=${pageSize}`);if(!r.ok)throw await apiError(r);return r.json()}
export async function fetchUploadRecords(limit=200):Promise<UploadRecord[]>{const r=await fetch(`${API_BASE}/v1/uploads?limit=${limit}`);if(!r.ok)throw await apiError(r);return r.json()}
export async function syncAlarmRules():Promise<AlarmRuleConfig>{const r=await fetch(`${API_BASE}/v1/alarm-rules/sync`,{method:'POST'});if(!r.ok)throw await apiError(r);return r.json()}
export async function saveAlarmRuleSyncConfig(syncIntervalS:number):Promise<void>{const r=await fetch(`${API_BASE}/v1/alarm-rules/config`,{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({syncIntervalS})});if(!r.ok)throw await apiError(r)}
export async function saveAlarmRule(id:number|null,body:unknown):Promise<void>{const r=await fetch(`${API_BASE}/v1/alarm-rules${id?`/${id}`:''}`,{method:id?'PUT':'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});if(!r.ok)throw await apiError(r)}
export async function deleteAlarmRule(id:number):Promise<void>{const r=await fetch(`${API_BASE}/v1/alarm-rules/${id}`,{method:'DELETE'});if(!r.ok)throw await apiError(r)}
export async function gatewayAction(path:string,body:unknown={}):Promise<void>{
  const response=await fetch(`${API_BASE}${path}`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)})
  if(!response.ok)throw await apiError(response)
}
export async function fetchMeters():Promise<Meter[]>{const r=await fetch(`${API_BASE}/v1/meters`);if(!r.ok)throw await apiError(r);return r.json()}
export async function saveMeter(id:number|null,body:unknown):Promise<void>{const r=await fetch(`${API_BASE}/v1/meters${id?`/${id}`:''}`,{method:id?'PUT':'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});if(!r.ok)throw await apiError(r)}
export async function deleteMeter(id:number):Promise<void>{const r=await fetch(`${API_BASE}/v1/meters/${id}`,{method:'DELETE'});if(!r.ok)throw await apiError(r)}
export async function bindMeter(id:number,channelId:string):Promise<void>{const r=await fetch(`${API_BASE}/v1/meters/${id}/bind`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({channelId})});if(!r.ok)throw await apiError(r)}
export async function setContinuousPull(id:number,enabled:boolean):Promise<void>{const r=await fetch(`${API_BASE}/v1/meters/${id}/continuous-pull`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({enabled})});if(!r.ok)throw await apiError(r)}
export async function fetchChannels():Promise<Rs485Channel[]>{const r=await fetch(`${API_BASE}/v1/channels`);if(!r.ok)throw await apiError(r);return r.json()}
export async function createChannel(body:unknown):Promise<Rs485Channel>{const r=await fetch(`${API_BASE}/v1/channels`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});if(!r.ok)throw await apiError(r);return r.json()}
export async function saveChannel(id:string,body:unknown):Promise<void>{const r=await fetch(`${API_BASE}/v1/channels/${encodeURIComponent(id)}`,{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});if(!r.ok)throw await apiError(r)}
export async function deleteChannel(id:string):Promise<void>{const r=await fetch(`${API_BASE}/v1/channels/${encodeURIComponent(id)}`,{method:'DELETE'});if(!r.ok)throw await apiError(r)}
export async function fetchThingModels():Promise<ThingModel[]>{const r=await fetch(`${API_BASE}/v1/thing-models`);if(!r.ok)throw await apiError(r);return r.json()}
export async function deleteThingModel(profile:string):Promise<void>{const r=await fetch(`${API_BASE}/v1/thing-models/${encodeURIComponent(profile)}`,{method:'DELETE'});if(!r.ok)throw await apiError(r)}
export async function syncPlatformConfig():Promise<{revision:string;devices:number;models:number;cloudAcknowledged:boolean}>{const r=await fetch(`${API_BASE}/v1/config/sync`,{method:'POST'});if(!r.ok)throw await apiError(r);return r.json()}
export async function fetchPlatformConfig():Promise<PlatformConfig>{
  const response=await fetch(`${API_BASE}/v1/platform`)
  if(!response.ok)throw await apiError(response)
  return await response.json()
}
export function connectSnapshotSocket(onSnapshot:(snapshot:GatewaySnapshot)=>void,onState:(connected:boolean)=>void):()=>void{
  let stopped=false,retry=0,socket:WebSocket|undefined,timer:number|undefined
  const open=()=>{
    const configured=import.meta.env.VITE_GATEWAY_WS_URL as string|undefined
    const url=configured??`${location.protocol==='https:'?'wss':'ws'}://${location.host}/api/v1/ws`
    socket=new WebSocket(url)
    socket.onopen=()=>{retry=0;onState(true)}
    socket.onmessage=event=>{try{const value=JSON.parse(event.data) as GatewaySnapshot;if(value.gatewayId)onSnapshot(value)}catch{/* 忽略无效帧 */}}
    socket.onclose=()=>{onState(false);if(!stopped)timer=window.setTimeout(open,Math.min(1000*2**retry++,10000))}
    socket.onerror=()=>socket?.close()
  }
  open()
  return()=>{stopped=true;if(timer)clearTimeout(timer);socket?.close()}
}
async function apiError(response:Response):Promise<Error>{
  try{const body=await response.json() as {message?:string};return new Error(body.message??`操作失败：${response.status}`)}catch{return new Error(`操作失败：${response.status}`)}
}

