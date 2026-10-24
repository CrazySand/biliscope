<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import {
  ElButton,
  ElConfigProvider,
  ElContainer,
  ElHeader,
  ElMain,
  ElMessage,
  ElScrollbar,
  ElSegmented,
  ElSkeleton,
  ElSkeletonItem,
  ElTag,
  type MessageHandler,
} from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import { version as appVersion } from '../package.json'
import { ArrowDownToLine, FolderOpen, LogOut, MessageSquare, Play, Settings2, TriangleAlert, User } from '@lucide/vue'
import LinkInput from './components/LinkInput.vue'
import VideoCard from './components/VideoCard.vue'
import DownloadList from './components/DownloadList.vue'
import LoginDialog from './components/LoginDialog.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import CommentDialog from './components/CommentDialog.vue'
import { useDownloads } from './composables/useDownloads'
import { api, desktop } from './api'
import type { LoginState, SettingsView, VideoDetail } from './types'

const settings = ref<SettingsView>({
  dataDir: '',
  downloadDir: '',
  maxConcurrent: 2,
  hasCookie: false,
})
const login = ref<LoginState>({ loggedIn: false, uname: '', mid: 0, avatar: '', vip: false })
const loading = ref(false)
const switchingPage = ref(false)
const adding = ref(false)
const video = ref<VideoDetail | null>(null)
const selectedQuality = ref(0)
const lastInput = ref('')
const linkText = ref('')
const parseError = ref('')
const showLogin = ref(false)
const showSettings = ref(false)
const showComments = ref(false)
let commentsTrigger: HTMLElement | null = null
function restoreHomeFocus() {
  void nextTick(() => commentsTrigger?.focus({ preventScroll: true }))
}
const addingComment = ref(false)
async function addCommentTask() {
  const current = video.value
  if (!current || addingComment.value) return
  addingComment.value = true
  try {
    await api.addCommentVideo(current.bvid, current.aid, current.title)
    notify('已保存到评论任务，可选择范围开始采集')
    openComments(current)
  } catch (error) {
    report(String(error))
  } finally {
    addingComment.value = false
  }
}
const commentVideo = ref<{ bvid: string; title: string } | null>(null)
function openComments(current: VideoDetail | null = null) {
  commentsTrigger = document.activeElement instanceof HTMLElement ? document.activeElement : null
  commentVideo.value = current ? { bvid: current.bvid, title: current.title } : null
  showComments.value = true
}
const filter = ref('all')

let notification: MessageHandler | undefined

function notify(message: string, type: 'success' | 'info' | 'warning' | 'error' = 'success') {
  notification?.close()
  notification = ElMessage({
    message,
    type,
    duration: type === 'error' ? 4000 : 2600,
    customClass: 'app-notification',
  })
}

function report(message: string) {
  notify(message, 'error')
}

const { tasks, cancel, reveal, remove, retry, removing, retrying } = useDownloads(report, () => notify('已删除下载记录'))

const activeCount = computed(
  () => tasks.value.filter(task => ['queued', 'running', 'merging'].includes(task.status)).length,
)
const filtered = computed(() =>
  tasks.value.filter(task =>
    filter.value === 'all'
      ? true
      : filter.value === 'active'
        ? ['queued', 'running', 'merging'].includes(task.status)
        : task.status === 'completed',
  ),
)
const filterOptions = computed(() => [
  { label: tasks.value.length ? `全部 ${tasks.value.length}` : '全部', value: 'all' },
  { label: '进行中', value: 'active' },
  { label: '已完成', value: 'completed' },
])
const emptyLabel = computed(() =>
  filter.value === 'active'
    ? '没有进行中的下载'
    : filter.value === 'completed'
      ? '没有已完成的下载'
      : '暂无下载',
)

watch(linkText, () => {
  parseError.value = ''
})

onMounted(async () => {
  if (!desktop) return
  try {
    settings.value = await api.settings()
  } catch (error) {
    report(String(error))
  }
  try {
    login.value = await api.loginState()
  } catch {
    // A network hiccup should not block the UI; parsing will surface details.
  }
})

