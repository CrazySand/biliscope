import { invoke, isTauri } from '@tauri-apps/api/core'
import { open, save } from '@tauri-apps/plugin-dialog'
import type {
  CommentCrawlJob,
  CommentRow,
  CommentStats,
  CommentVideo,
  DownloadRequest,
  DownloadTask,
  ExportResult,
  FfmpegStatus,
  LoginState,
  QrCode,
  QrPoll,
  SettingsInput,
  SettingsView,
  VideoDetail,
} from './types'

export const desktop = isTauri()

function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!desktop) {
    return Promise.reject(new Error('请使用 npm run tauri dev 启动桌面应用。'))
  }
  return invoke<T>(command, args)
}

export const api = {
  settings: () => call<SettingsView>('get_settings'),
  saveSettings: (settings: SettingsInput) => call<SettingsView>('save_settings', { settings }),
  loginState: () => call<LoginState>('login_state'),
  qrGenerate: () => call<QrCode>('login_qr_generate'),
  qrPoll: (key: string) => call<QrPoll>('login_qr_poll', { key }),
  logout: () => call<void>('logout'),
  fetchVideo: (input: string, page?: number) => call<VideoDetail>('fetch_video', { input, page: page ?? null }),
  download: (request: DownloadRequest) => call<string>('start_download', { request }),
  tasks: () => call<DownloadTask[]>('list_tasks'),
  retry: (id: string) => call<void>('retry_download', { id }),
  cancel: (id: string) => call<void>('cancel_download', { id }),
  remove: (id: string) => call<void>('remove_download', { id }),
  reveal: (id: string) => call<void>('reveal_download', { id }),
  ffmpeg: () => call<FfmpegStatus>('check_ffmpeg'),
  commentStats: (bvid: string) => call<CommentStats>('comment_stats', { bvid }),
  commentJobs: () => call<CommentCrawlJob[]>('list_comment_jobs'),
  deleteCommentVideo: (bvid: string) => call<void>('delete_comment_video', { bvid }),
  addCommentVideo: (bvid: string, aid: number, title: string) => call<void>('add_comment_video', { bvid, aid, title }),
  commentVideos: () => call<CommentVideo[]>('list_comment_videos'),
  startCommentCrawl: (bvid: string, mode: 'hot' | 'new', maxPages: number) =>
    call<CommentCrawlJob>('start_comment_crawl', { bvid, mode, maxPages }),
  cancelCommentCrawl: (id: string) => call<void>('cancel_comment_crawl', { id }),
  comments: (bvid: string, offset = 0, limit = 100, rootsOnly = true) =>
    call<CommentRow[]>('list_comments', { bvid, offset, limit, rootsOnly }),
  exportComments: (bvid: string, format: 'csv' | 'json', path: string) =>
    call<ExportResult>('export_comments', { bvid, format, path }),
  pickSaveFile: async (defaultName: string, format: 'csv' | 'json'): Promise<string | null> => {
    const result = await save({
      defaultPath: defaultName,
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    })
    return typeof result === 'string' ? result : null
  },
  pickDirectory: async (): Promise<string | null> => {
    const result = await open({ directory: true, multiple: false, title: '选择视频保存目录' })
    return typeof result === 'string' ? result : null
  },
}
