<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { ElButton, ElDialog, ElForm, ElFormItem, ElInput, ElMessage, ElScrollbar, ElSegmented, ElTag } from 'element-plus'
import { FolderOpen, LoaderCircle, RotateCw } from '@lucide/vue'
import { api } from '../api'
import type { FfmpegStatus, SettingsView } from '../types'

const props = defineProps<{ modelValue: boolean; settings: SettingsView }>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean]; saved: [value: SettingsView] }>()

const concurrencyOptions = [1, 2, 3, 4].map(value => ({ label: `${value}`, value }))
const form = reactive({ downloadDir: '', maxConcurrent: 2 })
const saving = ref(false)
const choosing = ref(false)
const checking = ref(false)
const ffmpeg = ref<FfmpegStatus | null>(null)
const checkError = ref('')
let checkRevision = 0

const ffmpegLabel = computed(() => checking.value ? '检测中' : checkError.value ? '检测失败' : ffmpeg.value?.found ? '可用' : ffmpeg.value ? '未找到' : checking.value ? '检测中' : '待检测')
const ffmpegHint = computed(() => {
  if (checking.value) return '正在检查本机 FFmpeg…'
  if (checkError.value) return checkError.value
  if (ffmpeg.value?.found) return '可下载视频、合并音视频及保存原始音轨。'
  if (ffmpeg.value) return '视频和音频下载均需要 FFmpeg，检测可用后才能下载。'
  if (checking.value) return '正在检查本机 FFmpeg…'
  return '检测本机是否支持高清音视频合并。'
})

function sync() {
  form.downloadDir = props.settings.downloadDir
  form.maxConcurrent = props.settings.maxConcurrent || 2
}

watch(
  () => props.modelValue,
  visible => {
    if (visible) {
      sync()
      void checkFfmpeg()
    } else {
      checkRevision++
      checking.value = false
    }
  },
  { immediate: true },
)

function close() {
  if (saving.value) return
  emit('update:modelValue', false)
}

async function choose() {
  if (choosing.value) return
  choosing.value = true
  try {
    const path = await api.pickDirectory()
    if (path) form.downloadDir = path
  } catch (error) {
    ElMessage.error({ message: String(error), duration: 4000 })
  } finally {
    choosing.value = false
  }
}

async function checkFfmpeg(manual = false) {
  if (checking.value) return
  const revision = ++checkRevision
  checking.value = true
  try {
    const result = await api.ffmpeg()
    if (revision !== checkRevision) return
    ffmpeg.value = result
    checkError.value = ''
    if (manual) ElMessage({
      message: result.found ? '检测完成，FFmpeg 可用' : '未检测到 FFmpeg，请安装后重试',
      type: result.found ? 'success' : 'warning',
      duration: 2600,
      grouping: true,
    })
  } catch (error) {
    if (revision !== checkRevision) return
    ffmpeg.value = null
    checkError.value = String(error).replace(/^Error:\s*/, '')
    if (manual) ElMessage.error({ message: `检测失败：${checkError.value}`, duration: 4000, grouping: true })
  } finally {
    if (revision === checkRevision) checking.value = false
  }
}

async function save() {
  if (saving.value) return
  if (!form.downloadDir.trim()) {
    ElMessage.error({ message: '请选择保存位置', duration: 3000 })
    return
  }
  saving.value = true
  try {
    const view = await api.saveSettings({
      downloadDir: form.downloadDir.trim(),
      maxConcurrent: form.maxConcurrent,
    })
    emit('saved', view)
    emit('update:modelValue', false)
  } catch (error) {
    ElMessage.error({ message: String(error), duration: 4000 })
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <ElDialog
    :model-value="modelValue"
    width="500px"
    align-center
    append-to-body
    class="settings-dialog"
    :close-on-click-modal="false"
    :close-on-press-escape="!saving"
    :show-close="!saving"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <template #header>
      <div class="settings-heading">
        <h2>设置</h2>
      </div>
    </template>
    <ElScrollbar class="settings-scroll" max-height="calc(100vh - 148px)" wrap-class="settings-scroll-wrap">
      <ElForm label-position="top" @submit.prevent="save">
        <section class="settings-section">
          <ElFormItem label="保存位置">
            <div class="directory-input">
              <ElInput v-model="form.downloadDir" :disabled="saving" aria-label="保存位置" placeholder="选择视频保存目录" />
              <ElButton
                text
                class="icon-button directory-button"
                aria-label="选择下载目录"
                :loading="choosing"
                :disabled="saving"
                :loading-icon="LoaderCircle"
                @click="choose"
              >
                <FolderOpen v-if="!choosing" :size="18" />
              </ElButton>
            </div>
          </ElFormItem>

          <ElFormItem label="同时下载数量" class="concurrency-item">
            <div class="concurrency-setting">
            <ElSegmented
              v-model="form.maxConcurrent"
              :options="concurrencyOptions"
              class="settings-segmented soft-segmented"
              aria-label="同时下载数量"
              :disabled="saving"
            />
            <span class="concurrency-unit">个任务</span>
            </div>
          </ElFormItem>
        </section>

        <section class="settings-section settings-environment" aria-label="FFmpeg 环境" :aria-busy="checking">
          <div class="settings-field-head">
            <span class="settings-field-label">FFmpeg</span>
            <ElTag
              :type="checking || checkError || !ffmpeg ? 'info' : ffmpeg.found ? 'success' : 'warning'"
              effect="light"
              :disable-transitions="true"
            >
              {{ ffmpegLabel }}
            </ElTag>
            <ElButton
              text
              class="settings-inline-action"
              :disabled="checking"
              :aria-disabled="checking"
              :aria-busy="checking"
              @click="checkFfmpeg(true)"
            >
              <RotateCw :size="13" class="ffmpeg-refresh-icon" :class="{ 'is-checking': checking }" aria-hidden="true" />
              <span>{{ checking ? '检测中…' : '重新检测' }}</span>
            </ElButton>
          </div>
          <p class="ffmpeg-hint" role="status">{{ ffmpegHint }}</p>
          <p v-if="!checkError && ffmpeg && !ffmpeg.found" class="settings-help">安装后将 FFmpeg 加入系统 PATH，再重新检测。</p>
        </section>

        <section class="settings-section">
          <ElFormItem label="数据目录">
            <div class="readonly-path">{{ settings.dataDir || '暂未获取数据目录' }}</div>
          </ElFormItem>
        </section>
      </ElForm>
    </ElScrollbar>

    <template #footer>
      <ElButton :disabled="saving" @click="close">取消</ElButton>
      <ElButton type="primary" :loading="saving" :loading-icon="LoaderCircle" @click="save">保存</ElButton>
    </template>
  </ElDialog>
</template>
