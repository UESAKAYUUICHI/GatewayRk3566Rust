<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
import { Activity, GitCompareArrows, Flame, PieChart } from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'

const tabs = [
  { label: '能耗趋势', path: '/overview/energy/trend', icon: Activity },
  { label: '负荷构成', path: '/overview/energy/load', icon: PieChart },
  { label: '设备对比', path: '/overview/energy/compare', icon: GitCompareArrows },
  { label: '异常热力', path: '/overview/energy/heatmap', icon: Flame },
]
</script>

<template>
  <div class="page energy-page">
    <PageHeader title="能源可视" subtitle="以图表方式观察网关侧能耗、负荷、设备差异与异常分布">
      <nav class="third-nav energy-tabs">
        <RouterLink v-for="tab in tabs" :key="tab.path" :to="tab.path">
          <component :is="tab.icon" />
          <span>{{ tab.label }}</span>
        </RouterLink>
      </nav>
    </PageHeader>
    <RouterView />
  </div>
</template>
