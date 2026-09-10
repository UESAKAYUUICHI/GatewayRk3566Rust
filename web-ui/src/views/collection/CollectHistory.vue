<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { Eye, RefreshCw, X } from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'
import SideSelect, { type SideSelectOption } from '../../components/SideSelect.vue'
import { fetchCollectHistory } from '../../services/gateway-api'
import type { CollectSample } from '../../types/gateway'
import { useDialog } from '../../composables/useDialog'

const { show } = useDialog()
const rows = ref<CollectSample[]>([])
const loading = ref(false)
const detail = ref<CollectSample | null>(null)
const pageSize = ref(80)
const page = ref(1)
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
    const result = await fetchCollectHistory(page.value, pageSize.value)
    rows.value = result.rows
    total.value = result.total
  } catch (e) {
    show('读取失败', e instanceof Error ? e.message : '历史采集数据读取失败', true)
  } finally {
    loading.value = false
  }
}

watch(pageSize, () => { page.value = 1; load() })
watch(page, () => load())
watch(pageCount, value => { if (page.value > value) page.value = value })
onMounted(load)
</script>

<template>
  <div class="page collect-history-page">
    <PageHeader title="历史采集数据">
      <label class="pager-size">显示
        <SideSelect v-model="pageSize" :options="pageSizeOptions" />
      </label>
      <button class="icon-only refresh-action" title="刷新" :disabled="loading" @click="load"><RefreshCw :class="{spin:loading}" /></button>
    </PageHeader>

    <article class="panel table-panel collect-history-panel">
      <div class="panel-title">
        <div><h2>本地采样记录</h2><p>采集值已按设备物模型完成过滤、命名和单位补齐</p></div>
        <StatusBadge :ok="total > 0" :text="`${total} 条`" />
      </div>
      <div class="table collect-history-table fill-table">
        <div class="tr th"><span>批次</span><span>采集日期</span><span>采集时间</span><span>设备</span><span>通道</span><span>地址</span><span>物模型</span><span>处理</span><span>测点</span><span>质量</span><span>详情</span></div>
        <div v-for="row in rows" :key="row.id" class="tr">
          <span>#{{ row.id }}</span>
          <span>{{ row.date }}</span>
          <span>{{ row.time }}</span>
          <span><strong>{{ row.deviceName }}</strong><small>{{ row.deviceSn }}</small></span>
          <span>{{ row.channelId }}</span>
          <span>{{ row.modbusAddr }}</span>
          <span>{{ row.profile }}</span>
          <span><i class="enum-pill ok">已聚合</i></span>
          <span>{{ row.pointCount }} 点</span>
          <span><i class="enum-pill" :class="row.qualityStatus">{{ row.qualityLabel }}</i></span>
          <span class="row-actions"><button class="icon-btn" title="查看测点详情" @click="detail = row"><Eye /></button></span>
        </div>
        <div v-if="!rows.length" class="empty-row">暂无历史采集数据</div>
      </div>
      <footer class="pagination">
        <span>共 {{ total }} 条 · 第 {{ page }} / {{ pageCount }} 页</span>
        <div>
          <button class="btn" :disabled="page<=1 || loading" @click="page--">上一页</button>
          <button class="btn" :disabled="page>=pageCount || loading" @click="page++">下一页</button>
        </div>
      </footer>
    </article>

    <Teleport to="body">
      <div v-if="detail" class="dialog-mask" @click.self="detail=null">
        <section class="dialog collect-detail-dialog">
          <header><Eye /><strong>采集测点详情</strong><button @click="detail=null"><X /></button></header>
          <dl class="detail-grid compact">
            <div><dt>设备</dt><dd>{{ detail.deviceName }}</dd></div>
            <div><dt>SN</dt><dd>{{ detail.deviceSn }}</dd></div>
            <div><dt>通道</dt><dd>{{ detail.channelId }}</dd></div>
            <div><dt>采集时间</dt><dd>{{ detail.time }}</dd></div>
          </dl>
          <div class="table-wrap collect-point-table">
            <table>
              <thead><tr><th>测点名称</th><th>编码</th><th>数值</th><th>单位</th></tr></thead>
              <tbody>
                <tr v-for="point in detail.points" :key="point.code">
                  <td>{{ point.name }}</td><td>{{ point.code }}</td><td>{{ Number(point.value).toFixed(2) }}</td><td>{{ point.unit }}</td>
                </tr>
              </tbody>
            </table>
          </div>
          <footer><button class="btn primary" @click="detail=null">关闭</button></footer>
        </section>
      </div>
    </Teleport>
  </div>
</template>