onUnmounted(() => notification?.close())

async function parse(value: string) {
  if (loading.value || switchingPage.value) return
  lastInput.value = value
  parseError.value = ''
  loading.value = true
  video.value = null
  try {
    const detail = await api.fetchVideo(value, 1)
    video.value = detail
    selectedQuality.value = Math.max(0, detail.qualities.findIndex(item => item.recommended))
  } catch (error) {
    parseError.value = String(error)
    report(String(error))
  } finally {
    loading.value = false
  }
}

async function loadPage(page: number) {
  if (!lastInput.value || loading.value || switchingPage.value || adding.value || page === video.value?.page) return
  switchingPage.value = true
  try {
    const detail = await api.fetchVideo(lastInput.value, page)
    video.value = detail
    selectedQuality.value = Math.max(0, detail.qualities.findIndex(item => item.recommended))
  } catch (error) {
    report(String(error))
  } finally {
    switchingPage.value = false
  }
}

async function download(audioOnly = false) {
  const detail = video.value
  const selected = detail ? detail.qualities[selectedQuality.value] : null
  const quality = audioOnly ? detail?.qualities.find(item => item.audioUrl) ?? selected : selected
  if (!detail || !quality || adding.value || switchingPage.value) return
  if (quality.mode === 'dash' && !quality.audioUrl) {
    report('该清晰度缺少音频流，请换一个清晰度。')
    return
  }
  adding.value = true
  try {
    if (!(await api.ffmpeg()).found) {
      notify('未检测到 FFmpeg，请先安装后再下载。', 'warning')
      return
    }
    await api.download({
      title: detail.title,
      cover: detail.cover,
      bvid: detail.bvid,
      page: detail.page,
      quality: audioOnly ? `仅音频 · ${quality.audioUrl ? quality.audioLabel : '原始音轨'}` : quality.label,
      mode: audioOnly ? 'audio' : quality.mode,
      qualityId: quality.id,
      codec: quality.codec,
      url: audioOnly ? quality.audioUrl || quality.url : quality.url,
      backup: audioOnly && quality.audioUrl ? quality.audioBackup : quality.backup,
      audioUrl: quality.audioUrl,
      audioBackup: quality.audioBackup,
      audioLabel: quality.audioLabel,
      audioFormat: quality.audioFormat,
    })
    notify('已加入下载')
    filter.value = 'all'
  } catch (error) {
    report(String(error))
  } finally {
    adding.value = false
  }
}

async function onLoggedIn() {
  try {
    login.value = await api.loginState()
  } catch (error) {
    report(String(error))
  }
  notify('登录成功')
}

async function logout() {
  try {
    await api.logout()
    login.value = { loggedIn: false, uname: '', mid: 0, avatar: '', vip: false }
    notify('已退出登录', 'info')
  } catch (error) {
    report(String(error))
  }
}

function onSettingsSaved(view: SettingsView) {
  settings.value = view
  notify('已保存')
}
</script>

