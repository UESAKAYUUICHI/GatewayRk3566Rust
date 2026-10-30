<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { RefreshCw } from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'
import SideSelect, { type SideSelectOption } from '../../components/SideSelect.vue'
import StatusBadge from '../../components/StatusBadge.vue'
import { fetchCommandRecords } from '../../services/gateway-api'
import type { CommandRecord } from '../../types/gateway'
import { useDialog } from '../../composables/useDialog'
import { useGatewayStore } from '../../stores/gateway'

const store = useGatewayStore()
const { show } = useDialog()
const rows = ref<CommandRecord[]>([])
const loading = ref(false)
const page = ref(1)
const pageSize = ref(80)
const total = ref(0)
const pageSizeOptions: SideSelectOption[] = [
  { label: '50', value: 50 },
  { label: '80', value: 80 },
  { label: '120', value: 120 },
  { label: '200', value: 200 },
]
const pageCount = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)))

async function load() {
  loading.value = true
  try {
    const result = await fetchCommandRecords(page.value, pageSize.value)
    rows.value = result.rows
    total.value = result.total
  } catch (error) {
    show('读取失败', error instanceof Error ? error.message : '指令记录读取失败', true)
  } finally {
    loading.value = false
  }
}

function statusText(status: string) {
  if (status === 'SUCCESS') return '成功'
  if (status === 'PENDING') return '处理中'
  if (status === 'FAILED') return '失败'
  return status
}

watch(pageSize, () => { page.value = 1; load() })
watch(page, () => load())
watch(pageCount, value => { if (page.value > value) page.value = value })
onMounted(load)
</script>

<template>
  <div class="page report-page command-page">
    <PageHeader title="指令接收">
      <label class="pager-size">显示<SideSelect v-model="pageSize" :options="pageSizeOptions" /></label>
      <button class="icon-only refresh-action" title="刷新指令" :disabled="loading" @click="load"><RefreshCw :class="{spin:loading}" /></button>
    </PageHeader>
    <article class="panel table-panel">
      <div class="panel-title"><div><h2>平台指令</h2><p>按接收时间倒序分页读取</p></div><StatusBadge :ok="store.snapshot?.cloudOnline" :text="`${total} 条`"/></div>
      <div class="table six fill-table">
        <div class="tr th"><span>接收时间</span><span>指令编号</span><span>类型</span><span>目标</span><span>状态</span><span>结果</span></div>
        <div v-for="c in rows" :key="c.id" class="tr"><span>{{c.time}}</span><span>{{c.id}}</span><span>{{c.type}}</span><span>{{c.target || '--'}}</span><span><StatusBadge :ok="c.status==='SUCCESS'" :text="statusText(c.status)"/></span><span>{{c.message || '--'}}</span></div>
        <div v-if="!rows.length" class="empty-row">暂无指令记录。</div>
      </div>
      <footer class="pagination">
        <span>共 {{ total }} 条 · 第 {{ page }} / {{ pageCount }} 页</span>
        <div>
          <button class="btn" :disabled="page<=1 || loading" @click="page--">上一页</button>
          <button class="btn" :disabled="page>=pageCount || loading" @click="page++">下一页</button>
        </div>
      </footer>
    </article>
  </div>
</template>
