<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
import { Cable, Database, ServerCog } from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'

const tabs = [
  { label: '串口通道', path: '/collection/channels/serial', icon: Cable },
  { label: 'MQTT 平台', path: '/collection/channels/mqtt', icon: ServerCog },
  { label: '边缘缓存', path: '/collection/channels/cache', icon: Database },
]
</script>

<template>
  <div class="page channel-page">
    <PageHeader title="串口通道配置" subtitle="RS485 采集、MQTT 上报与本地缓存分开配置，避免一个页面过挤">
      <nav class="third-nav">
        <RouterLink v-for="tab in tabs" :key="tab.path" :to="tab.path">
          <component :is="tab.icon" />
          <span>{{ tab.label }}</span>
        </RouterLink>
      </nav>
    </PageHeader>
    <RouterView />
  </div>
</template>