<template>
  <ElConfigProvider :locale="zhCn" size="small">
    <div class="app-pages">
    <Transition name="home-page" @after-enter="restoreHomeFocus">
    <div v-show="!showComments" class="app-page-layer" :inert="showComments">
    <ElScrollbar class="page-scrollbar" height="100vh" wrap-class="page-scroll-wrap">
      <ElContainer direction="vertical" class="app-shell">
        <ElHeader class="app-header">
          <div class="brand">
            <span class="brand-mark"><Play :size="17" fill="currentColor" /></span>
            <h1>BiliScope</h1>
            <ElTag class="brand-version" size="small" effect="light" :disable-transitions="true" :aria-label="`版本 ${appVersion}`">v{{ appVersion }}</ElTag>
          </div>
          <div class="header-actions">
            <button type="button" class="user-chip" @click="showLogin = true">
              <img v-if="login.avatar" :src="login.avatar" alt="" referrerpolicy="no-referrer" />
              <span v-else class="user-avatar"><User :size="14" /></span>
              <span class="user-name">{{ login.loggedIn ? login.uname : '扫码登录' }}</span>
            </button>
            <ElButton
              v-if="login.loggedIn"
              text
              class="icon-button"
              aria-label="退出登录"
              @click="logout"
            >
              <LogOut :size="18" />
            </ElButton>
            <ElButton text class="icon-button" title="设置" aria-label="设置" @click="showSettings = true">
              <Settings2 :size="19" />
            </ElButton>
          </div>
        </ElHeader>

        <ElMain class="app-main">
          <section class="capture-panel" aria-labelledby="capture-title">
          <div class="capture-heading">
            <h2 id="capture-title">添加视频</h2>
          </div>
          <div class="input-section">
            <LinkInput :busy="loading || switchingPage" @parse="parse" @input-change="linkText = $event" />
          </div>

          <section v-if="loading || video || parseError" class="result-section" :aria-busy="loading">
            <ElSkeleton v-if="loading" animated class="result-loading" aria-label="正在解析视频">
              <template #template>
                <div class="loading-card">
                  <ElSkeletonItem variant="image" class="loading-cover" />
                  <div class="loading-lines">
                    <ElSkeletonItem variant="text" />
                    <ElSkeletonItem variant="text" />
                    <ElSkeletonItem variant="button" />
                  </div>
                </div>
              </template>
            </ElSkeleton>
            <VideoCard
              v-else-if="video"
              :video="video"
              :selected-quality="selectedQuality"
              :busy="adding || switchingPage"
              :page-busy="switchingPage"
              @download-audio="download(true)"
              :comment-busy="addingComment"
              @comments="addCommentTask"
              @update:selected-quality="selectedQuality = $event"
              @download="download(false)"
              @page-change="loadPage"
            />
            <div
              v-else-if="parseError"
              class="result-placeholder is-error"
              role="status"
              aria-live="polite"
              aria-atomic="true"
            >
              <div class="placeholder-icon">
                <TriangleAlert :size="18" />
              </div>
              <div class="placeholder-copy">
                <p class="placeholder-title">解析失败</p>
                <p class="placeholder-hint">{{ parseError.replace(/^Error:\s*/, '') }}</p>
              </div>
            </div>
          </section>
          </section>

          <section class="downloads-section" :class="{ 'has-tasks': tasks.length > 0 }" aria-label="下载列表">
            <div class="section-heading">
              <h2>
                <ArrowDownToLine :size="16" />下载队列
                <span v-if="activeCount" class="count-badge" role="status" :aria-label="`${activeCount} 个下载任务进行中`"><span class="count-badge-dot" aria-hidden="true"></span><span>进行中</span><b>{{ activeCount }}</b></span>
              </h2>
              <div class="library-heading-actions">
                <ElSegmented v-model="filter" :options="filterOptions" class="download-filters soft-segmented" aria-label="筛选下载任务" />
                <ElButton text class="comment-history-button" size="default" title="评论记录" aria-label="评论记录" @click="openComments()"><MessageSquare :size="17" aria-hidden="true" /></ElButton>
              </div>
            </div>
            <DownloadList
              :tasks="filtered"
              :empty-label="emptyLabel"
              :removing="removing"
              :retrying="retrying"
              @cancel="cancel"
              @retry="retry"
              @reveal="reveal"
              @remove="remove"
            />
            <div class="download-footer">
              <FolderOpen :size="14" />
              <span>{{ settings.downloadDir || '尚未设置保存位置' }}</span>
            </div>
          </section>
        </ElMain>
      </ElContainer>
    </ElScrollbar>
    </div>
    </Transition>
    <Transition name="comments-page">
      <div v-show="showComments" class="app-page-layer" :inert="!showComments">
        <CommentDialog v-model="showComments" :video="commentVideo" />
      </div>
    </Transition>
    </div>

    <LoginDialog v-model="showLogin" @logged-in="onLoggedIn" />
    <SettingsPanel v-model="showSettings" :settings="settings" @saved="onSettingsSaved" />
  </ElConfigProvider>
</template>
