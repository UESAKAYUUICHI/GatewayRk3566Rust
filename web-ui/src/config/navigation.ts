import { Activity, BellRing, Boxes, Cable, Camera, ChartSpline, CloudUpload, Cpu, DatabaseZap, FileClock, Gauge, ListChecks, Network, Radio, Settings2, ShieldAlert, type LucideIcon } from 'lucide-vue-next'

export interface NavItem{label:string;hint:string;path:string;icon:LucideIcon}
export interface NavGroup{label:string;code:string;path:string;icon:LucideIcon;children:NavItem[]}
export const navigation:NavGroup[]=[
  {label:'运行总览',code:'OVERVIEW',path:'/overview',icon:Gauge,children:[
    {label:'网关概览',hint:'实时状态与快捷运维',path:'/overview/dashboard',icon:Activity},
    {label:'能源可视',hint:'趋势、负荷与异常分析',path:'/overview/energy',icon:ChartSpline},
    {label:'告警中心',hint:'当前异常与历史告警',path:'/overview/alarms',icon:BellRing},
    {label:'摄像头测试',hint:'本地摄像头实时画面',path:'/overview/camera',icon:Camera},
  ]},
  {label:'设备采集',code:'COLLECTION',path:'/collection',icon:Radio,children:[
    {label:'设备监控',hint:'RS485 设备与实时测点',path:'/collection/devices',icon:Cpu},
    {label:'历史采集数据',hint:'本地原始采样与测点详情',path:'/collection/history',icon:DatabaseZap},
    {label:'串口通道配置',hint:'RS485、MQTT 与缓存',path:'/collection/channels',icon:Cable},
    {label:'物模型',hint:'设备语义与寄存器定义',path:'/collection/profiles',icon:Boxes},
  ]},
  {label:'上报中心',code:'REPORT',path:'/report',icon:CloudUpload,children:[
    {label:'上报记录',hint:'Outbox 与平台消费结果',path:'/report/uploads',icon:FileClock},
    {label:'指令接收',hint:'平台下发与执行结果',path:'/report/commands',icon:ListChecks},
    {label:'告警规则同步',hint:'平台规则与本地规则',path:'/report/alarm-rules/platform',icon:ShieldAlert},
  ]},
  {label:'系统运维',code:'SYSTEM',path:'/system',icon:Settings2,children:[
    {label:'网络配置',hint:'Wi-Fi、LAN 与地址信息',path:'/system/network',icon:Network},
    {label:'系统信息',hint:'版本、资源与诊断',path:'/system/info',icon:Gauge},
  ]},
]
