<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { ElButton, ElDialog, ElInputNumber, ElMessage, ElMessageBox, ElOption, ElPagination, ElProgress, ElScrollbar, ElSegmented, ElSelect, ElTag } from 'element-plus'
import { listen } from '@tauri-apps/api/event'
import { ArrowLeft, ChevronDown, Info, Heart, Film, ListPlus, LoaderCircle, MessageSquare, Trash2, TriangleAlert } from '@lucide/vue'
import { api, desktop } from '../api'
import type { CommentCrawlJob, CommentRow, CommentStats, CommentVideo } from '../types'

type VideoIdentity = { bvid: string; title: string }
const props = defineProps<{ modelValue: boolean; video?: VideoIdentity | null }>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()
const backButton = ref<InstanceType<typeof ElButton>>()
const video = ref<VideoIdentity | null>(null)
const library = ref<CommentVideo[]>([])
const jobs = ref<CommentCrawlJob[]>([])
const libraryLoading = ref(false)
const libraryError = ref('')
const reading = ref(false)
const readError = ref('')
const stats = ref<CommentStats | null>(null)
const job = ref<CommentCrawlJob | null>(null)
const preview = ref<CommentRow[]>([])
const failedAvatars = ref(new Set<string>())
const page = ref(1)
const pageSize = 20
const commentScroll = ref<InstanceType<typeof ElScrollbar>>()
const mode = ref<'hot' | 'new'>('hot')
const range = ref('limited')
const maxPages = ref(5)
const starting = ref(false)
const cancelling = ref(false)
const exporting = ref(false)
const lastExport = ref('')
const deleting = ref(false)
const deletedVideos = ref(new Set<string>())
const settingsExpanded = ref(false)
const settingsClosing = ref(false)
const statusExpanded = ref(false)
let lastProgressRefresh = 0
let revision = 0
let libraryRevision = 0
let openRevision = 0
let jobEventRevision = 0
const eventJobs = new Map<string, { job: CommentCrawlJob; revision: number }>()
let disposed = false
let unlisten: (() => void) | null = null
const modeOptions = [{ label: '热门', value: 'hot' }, { label: '最新', value: 'new' }]
const rangeOptions = [{ label: '前 N 页', value: 'limited' }, { label: '全部可见', value: 'all' }]
const running = computed(() => job.value?.status === 'queued' || job.value?.status === 'running')
const hasResult = computed(() => (stats.value?.roots ?? 0) > 0)
const collectionLabel = computed(() => hasResult.value ? '重新采集并合并' : '采集评论')
const effectiveStatus = computed(() => job.value?.status ?? (stats.value?.roots ? 'saved' : undefined))
const showStatus = computed(() => Boolean(job.value))
const displayStats = computed(() => stats.value ?? { roots: 0, subs: 0, total: 0 })
const options = computed(() => {
  const items = new Map(library.value.map(item => [item.bvid, { ...item, status: '' }]))
  for (const item of jobs.value) {
    if (!items.has(item.bvid)) items.set(item.bvid, { bvid: item.bvid, title: item.title || item.bvid, total: 0, updatedAt: item.startedAt, status: '' })
    if (['queued', 'running'].includes(item.status)) items.get(item.bvid)!.status = '采集中'
  }
  for (const item of [props.video, video.value]) {
    if (item && !deletedVideos.value.has(item.bvid) && !items.has(item.bvid)) items.set(item.bvid, { ...item, total: 0, updatedAt: 0, status: '' })
  }
  return [...items.values()].sort((a, b) => b.updatedAt - a.updatedAt)
})
const statusText: Record<string, string> = {
  queued: '排队中', running: '采集中', completed: '已完成', failed: '失败', cancelled: '已停止', saved: '本地已保存',
}
const reachedPageLimit = computed(() => Boolean(job.value?.note?.startsWith('已达到本次页数上限')))
const statusNote = computed(() => reachedPageLimit.value
  ? '已达所选页数上限，可增大采集范围后重新采集并合并。'
  : job.value?.note?.startsWith('未登录')
    ? '当前未登录，仅返回可见评论；登录后可重新采集尝试获取更多。'
    : job.value?.note)
