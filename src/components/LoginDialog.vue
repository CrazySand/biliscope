<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { ElButton, ElDialog } from 'element-plus'
import { LoaderCircle, RefreshCw, ShieldCheck } from '@lucide/vue'
import QrcodeVue from 'qrcode.vue'
import { api } from '../api'

const props = defineProps<{ modelValue: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean]; loggedIn: [] }>()

const loading = ref(false)
const qrUrl = ref('')
const key = ref('')
const status = ref(86101)
const message = ref('正在获取二维码…')
const error = ref('')
const polling = ref(false)
let timer: number | undefined

const expired = computed(() => status.value === 86038)
const statusText = computed(() => error.value || message.value)

function stopPolling() {
  if (timer !== undefined) {
    window.clearInterval(timer)
    timer = undefined
  }
}

async function start() {
  stopPolling()
  loading.value = true
  error.value = ''
  qrUrl.value = ''
  key.value = ''
  status.value = 86101
  message.value = '正在获取二维码…'
  try {
    const qr = await api.qrGenerate()
    qrUrl.value = qr.url
    key.value = qr.key
    message.value = '等待扫码'
    startPolling()
  } catch (cause) {
    error.value = String(cause)
  } finally {
    loading.value = false
  }
}

function startPolling() {
  stopPolling()
  timer = window.setInterval(() => void poll(), 1500)
}

async function poll() {
  if (!key.value || polling.value) return
  polling.value = true
  try {
    const result = await api.qrPoll(key.value)
    status.value = result.code
    if (result.code === 0) {
      message.value = '登录成功'
      stopPolling()
      emit('loggedIn')
      window.setTimeout(close, 700)
    } else if (result.code === 86090) {
      message.value = '已扫码，请在手机上确认'
    } else if (result.code === 86038) {
      message.value = '二维码已过期'
      stopPolling()
    } else {
      message.value = result.message || '等待扫码'
    }
  } catch (cause) {
    message.value = String(cause)
  } finally {
    polling.value = false
  }
}

function close() {
  emit('update:modelValue', false)
}

function onVisibleChange(value: boolean) {
  emit('update:modelValue', value)
  if (!value) stopPolling()
}

watch(
  () => props.modelValue,
  visible => {
    if (visible) void start()
    else stopPolling()
  },
  { immediate: true },
)

onUnmounted(stopPolling)
</script>

<template>
  <ElDialog
    :model-value="modelValue"
    width="380px"
    align-center
    append-to-body
    class="login-dialog"
    @update:model-value="onVisibleChange"
    @closed="stopPolling"
  >
    <template #header>
      <div class="dialog-title"><ShieldCheck :size="18" /><span>扫码登录</span></div>
    </template>
    <div class="login-body">
      <div v-if="loading" class="qr-frame qr-loading">
        <LoaderCircle :size="26" class="spin" />
      </div>
      <div v-else-if="qrUrl" class="qr-frame">
        <QrcodeVue :value="qrUrl" :size="216" level="M" render-as="svg" />
        <div v-if="expired" class="qr-mask">
          <span>二维码已过期</span>
          <ElButton type="primary" size="small" :icon="RefreshCw" @click="start">刷新二维码</ElButton>
        </div>
      </div>
      <div v-else class="qr-frame qr-loading">
        <span class="muted">二维码获取失败</span>
      </div>
      <p class="login-status" :class="{ success: status === 0, error: !!error }">{{ statusText }}</p>
      <p class="login-tip">打开哔哩哔哩 App → 我的 → 扫一扫</p>
    </div>
  </ElDialog>
</template>
