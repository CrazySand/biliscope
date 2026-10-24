<script setup lang="ts">
import { ref } from 'vue'
import { ElButton, ElEmpty, ElImage, ElMessageBox, ElProgress, ElScrollbar, ElTag } from 'element-plus'
import { Download, File, Film, FolderOpen, RotateCcw, Trash2, X } from '@lucide/vue'
import type { DownloadTask } from '../types'
import { idBackground } from '../idColor'
import { imageUrl } from '../imageUrl'

defineProps<{ tasks: DownloadTask[]; emptyLabel: string; removing: string[]; retrying: string[] }>()
const emit = defineEmits<{
  cancel: [id: string]
  reveal: [id: string]
  remove: [id: string]
  retry: [id: string]
}>()

const confirming = ref(false)

const labels: Record<DownloadTask['status'], string> = {
  queued: '排队中',
  running: '下载中',
  merging: '合并中',
  completed: '已完成',
  failed: '失败',
  cancelled: '已取消',
}

const tagTypes: Record<DownloadTask['status'], 'primary' | 'success' | 'warning' | 'danger' | 'info'> = {
  queued: 'warning',
  running: 'primary',
  merging: 'primary',
  completed: 'success',
  failed: 'danger',
  cancelled: 'info',
}

function bytes(value: number): string {
  if (value < 1024) return `${value} B`
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`
  if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MB`
  return `${(value / 1024 ** 3).toFixed(2)} GB`
}

function percentage(task: DownloadTask): number {
  if (!task.total) return 0
  return Math.max(0, Math.min(100, (task.downloaded / task.total) * 100))
}

function phaseText(task: DownloadTask): string {
  if (task.status === 'merging') return task.mode === 'audio' ? '正在处理音频' : '正在合并音视频'
  if (task.phase === 'video') return '下载视频'
  if (task.phase === 'audio') return '下载音频'
  return task.phase || '准备中'
}

async function confirmRemove(id: string) {
  if (confirming.value) return
  confirming.value = true
  try {
    await ElMessageBox.confirm('删除此记录？已下载的视频文件会保留。', '删除下载记录', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消',
      confirmButtonClass: 'el-button--danger',
      closeOnClickModal: false,
    })
  } catch {
    return
  } finally {
    confirming.value = false
  }
  emit('remove', id)
}
</script>

<template>
  <ElEmpty v-if="!tasks.length" class="empty-state" :description="emptyLabel" :image-size="46">
    <template #image><div class="empty-icon"><Download :size="22" /></div></template>
  </ElEmpty>
  <div v-else class="task-list-wrap">
    <ElScrollbar class="task-list" height="100%" wrap-class="list-scroll-wrap">
    <article v-for="task in tasks" :key="task.id" class="task-row">
      <ElImage class="task-thumbnail" :src="imageUrl(task.cover) || undefined" referrerpolicy="no-referrer" fit="cover" alt="视频封面" loading="lazy">
        <template #placeholder><div class="task-thumbnail-fallback"><Film :size="20" /></div></template>
        <template #error><div class="task-thumbnail-fallback"><Film :size="20" /></div></template>
      </ElImage>

      <div class="task-main">
        <div class="task-heading">
          <strong>{{ task.title || task.filename }}</strong>
        </div>
        <div class="task-filename">
          <span class="task-quality">{{ task.quality }}</span>
          <span v-if="task.page > 1" class="task-page">P{{ task.page }}</span>
          <span class="task-bvid" :style="{ backgroundColor: idBackground(task.bvid), color: '#334155' }">{{ task.bvid }}</span>
        </div>
        <ElProgress
          v-if="task.status === 'running'"
          class="task-progress"
          :percentage="percentage(task)"
          :show-text="false"
          :stroke-width="4"
          :indeterminate="!task.total"
        />
        <ElProgress
          v-else-if="task.status === 'merging'"
          class="task-progress"
          :percentage="100"
          :show-text="false"
          :stroke-width="4"
          indeterminate
        />
        <p v-if="task.error" class="task-error">{{ task.error }}</p>
        <p v-else-if="task.status === 'running'">
          {{ phaseText(task) }} · {{ bytes(task.downloaded) }}<template v-if="task.total"> / {{ bytes(task.total) }}</template>
          <span class="speed">{{ bytes(task.speed) }}/s</span>
        </p>
        <p v-else-if="task.status === 'merging'">{{ task.mode === 'audio' ? '正在处理音频…' : '正在合并音视频…' }}</p>
        <p v-else-if="task.status === 'completed'" class="task-file-size"><File :size="13" aria-hidden="true" /><b :aria-label="`文件大小 ${bytes(task.downloaded)}`">{{ bytes(task.downloaded) }}</b></p>
        <p v-else>{{ task.status === 'queued' ? '等待下载' : '点击重试，重新开始下载' }}</p>
      </div>

      <div class="task-aside">
        <ElTag class="task-status" :type="tagTypes[task.status]" effect="light" :disable-transitions="true">
          {{ task.status === 'merging' && task.mode === 'audio' ? '处理中' : labels[task.status] }}
        </ElTag>
        <div class="task-actions">
          <ElButton
            v-if="task.status === 'completed'"
            text
            class="icon-button"
            aria-label="在文件夹中显示"
            @click="emit('reveal', task.id)"
          >
            <FolderOpen :size="18" />
          </ElButton>
          <ElButton
            v-if="['queued', 'running', 'merging'].includes(task.status)"
            text
            class="icon-button"
            aria-label="取消下载"
            @click="emit('cancel', task.id)"
          >
            <X :size="18" />
          </ElButton>
          <ElButton
            v-if="['failed', 'cancelled'].includes(task.status)"
            text
            class="icon-button retry-task-button"
            title="重试下载"
            aria-label="重试下载"
            :disabled="retrying.includes(task.id) || confirming"
            @click="emit('retry', task.id)"
          >
            <RotateCcw :size="18" :class="{ spin: retrying.includes(task.id) }" aria-hidden="true" />
          </ElButton>
          <ElButton
            v-if="!['queued', 'running', 'merging'].includes(task.status)"
            text
            class="icon-button delete-task-button"
            aria-label="删除下载记录"
            :disabled="removing.includes(task.id) || confirming"
            @click="confirmRemove(task.id)"
          >
            <Trash2 :size="17" />
          </ElButton>
        </div>
      </div>
    </article>
    </ElScrollbar>
  </div>
</template>
