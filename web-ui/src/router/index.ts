import { createRouter, createWebHashHistory } from 'vue-router'

export default createRouter({history:createWebHashHistory(),routes:[
  {path:'/',redirect:'/overview/dashboard'},
  {path:'/overview/dashboard',component:()=>import('../views/overview/OverviewDashboard.vue')},
  {path:'/overview/energy',component:()=>import('../views/overview/EnergyVisual.vue'),children:[
    {path:'',redirect:'/overview/energy/trend'},
    {path:'trend',component:()=>import('../views/overview/energy/EnergyTrend.vue')},
    {path:'load',component:()=>import('../views/overview/energy/LoadComposition.vue')},
    {path:'compare',component:()=>import('../views/overview/energy/DeviceCompare.vue')},
    {path:'heatmap',component:()=>import('../views/overview/energy/AnomalyHeatmap.vue')},
  ]},
  {path:'/overview/alarms',component:()=>import('../views/overview/AlarmCenter.vue')},
  {path:'/overview/camera',component:()=>import('../views/overview/CameraTest.vue')},
  {path:'/collection/devices',component:()=>import('../views/collection/DeviceMonitor.vue')},
  {path:'/collection/history',component:()=>import('../views/collection/CollectHistory.vue')},
  {path:'/collection/channels',component:()=>import('../views/collection/ChannelManager.vue'),children:[
    {path:'',redirect:'/collection/channels/serial'},
    {path:'serial',component:()=>import('../views/collection/channels/SerialChannelConfig.vue')},
    {path:'mqtt',component:()=>import('../views/collection/channels/MqttPlatformConfig.vue')},
    {path:'cache',component:()=>import('../views/collection/channels/EdgeCacheConfig.vue')},
  ]},
  {path:'/collection/profiles',component:()=>import('../views/collection/ProfileManager.vue')},
  {path:'/report/uploads',component:()=>import('../views/report/UploadRecords.vue')},
  {path:'/report/commands',component:()=>import('../views/report/CommandInbox.vue')},
  {path:'/report/alarm-rules',component:()=>import('../views/report/AlarmRuleSync.vue'),children:[
    {path:'',redirect:'/report/alarm-rules/platform'},
    {path:'platform',component:()=>import('../views/report/PlatformAlarmRules.vue')},
    {path:'local',component:()=>import('../views/report/LocalAlarmRules.vue')},
  ]},
  {path:'/system/network',component:()=>import('../views/system/NetworkSettings.vue')},
  {path:'/system/info',component:()=>import('../views/system/SystemInfo.vue')},
]})
