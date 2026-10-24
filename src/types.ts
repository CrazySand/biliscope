export interface SettingsView {
  dataDir: string
  downloadDir: string
  maxConcurrent: number
  hasCookie: boolean
}

export interface SettingsInput {
  downloadDir: string
  maxConcurrent: number
}

export interface LoginState {
  loggedIn: boolean
  uname: string
  mid: number
  avatar: string
  vip: boolean
}

export interface QrCode {
  url: string
  key: string
}

export interface QrPoll {
  code: number
  message: string
}

export interface PageInfo {
  page: number
  cid: number
  part: string
  duration: number
}

export interface QualityOption {
  id: number
  label: string
  codec: string
  width: number
  height: number
  fps: string
  bandwidth: number
  size: number | null
  mode: 'dash' | 'durl'
  url: string
  backup: string[]
  audioLabel: string
  audioFormat: string
  audioUrl: string | null
  audioBackup: string[]
  audioSize: number | null
  needsVip: boolean
  recommended: boolean
}

export interface VideoDetail {
  bvid: string
  aid: number
  title: string
  cover: string
  duration: number
  owner: string
  pages: PageInfo[]
  page: number
  qualities: QualityOption[]
  accept: number[]
  loggedIn: boolean
  hint: string
}

export type TaskStatus = 'queued' | 'running' | 'merging' | 'completed' | 'failed' | 'cancelled'

export interface DownloadTask {
  id: string
  createdAt: number
  title: string
  cover: string
  bvid: string
  page: number
  quality: string
  qualityId: number
  codec: string
  mode: string
  filename: string
  path: string
  status: TaskStatus
  phase: string
  downloaded: number
  total: number | null
  speed: number
  error: string | null
}

export interface DownloadRequest {
  title: string
  cover: string
  bvid: string
  page: number
  quality: string
  mode: 'dash' | 'durl' | 'audio'
  qualityId: number
  codec: string
  url: string
  backup: string[]
  audioUrl: string | null
  audioBackup: string[]
  audioLabel: string
  audioFormat: string
}

export interface FfmpegStatus {
  found: boolean
  path: string
  version: string
  source: string
}

export type CommentJobStatus = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled'

export interface CommentCrawlJob {
  id: string
  bvid: string
  aid: number
  title: string
  mode: 'hot' | 'new'
  status: CommentJobStatus
  total: number
  roots: number
  subs: number
  pages: number
  current: string
  note: string | null
  error: string | null
  startedAt: number
  finishedAt: number | null
}

export interface CommentStats {
  roots: number
  subs: number
  total: number
}

export interface CommentVideo {
  bvid: string
  title: string
  total: number
  updatedAt: number
}

export interface CommentRow {
  rpid: number
  root: number
  parent: number
  mid: number
  uname: string
  avatarUrl: string
  message: string
  like: number
  ctime: number
  isSub: boolean
  rcount: number
}

export interface ExportResult {
  path: string
  count: number
}
