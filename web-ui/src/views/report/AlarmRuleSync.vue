<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
import { CloudDownload, ShieldAlert } from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'

const tabs = [
  { label: '平台同步规则', path: '/report/alarm-rules/platform', icon: CloudDownload },
  { label: '本地告警规则', path: '/report/alarm-rules/local', icon: ShieldAlert },
]
</script>

<template>
  <div class="page alarm-rule-shell">
    <PageHeader title="告警规则同步" subtitle="平台规则锁定只读，本地规则由网关独立维护">
      <nav class="third-nav alarm-rule-tabs">
        <RouterLink v-for="tab in tabs" :key="tab.path" :to="tab.path">
          <component :is="tab.icon" />
          <span>{{ tab.label }}</span>
        </RouterLink>
      </nav>
    </PageHeader>
    <RouterView />
  </div>
</template>