const emptyTitle = computed(() => reading.value ? '正在读取本地评论' : readError.value ? '读取失败' : running.value ? '正在采集评论' : '暂无已保存的评论')
const emptyHint = computed(() => readError.value || (reading.value ? '正在读取已保存的结果…' : running.value ? '结果会自动更新；返回主页不影响采集。' : '选择采集范围，评论会自动保存在这里。'))
const emptyIcon = computed(() => reading.value || running.value ? LoaderCircle : readError.value ? TriangleAlert : MessageSquare)
function statusLabel(status?: string) { return status ? statusText[status] ?? status : '未开始' }
function statusType(status?: string): 'success' | 'danger' | 'info' | 'primary' | 'warning' {
  if (status === 'completed' || status === 'saved') return 'success'
  if (status === 'failed') return 'danger'
  if (status === 'running') return 'primary'
  return 'info'
}
function formatTime(seconds: number) { return seconds ? new Date(seconds * 1000).toLocaleString('zh-CN', { hour12: false }) : '' }
function avatarSource(value?: string) {
  if (!value) return ''
  try {
    const url = new URL(value.startsWith('//') ? `https:${value}` : value)
    if (url.protocol === 'http:') url.protocol = 'https:'
    return url.protocol === 'https:' ? url.href : ''
  } catch { return '' }
}
function initial(name: string) { return (name || '?').trim().charAt(0).toUpperCase() || '?' }
function restorePreferences(bvid: string) {
  mode.value = 'hot'; range.value = 'limited'; maxPages.value = 5
  try {
    const value = JSON.parse(localStorage.getItem(`comment-options:${bvid}`) || 'null')
    if (!value) return
    mode.value = value.mode === 'new' ? 'new' : 'hot'
    range.value = value.range === 'all' ? 'all' : 'limited'
    maxPages.value = Number.isInteger(value.maxPages) && value.maxPages >= 1 && value.maxPages <= 1000 ? value.maxPages : 5
  } catch { /* Invalid preferences fall back to a bounded first run. */ }
}
watch([mode, range, maxPages], () => {
  if (!video.value) return
  try { localStorage.setItem(`comment-options:${video.value.bvid}`, JSON.stringify({ mode: mode.value, range: range.value, maxPages: maxPages.value })) } catch { /* Preferences must not block collection. */ }
})
function selectVideo(bvid: string) {
  const next = options.value.find(item => item.bvid === bvid)
  if (!next) return
  revision++
  video.value = { bvid: next.bvid, title: next.title }
  job.value = jobs.value.find(item => item.bvid === bvid) ?? null
  page.value = 1
  settingsExpanded.value = false; statusExpanded.value = false
  stats.value = null; preview.value = []; lastExport.value = ''; readError.value = ''
  restorePreferences(bvid)
  void refresh()
}
async function loadLibrary() {
  const current = ++libraryRevision
  const beforeEvents = jobEventRevision
  libraryLoading.value = true; libraryError.value = ''
  try {
    const [saved, active] = await Promise.all([api.commentVideos(), api.commentJobs()])
    if (disposed || current !== libraryRevision) return
    library.value = saved
    // A progress event received while reading must win over this snapshot.
    for (const event of eventJobs.values()) {
      if (event.revision <= beforeEvents) continue
      const item = event.job
      const index = active.findIndex(value => value.id === item.id)
      if (index >= 0) active[index] = item
      else active.push(item)
    }
    jobs.value = active.sort((a, b) => b.startedAt - a.startedAt)
  } catch (error) {
    if (!disposed && current === libraryRevision) libraryError.value = String(error)
  } finally {
    if (!disposed && current === libraryRevision) libraryLoading.value = false
  }
}
async function refresh() { await readPage(page.value) }
async function changePage(targetPage: number) {
  if (reading.value || targetPage === page.value) return
  await readPage(targetPage)
}
async function readPage(targetPage: number) {
  const bvid = video.value?.bvid
  if (!bvid) return
  const current = ++revision
  reading.value = true; readError.value = ''
  try {
    const nextStats = await api.commentStats(bvid)
    if (disposed || current !== revision || video.value?.bvid !== bvid) return
    const nextPage = Math.max(1, Math.min(targetPage, Math.ceil(nextStats.roots / pageSize) || 1))
    const rows = await api.comments(bvid, (nextPage - 1) * pageSize, pageSize, true)
    if (disposed || current !== revision || video.value?.bvid !== bvid) return
    const moved = nextPage !== page.value
    stats.value = nextStats; preview.value = rows; page.value = nextPage
    if (moved) {
      await nextTick()
      if (!disposed && current === revision) commentScroll.value?.setScrollTop(0)
    }
  } catch (error) {
    if (!disposed && current === revision) readError.value = String(error)
  } finally {
    if (!disposed && current === revision) reading.value = false
  }
}

