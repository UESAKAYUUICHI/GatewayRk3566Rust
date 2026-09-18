<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'
import { Camera, RefreshCw, Video, VideoOff } from 'lucide-vue-next'
import PageHeader from '../../components/PageHeader.vue'
import { cameraStreamUrl, fetchCameraDevices } from '../../services/gateway-api'
import type { CameraDevice } from '../../types/gateway'

const devices = ref<CameraDevice[]>([])
const selectedPath = ref('')
const loading = ref(false)
const error = ref('')
const streamKey = ref(0)
const playing = ref(false)
const isStreaming = ref(false)

const selectedDevice = computed(() => devices.value.find(item => item.path === selectedPath.value) ?? devices.value[0])
const previewUrl = computed(() => isStreaming.value && selectedDevice.value ? cameraStreamUrl(selectedDevice.value.path) + `&t=${streamKey.value}` : '')

async function loadDevices() {
  loading.value = true
  error.value = ''
  playing.value = false
  try {
    const result = await fetchCameraDevices()
    devices.value = result
    selectedPath.value = result.find(item => item.primary)?.path ?? result[0]?.path ?? ''
    startStream()
    if (!result.length) error.value = '未发现可用摄像头设备'
  } catch (err) {
    error.value = err instanceof Error ? err.message : '摄像头设备读取失败'
    devices.value = []
  } finally {
    loading.value = false
  }
}

function reloadStream() {
  stopStream()
  window.setTimeout(startStream, 120)
}

function startStream() {
  if (!selectedDevice.value) return
  error.value = ''
  streamKey.value += 1
  isStreaming.value = true
  playing.value = false
}

function stopStream() {
  isStreaming.value = false
  playing.value = false
}

function selectDevice(path: string) {
  selectedPath.value = path
  reloadStream()
}

onMounted(loadDevices)
onBeforeUnmount(stopStream)
onBeforeRouteLeave(() => {
  stopStream()
})
</script>

<template>
  <section class="page camera-test-page">
    <PageHeader title="摄像头测试">
      <button class="btn secondary" :disabled="loading" @click="loadDevices">
        <RefreshCw :class="{spin:loading}"/>刷新设备
      </button>
    </PageHeader>

    <div class="camera-test-layout">
      <aside class="camera-side">
        <div class="camera-summary">
          <div class="camera-summary-icon"><Camera/></div>
          <div>
            <strong>{{ selectedDevice?.name ?? '未发现摄像头' }}</strong>
            <small>{{ selectedDevice?.path ?? '请检查 CSI/USB 摄像头连接' }}</small>
          </div>
        </div>
        <div class="camera-device-list">
          <button
            v-for="device in devices"
            :key="device.path"
            class="camera-device-item"
            :class="{active:selectedPath===device.path}"
            @click="selectDevice(device.path)"
          >
            <Video/>
            <span>
              <strong>{{ device.name }}</strong>
              <small>{{ device.path }}{{ device.primary ? ' · 主通道' : '' }}</small>
            </span>
          </button>
          <div v-if="!devices.length" class="camera-empty">
            <VideoOff/>
            <span>{{ error || '暂无摄像头设备' }}</span>
          </div>
        </div>
      </aside>

      <main class="camera-stage">
        <div class="camera-toolbar">
          <span :class="['camera-state', playing ? 'ok' : error ? 'bad' : 'wait']"></span>
          <strong>{{ playing ? '实时画面' : error ? '连接异常' : '等待画面' }}</strong>
          <button class="btn secondary" :disabled="!selectedDevice" @click="reloadStream"><RefreshCw/>重载画面</button>
        </div>
        <div class="camera-frame">
          <img
            v-if="selectedDevice && previewUrl"
            :key="streamKey"
            :src="previewUrl"
            alt="摄像头实时画面"
            @load="playing=true;error=''"
            @error="playing=false;error='摄像头画面打开失败，请检查权限、占用或格式'"
          >
          <div v-else class="camera-empty large">
            <VideoOff/>
            <span>{{ error || '没有可播放的摄像头' }}</span>
          </div>
        </div>
        <p v-if="error" class="camera-error">{{ error }}</p>
      </main>
    </div>
  </section>
</template>
