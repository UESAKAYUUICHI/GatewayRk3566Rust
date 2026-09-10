<script setup lang="ts">
import { computed, onMounted, reactive, watchEffect } from 'vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'
import { Clock3, Cloud, RefreshCw } from 'lucide-vue-next'
import { navigation } from '../config/navigation'
import { useGatewayStore } from '../stores/gateway'
import AppDialog from '../components/AppDialog.vue'
import AppToast from '../components/AppToast.vue'

const route=useRoute(),store=useGatewayStore()
const activeGroup=computed(()=>navigation.find(g=>route.path.startsWith(g.path))??navigation[0])
type DisplayConfig={mode?:'native-portrait'|'native-landscape'|'landscape-in-portrait'|'landscape-in-portrait-inverted';designWidth?:number;designHeight?:number}
const display=reactive({mode:'native-portrait' as DisplayConfig['mode'],designWidth:1280,designHeight:800})
const displayStyle=computed(()=>({
  '--design-width': `${display.designWidth}px`,
  '--design-height': `${display.designHeight}px`,
}))
watchEffect(()=>{
  document.body.dataset.displayMode=display.mode??'native-portrait'
  document.body.style.setProperty('--design-width', `${display.designWidth}px`)
  document.body.style.setProperty('--design-height', `${display.designHeight}px`)
})
onMounted(async()=>{
  try{
    const response=await fetch('/display-config.json',{cache:'no-store'})
    if(response.ok){
      const config=await response.json() as DisplayConfig
      if(config.mode)display.mode=config.mode
      if(config.designWidth)display.designWidth=config.designWidth
      if(config.designHeight)display.designHeight=config.designHeight
    }
  }catch{/* 使用内置竖屏显示配置 */}
  store.refresh();store.connectRealtime()
})
</script>

<template>
  <div class="display-root" :class="`display-${display.mode}`" :style="displayStyle">
    <div class="display-stage">
      <div class="app-shell">
        <header class="topbar">
          <RouterLink to="/overview/dashboard" class="brand"><img src="/gateway-logo.png" alt="平台 Logo"><div><strong>智能园管AI采集边缘网关系统</strong><small>EDGE GATEWAY · {{store.snapshot?.gatewaySn??'正在读取'}}</small></div></RouterLink>
          <nav class="top-nav"><RouterLink v-for="item in navigation" :key="item.path" :to="item.children[0].path" :class="{active:route.path.startsWith(item.path)}"><component :is="item.icon"/><span>{{item.label}}</span><small>{{item.code}}</small></RouterLink></nav>
          <div class="top-status"><span><Cloud/>{{store.snapshot?.cloudOnline?'平台在线':'平台离线'}}</span><span><Clock3/>{{store.snapshot?.lastPublish??'--:--:--'}}</span><button class="icon-only refresh-action" title="刷新" @click="store.refresh"><RefreshCw :class="{spin:store.loading}"/></button></div>
        </header>
        <div class="workspace">
          <aside class="sidebar"><div class="side-title"><span>{{activeGroup.code}}</span><strong>{{activeGroup.label}}</strong></div><nav><RouterLink v-for="item in activeGroup.children" :key="item.path" :to="item.path"><component :is="item.icon"/><div><strong>{{item.label}}</strong><small>{{item.hint}}</small></div></RouterLink></nav><div class="device-foot">v{{store.snapshot?.version??'--'}} · copyright@hukaidong</div></aside>
          <main class="content"><RouterView/></main>
        </div>
      </div>
      <AppDialog/>
      <AppToast/>
    </div>
  </div>
</template>
