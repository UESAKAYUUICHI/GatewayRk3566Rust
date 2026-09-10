<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import PageHeader from '../../components/PageHeader.vue'
import SideSelect, { type SideSelectOption } from '../../components/SideSelect.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import { gatewayAction } from '../../services/gateway-api'
import { useGatewayStore } from '../../stores/gateway'
import { useDialog } from '../../composables/useDialog'

const store = useGatewayStore()
const { show } = useDialog()
const level = ref('ALL')
const page = ref(1)
const pageSize = ref(10)
const confirming = ref(false)
const levelOptions: SideSelectOption[] = [
  { label: '全部级别', value: 'ALL' },
  { label: 'ERROR', value: 'ERROR' },
  { label: 'WARN', value: 'WARN' },
  { label: 'INFO', value: 'INFO' },
]
const pageSizeOptions: SideSelectOption[] = [
  { label: '8', value: 8 },
  { label: '10', value: 10 },
  { label: '15', value: 15 },
]
const events = computed(() => store.snapshot?.events ?? [])
const rows = computed(() => level.value === 'ALL' ? events.value : events.value.filter(e => e.level === level.value))
const activeCount = computed(() => events.value.filter(event => event.status === 'ACTIVE').length)
const pageCount = computed(() => Math.max(1, Math.ceil(rows.value.length / pageSize.value)))
const pagedRows = computed(() => rows.value.slice((page.value - 1) * pageSize.value, page.value * pageSize.value))
const count = (value: string) => events.value.filter(x => x.level === value).length
const statusText = (value: string) => value === 'ACTIVE' ? '活动' : value === 'RECOVERED' ? '已恢复' : value === 'ACKED' ? '已确认' : '记录'

async function acknowledgeAll() {
  confirming.value = true
  try {
    await gatewayAction('/v1/events/acknowledge-all')
    await store.refresh()
    show('确认完成', '当前活动告警已标记为已确认。')
  } catch (error) {
    show('确认失败', error instanceof Error ? error.message : '告警确认失败', true)
  } finally {
    confirming.value = false
  }
}

watch([level, pageSize], () => { page.value = 1 })
watch(pageCount, value => { if (page.value > value) page.value = value })
</script>

<template>
  <div class="page alarm-page">
    <PageHeader title="告警中心">
      <SideSelect v-model="level" :options="levelOptions" />
      <button class="btn" :disabled="confirming || activeCount===0" @click="acknowledgeAll">{{confirming ? '确认中' : `全部确认 ${activeCount}`}}</button>
    </PageHeader>

    <section class="metric-grid three">
      <article class="metric"><span>严重告警</span><strong>{{count('ERROR')}}</strong><small>需要现场处理</small></article>
      <article class="metric"><span>一般告警</span><strong>{{count('WARN')}}</strong><small>建议关注</small></article>
      <article class="metric"><span>信息事件</span><strong>{{count('INFO')}}</strong><small>运行记录</small></article>
    </section>

    <article class="panel alarm-panel">
      <div class="panel-title">
        <div><h2>告警与事件</h2></div>
        <div class="pager-size">
          <span>每页</span>
          <SideSelect v-model="pageSize" :options="pageSizeOptions" />
        </div>
      </div>
      <div class="table alarm-table">
        <div class="tr th"><span>时间</span><span>级别</span><span>来源</span><span class="wide">内容</span><span>状态</span></div>
        <div v-for="e in pagedRows" :key="e.id" class="tr"><span>{{e.time}}</span><span><StatusBadge :ok="e.level==='INFO'" :text="e.level"/></span><span>{{e.source}}</span><span class="wide">{{e.message}}</span><span>{{statusText(e.status)}}<small v-if="e.delivery && e.delivery !== '—'">{{e.delivery}}</small></span></div>
        <div v-if="!pagedRows.length" class="empty-row">暂无告警事件</div>
      </div>
      <footer class="pagination">
        <span>共 {{rows.length}} 条 · 第 {{page}} / {{pageCount}} 页</span>
        <div>
          <button class="btn" :disabled="page<=1" @click="page--">上一页</button>
          <button class="btn" :disabled="page>=pageCount" @click="page++">下一页</button>
        </div>
      </footer>
    </article>
  </div>
</template>
