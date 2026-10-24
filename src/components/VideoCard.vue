<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ElButton, ElOption, ElSelect, ElTooltip } from 'element-plus'
import { Clock3, Download as DownloadIcon, Film, Info, LoaderCircle, MessageSquarePlus, Music2, User } from '@lucide/vue'
import type { QualityOption, VideoDetail } from '../types'
import { idBackground } from '../idColor'

const props = defineProps<{ video: VideoDetail; selectedQuality: number; busy?: boolean; commentBusy?: boolean }>()
const emit = defineEmits<{
  'update:selectedQuality': [index: number]
  comments: []
  download: []
  downloadAudio: []
  pageChange: [page: number]
}>()

const imageFailed = ref(false)
const current = computed<QualityOption | null>(
  () => props.video.qualities[props.selectedQuality] ?? null,
)

watch(() => props.video, () => { imageFailed.value = false })

function qualityLabel(quality: QualityOption): string {
  return quality.mode === 'durl' ? quality.label : `${quality.label} · ${quality.codec}`
}

function qualityMeta(quality: QualityOption): string {
  const parts: string[] = []
  if (quality.codec && quality.codec !== 'MP4') parts.push(quality.codec)
  if (quality.width && quality.height) parts.push(`${quality.width}×${quality.height}`)
  if (quality.fps) parts.push(`${quality.fps}fps`)
  if (quality.bandwidth) parts.push(formatBitrate(quality.bandwidth))
  if (quality.size) parts.push(`≈${formatSize(quality.size)}`)
  if (quality.mode === 'durl') parts.push('含音轨')
  return parts.join(' · ')
}

function formatDuration(seconds: number): string {
  if (!seconds) return '--:--'
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const rest = seconds % 60
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, '0')}:${String(rest).padStart(2, '0')}`
    : `${minutes}:${String(rest).padStart(2, '0')}`
}

function formatSize(bytes: number | null): string {
  if (!bytes) return '-'
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
  return `${(bytes / 1024).toFixed(0)} KB`
}

function formatBitrate(bps: number): string {
  if (!bps) return '-'
  return `${Math.round(bps / 1000)} kbps`
}

function onPageChange(value: string | number | boolean) {
  emit('pageChange', Number(value))
}

function onQualityChange(value: string | number | boolean) {
  emit('update:selectedQuality', Number(value))
}
</script>

<template>
  <article class="video-card">
    <div class="video-cover">
      <img
        v-if="video.cover && !imageFailed"
        :src="video.cover"
        alt="视频封面"
        referrerpolicy="no-referrer"
        @error="imageFailed = true"
      />
      <div v-else class="cover-fallback"><Film :size="30" /></div>
      <span class="duration-badge">{{ formatDuration(video.duration) }}</span>
    </div>

    <div class="video-info">
      <div class="video-heading">
        <h2>{{ video.title }}</h2>
      </div>

      <div class="video-meta">
        <span class="meta-item video-owner"><User :size="13" aria-hidden="true" /><span>{{ video.owner || '未知 UP 主' }}</span></span>
        <span class="meta-item"><Clock3 :size="13" />{{ formatDuration(video.duration) }}</span>
        <span class="video-bv" :style="{ backgroundColor: idBackground(video.bvid), color: '#334155' }">{{ video.bvid }}</span>
      </div>

      <div v-if="video.pages.length > 1" class="video-page-row">
        <span class="video-field-label">分 P</span>
        <ElSelect
          v-if="video.pages.length > 1"
          :model-value="video.page"
          class="page-select"
          size="small"
          aria-label="选择分P"
          @update:model-value="onPageChange"
        >
          <ElOption
            v-for="page in video.pages"
            :key="page.cid"
            :value="page.page"
            :label="`P${page.page} · ${page.part}`"
          />
        </ElSelect>
      </div>

      <div class="quality-row">
        <span class="video-field-label">画质</span>
        <ElSelect
          :model-value="selectedQuality"
          class="quality-select"
          aria-label="选择清晰度"
          :disabled="!video.qualities.length || busy"
          placeholder="暂无可用画质"
          @update:model-value="onQualityChange"
        >
          <ElOption
            v-for="(quality, index) in video.qualities"
            :key="`${quality.mode}-${quality.id}-${quality.codec}`"
            :value="index"
            :label="qualityLabel(quality)"
          >
            <div class="quality-option">
              <span class="quality-name">{{ quality.label }}</span>
              <span class="quality-meta">{{ qualityMeta(quality) }}</span>
              <span v-if="quality.recommended" class="badge badge-recommend">推荐</span>
              <span v-if="quality.needsVip" class="badge badge-vip">大会员</span>
            </div>
          </ElOption>
        </ElSelect>
        <div class="video-actions">
        <ElButton class="comment-task-button" title="解析评论" aria-label="解析评论" :loading="commentBusy" :disabled="commentBusy" @click="emit('comments')">
          <MessageSquarePlus v-if="!commentBusy" :size="17" aria-hidden="true" />
        </ElButton>
        <ElButton class="audio-download-button" title="下载音频 · 最高可用音质（原音轨）" aria-label="下载音频" :disabled="!current || busy" @click="emit('downloadAudio')"><Music2 :size="17" aria-hidden="true" /></ElButton>
        <ElButton
          type="primary"
          class="download-button"
          :title="busy ? '添加中' : '下载'"
          :aria-label="busy ? '正在添加下载' : '下载视频'"
          :disabled="!current || busy"
          :aria-disabled="busy || !current"
          :aria-busy="busy"
          @click="!busy && emit('download')"
        >
          <span class="video-download-icon" aria-hidden="true"><LoaderCircle v-if="busy" :size="17" class="spin" /><DownloadIcon v-else :size="17" /></span>
        </ElButton>
        </div>
      </div>

    </div>

      <div v-if="current" class="quality-summary">
        <span v-if="current.mode === 'dash'">音频：{{ current.audioLabel || '自动选择' }}</span>
        <span v-else>单文件 MP4 · 含音轨</span>
        <span v-if="current.size">约 {{ formatSize(current.size) }}</span>
        <ElTooltip v-if="video.hint" :content="video.hint" placement="top" :show-after="150" popper-class="video-hint-tooltip">
          <button type="button" class="video-help" aria-label="清晰度与账号权限说明"><Info :size="13" aria-hidden="true" /><span>清晰度说明</span></button>
        </ElTooltip>
      </div>
      <p v-else class="video-hint" role="status">{{ video.hint }}</p>
  </article>
</template>
