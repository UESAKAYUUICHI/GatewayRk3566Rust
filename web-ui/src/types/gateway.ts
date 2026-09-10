export interface PointValue {code:string;name:string;value:number|null;unit:string;quality:number;collectTime:number}
export interface Meter { id:number; sn:string; name:string; profile:string;modelVersion:string;channelId:string;configSource:'LOCAL'|'PLATFORM';uploadEnabled:boolean;enabled:boolean;collectIntervalS:number;continuousPull:boolean; address:number; online:boolean; voltage:number; current:number; power:number; energy:number; lastRead:string;points:PointValue[] }
export interface Rs485Channel {id:string;name:string;port:string;baud:number;dataBits:number;stopBits:number;parity:string;enabled:boolean}
export interface ThingModelPoint {code:string;name:string;unit:string;functionCode:number;address:number;quantity:number;dataType:string;scale:number;offset:number}
export interface ThingModel {profile:string;name:string;version:string;source:'LOCAL'|'PLATFORM';pointCount:number;deviceCount:number;points:ThingModelPoint[]}
export interface GatewayEvent { id:string; time:string; level:'INFO'|'WARN'|'ERROR'; source:string; message:string; status:'ACTIVE'|'RECOVERED'|'ACKED'|'LOG'; delivery:string }
export interface UploadRecord { id:number; time:string; deviceSn:string; points:number; type:string; accessStatus:string; dataStatus:string; latency:number }
export interface CommandRecord { id:string; time:string; type:string; target:string; status:'SUCCESS'|'PENDING'|'FAILED'; message:string }
export interface WifiNetwork { ssid:string; signal:number; security:string; connected:boolean }
export interface AlarmRule {id:number;ruleCode:string;source:'LOCAL'|'PLATFORM';name:string;level:'INFO'|'WARN'|'ERROR'|string;targetDeviceSn:string|null;pointCode:string;operator:string;threshold:number;unit:string;durationS:number;enabled:boolean;locked:boolean;updated:string}
export interface AlarmRuleConfig {syncIntervalS:number;lastSync:string;rules:AlarmRule[]}
export interface CollectPoint {code:string;name:string;value:number;unit:string}
export interface CollectSample {id:number;date:string;time:string;timestampMs:number;meterId:number;deviceSn:string;deviceName:string;channelId:string;profile:string;modbusAddr:number;quality:number;qualityLabel:string;qualityStatus:'ok'|'warn'|'error'|string;pointCount:number;summary:CollectPoint[];points:CollectPoint[]}
export interface CollectHistoryPage {pageNum:number;pageSize:number;total:number;rows:CollectSample[]}
export interface SystemInfoItem {key:string;value:string}
export interface SystemEnvironment {hostname:string;osRelease:string;kernel:string;architecture:string;cpuModel:string;cpuCores:number;cpuTemperature:string;loadAverage:string;memory:SystemInfoItem[];storage:SystemInfoItem[];runtime:SystemInfoItem[];kernelParams:SystemInfoItem[];bootParams:string;thermalZones:SystemInfoItem[]}
export interface GatewaySnapshot { gatewayId:string; gatewaySn:string; version:string; uptime:string; cloudOnline:boolean; mqttLabel:string; clockTrusted:boolean; pending:number; sent:number; todayKwh:number; lastPublish:string; runtime:{production:boolean;transport:string;cloudLink:string;network:string};wifi:{connected:boolean;ssid:string;signal:number;ipv4:string;gateway:string;dns:string;interfaceName:string};meters:Meter[];events:GatewayEvent[];uploads:UploadRecord[];commands:CommandRecord[];networks:WifiNetwork[] }