async function open() {
  const current = ++openRevision
  if (props.video) deletedVideos.value.delete(props.video.bvid)
  await loadLibrary()
  if (!props.modelValue || disposed || current !== openRevision) return
  const bvid = props.video?.bvid ?? video.value?.bvid ?? options.value[0]?.bvid
  if (bvid) selectVideo(bvid)
}
watch(() => props.modelValue, visible => {
  if (visible) {
    void open()
    void nextTick(() => backButton.value?.$el?.focus({ preventScroll: true }))
  }
  else { openRevision++; settingsExpanded.value = false }
}, { immediate: true })
async function start() {
  const target = video.value
  if (!target || deleting.value || starting.value || running.value || reading.value || readError.value) return
  const pages = range.value === 'all' ? 0 : maxPages.value
  if (!Number.isInteger(pages) || pages < 0 || pages > 1000 || (range.value !== 'all' && pages < 1)) {
    ElMessage.warning('请输入 1–1000 的页数'); return
  }
  starting.value = true
  try {
    const next = await api.startCommentCrawl(target.bvid, mode.value, pages)
    if (disposed) return
    const eventJob = jobs.value.find(item => item.id === next.id)
    if (!eventJob) jobs.value.unshift(next)
    if (video.value?.bvid === target.bvid) {
      job.value = eventJob ?? next
      settingsExpanded.value = false; statusExpanded.value = false
    }
    ElMessage.success('已开始采集，已有评论会保留并去重')
    void loadLibrary()
  } catch (error) { if (!disposed) ElMessage.error(String(error)) }
  finally { if (!disposed) starting.value = false }
}
async function cancel() {
  if (!job.value || cancelling.value) return
  cancelling.value = true
  try {
    await api.cancelCommentCrawl(job.value.id)
    ElMessage.info('当前请求结束后停止，已采集评论会保留')
  } catch (error) { ElMessage.error(String(error)) }
  finally { cancelling.value = false }
}
async function deleteRecord() {
  const target = video.value
  if (!target || deleting.value || running.value || starting.value || exporting.value) return
  deleting.value = true
  try {
    await ElMessageBox.confirm(`删除「${target.title}」的全部本地评论及采集记录？此操作无法撤销，已导出的文件不受影响。`, '删除评论记录', {
      confirmButtonText: '删除记录', cancelButtonText: '取消', type: 'warning',
    })
    await api.deleteCommentVideo(target.bvid)
    revision++; libraryRevision++; openRevision++
    libraryLoading.value = false; reading.value = false
    deletedVideos.value.add(target.bvid)
    library.value = library.value.filter(item => item.bvid !== target.bvid)
    jobs.value = jobs.value.filter(item => item.bvid !== target.bvid)
    for (const [id, event] of eventJobs) if (event.job.bvid === target.bvid) eventJobs.delete(id)
    video.value = null; job.value = null; stats.value = null; preview.value = []; lastExport.value = ''; readError.value = ''
    settingsExpanded.value = false; statusExpanded.value = false; page.value = 1
    const next = options.value[0]
    if (next) selectVideo(next.bvid)
    ElMessage.success('已删除本地评论记录')
  } catch (error) {
    if (error !== 'cancel' && error !== 'close') ElMessage.error(String(error))
  } finally { deleting.value = false }
}
async function exportComments(format: 'csv' | 'json') {
  const target = video.value
  if (!target || deleting.value || exporting.value || !stats.value?.roots) return
  exporting.value = true
  try {
    const safeTitle = target.title.replace(/[<>:"/\\|?*\n\r]/g, '_').slice(0, 60)
    const path = await api.pickSaveFile(`${safeTitle} [${target.bvid}] 评论.${format}`, format)
    if (!path) return
    const result = await api.exportComments(target.bvid, format, path)
    if (video.value?.bvid === target.bvid) lastExport.value = result.path
    ElMessage.success(`已导出 ${result.count} 条评论`)
  } catch (error) { ElMessage.error(String(error)) }
  finally { exporting.value = false }
}
onMounted(async () => {
  if (!desktop) return
  try {
    const stop = await listen<CommentCrawlJob>('comment-progress', event => {
      const incoming = event.payload
      if (deletedVideos.value.has(incoming.bvid)) return
      eventJobs.set(incoming.id, { job: incoming, revision: ++jobEventRevision })
      const index = jobs.value.findIndex(item => item.id === incoming.id)
      if (index < 0) jobs.value.unshift(incoming)
      else jobs.value[index] = incoming
      jobs.value.sort((a, b) => b.startedAt - a.startedAt)
      if (incoming.bvid === video.value?.bvid && (!job.value || incoming.startedAt >= job.value.startedAt)) {
        job.value = incoming
        const finished = ['completed', 'failed', 'cancelled'].includes(incoming.status)
        if (finished || (props.modelValue && !reading.value && Date.now() - lastProgressRefresh >= 2000)) {
          lastProgressRefresh = Date.now()
          void refresh()
        }
      }
      if (['completed', 'failed', 'cancelled'].includes(incoming.status)) void loadLibrary()
    })
    if (disposed) stop()
    else unlisten = stop
  } catch (error) { if (!disposed) libraryError.value = String(error) }
})
onUnmounted(() => { disposed = true; revision++; libraryRevision++; openRevision++; unlisten?.() })
</script>

<template>
  <section class="comment-page comment-dialog" aria-label="评论记录">
    <header class="comment-page-header">
      <ElButton ref="backButton" text class="comment-back-button" title="返回主页" aria-label="返回主页" @click="emit('update:modelValue', false)">
        <ArrowLeft :size="20" aria-hidden="true" />
      </ElButton>
      <div class="comment-head-text">
        <strong>评论记录</strong>
        <small>采集与管理视频评论</small>
      </div>
    </header>
    <div class="comment-page-body">
    <div class="comment-library-bar">
      <span class="comment-video-label" title="视频" aria-label="视频"><Film :size="18" aria-hidden="true" /></span>
      <ElSelect :model-value="video?.bvid" filterable placeholder="选择已采集的视频" aria-label="评论记录视频" :loading="libraryLoading" :disabled="libraryLoading || starting || exporting || deleting" @update:model-value="selectVideo">
        <ElOption v-for="item in options" :key="item.bvid" :value="item.bvid" :label="`${item.title} · ${item.bvid}`">
          <span class="comment-library-title">{{ item.title }}</span>
          <span class="comment-library-meta">{{ item.status || (item.total ? `${item.total} 条已保存` : '待采集') }}</span>
        </ElOption>
      </ElSelect>
      <ElButton v-if="video" type="primary" class="comment-collect-icon" :title="running ? '正在采集' : collectionLabel" :aria-label="collectionLabel" :disabled="running || settingsClosing || reading || !!readError || starting || exporting || deleting" @click="settingsExpanded = true"><ListPlus :size="18" aria-hidden="true" /></ElButton>
      <ElButton v-if="video" class="comment-delete-button" :title="running ? '请先停止采集再删除' : '删除此视频的评论记录'" aria-label="删除评论记录" :disabled="running || starting || exporting || deleting" @click="deleteRecord"><Trash2 :size="16" aria-hidden="true" /></ElButton>
    </div>
    <p v-if="libraryError" class="comment-status-error" role="alert">{{ libraryError }}</p>
    <div v-if="!video" class="comment-empty" role="status">
      <MessageSquare :size="24" />
      <p>{{ libraryLoading ? '正在读取评论记录' : libraryError ? '暂时无法读取记录' : '还没有评论记录' }}</p>
      <span>在主页解析视频，点击「解析评论」即可添加任务，再选择范围开始采集。</span>
    </div>
    <div v-else class="comment-dialog-body">
      <div v-if="showStatus" class="comment-status-card" :class="{ 'is-running': running }" :aria-busy="running">
        <div class="comment-status-head">
          <ElTag :type="statusType(effectiveStatus)" size="small" effect="light">
            {{ statusLabel(effectiveStatus) }}
          </ElTag>
          <div class="comment-status-text" role="status" aria-live="polite">
            <strong>{{ running ? '已读取' : '上次读取' }} {{ job?.roots ?? 0 }} 条主评论</strong>
            <span class="comment-status-secondary">{{ job?.mode === 'new' ? '最新' : '热门' }} · {{ job?.pages ?? 0 }} 页</span>
          </div>
          <ElButton v-if="running" size="small" :loading="cancelling" @click="cancel">停止采集</ElButton>
          <ElButton v-else-if="(statusNote || job?.error) && effectiveStatus !== 'failed'" text size="small" class="comment-status-toggle" :title="statusExpanded ? '收起详情' : '查看详情'" :aria-label="statusExpanded ? '收起采集详情' : '查看采集详情'" :aria-expanded="statusExpanded" @click="statusExpanded = !statusExpanded"><ChevronDown :size="16" :class="{ 'is-expanded': statusExpanded }" /></ElButton>
        </div>
        <ElProgress
          v-if="running"
          :percentage="35"
          indeterminate
          aria-label="正在采集，完成时间取决于评论数量"
          :stroke-width="6"
          :show-text="false"
        />
        <div v-if="running || statusExpanded || effectiveStatus === 'failed'" class="comment-status-meta">
          <span v-if="running && job?.current" class="comment-current-step">{{ job.current }}</span>
          <span v-if="running" class="comment-status-hint">返回主页后继续采集</span>
          <span v-if="statusNote" class="comment-status-note" :class="{ 'is-neutral': reachedPageLimit || job?.note?.startsWith('未登录') }"><Info :size="14" aria-hidden="true" />{{ statusNote }}</span>
          <span v-if="job?.error" class="comment-status-error" role="alert">{{ job.error }}</span>
        </div>
      </div>

      <div class="comment-preview-block">
        <p v-if="readError && preview.length" class="comment-status-error" role="alert">刷新失败，当前显示上次读取的结果：{{ readError }}</p>
        <div class="comment-preview-bar">
          <div class="comment-preview-title">
            <strong>已保存评论</strong>
            <span v-if="hasResult" class="comment-faint">{{ displayStats.roots }} 条主评论</span>
          </div>
          <div class="comment-export-actions">
            <ElButton v-if="readError || running" text :loading="reading" @click="refresh">刷新结果</ElButton>
            <template v-if="stats?.roots && !running">
            <ElButton size="default" :loading="exporting" :disabled="exporting" @click="exportComments('csv')">
              导出 CSV
            </ElButton>
            <ElButton size="default" :loading="exporting" :disabled="exporting" @click="exportComments('json')">
              导出 JSON
            </ElButton>
            </template>
          </div>
        </div>

        <ElScrollbar ref="commentScroll" class="comment-list" :class="{ 'is-empty': preview.length === 0 }" :aria-busy="reading">
          <div v-if="preview.length === 0" class="comment-empty" role="status">
            <div class="comment-empty-symbol"><component :is="emptyIcon" :size="25" :class="{ spin: running || reading }" /></div>
            <p>{{ emptyTitle }}</p>
            <span>{{ emptyHint }}</span>
            <ElButton v-if="!reading && !running && !readError" type="primary" size="default" class="comment-empty-action" :disabled="settingsClosing || starting || exporting || deleting" @click="settingsExpanded = true">{{ collectionLabel }}</ElButton>
            <ElButton v-else-if="readError && !reading" size="default" class="comment-empty-action" @click="refresh">重新读取</ElButton>
          </div>
          <div v-for="item in preview" :key="item.rpid" class="comment-item">
            <div class="comment-avatar">
              <span aria-hidden="true">{{ initial(item.uname) }}</span>
              <img v-if="avatarSource(item.avatarUrl) && !failedAvatars.has(avatarSource(item.avatarUrl))"
                :key="avatarSource(item.avatarUrl)" :src="avatarSource(item.avatarUrl)"
                :alt="`${item.uname}的头像`" loading="lazy" decoding="async" referrerpolicy="no-referrer"
                @error="failedAvatars.add(avatarSource(item.avatarUrl))" />
            </div>
            <div class="comment-item-body">
              <div class="comment-item-head">
                <strong>{{ item.uname }}</strong>
                <span class="comment-faint">{{ formatTime(item.ctime) }}</span>
                <span class="comment-like"><Heart :size="12" />{{ item.like }}</span>
              </div>
              <p class="comment-message">{{ item.message }}</p>
              <span v-if="item.rcount" class="comment-sub-count">{{ item.rcount }} 条子评论</span>
            </div>
          </div>
        </ElScrollbar>
        <div v-if="hasResult" class="comment-list-footer">
          <span class="comment-preview-caption">每页 {{ pageSize }} 条<span v-if="reading"> · 加载中…</span></span>
          <ElPagination :current-page="page" :page-size="pageSize" :total="displayStats.roots" :pager-count="5" :disabled="reading" layout="prev, pager, next" size="small" aria-label="评论列表分页" @update:current-page="changePage" />
        </div>
        <p v-if="lastExport" class="comment-export-path">已导出：{{ lastExport }}</p>
      </div>
    </div>
    </div>
  </section>
  <ElDialog v-model="settingsExpanded" @close="settingsClosing = true" @closed="settingsClosing = false" width="460px" align-center append-to-body class="comment-setup-dialog"
    :close-on-click-modal="false" :close-on-press-escape="!starting" :show-close="!starting">
    <template #header><strong class="comment-setup-heading">{{ hasResult ? '重新采集' : '采集评论' }}</strong></template>
      <div class="comment-collection-settings">
        <div class="comment-toolbar">
          <div class="comment-setting-field">
            <span class="comment-field-label">采集顺序</span>
            <ElSegmented v-model="mode" :options="modeOptions" :disabled="running || starting" aria-label="评论排序" />
          </div>
          <div class="comment-setting-field comment-range-field">
            <span class="comment-field-label">采集范围</span>
            <div class="comment-range-controls">
              <ElSelect v-model="range" :disabled="running || starting" aria-label="采集范围" class="comment-range-select">
                <ElOption v-for="item in rangeOptions" :key="item.value" :label="item.label" :value="item.value" />
              </ElSelect>
              <div v-if="range === 'limited'" class="comment-page-limit">
                <ElInputNumber v-model="maxPages" :min="1" :max="1000" :precision="0" :controls="false" :disabled="running || starting" aria-label="最多采集页数" />
                <span>页</span>
              </div>
            </div>
          </div>

        </div>
        <p class="comment-scope-help">{{ hasResult ? '从第 1 页重新读取，已有评论保留，新评论去重追加。' : '从第 1 页开始，只采集主评论，结果自动保存。' }}</p>
      </div>

    <template #footer>
      <ElButton size="default" :disabled="starting" @click="settingsExpanded = false">取消</ElButton>
      <ElButton type="primary" size="default" :loading="starting" :disabled="running || reading || !!readError || exporting || deleting" @click="start">{{ hasResult ? '重新采集' : '开始采集' }}</ElButton>
    </template>
  </ElDialog>
</template>
