import { onMounted, onUnmounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api, desktop } from '../api'
import type { DownloadTask } from '../types'

export function useDownloads(
  report: (message: string) => void,
  onRemoved: () => void,
) {
  const tasks = ref<DownloadTask[]>([])
  const removing = ref<string[]>([])
  const retrying = ref<string[]>([])
  let unlisten: UnlistenFn | undefined
  let unlistenHistory: UnlistenFn | undefined
  let disposed = false
  let revision = 0
  const removed = new Set<string>()

  function upsert(task: DownloadTask) {
    if (removed.has(task.id)) return
    revision++
    const index = tasks.value.findIndex(item => item.id === task.id)
    if (index < 0) tasks.value.unshift(task)
    else tasks.value[index] = task
  }

  onMounted(async () => {
    if (!desktop) return
    try {
      const stop = await listen<DownloadTask>('download-progress', event => upsert(event.payload))
      if (disposed) {
        stop()
        return
      }
      unlisten = stop
      const stopHistory = await listen<string>('download-history-error', event => report(event.payload))
      if (disposed) {
        stopHistory()
        return
      }
      unlistenHistory = stopHistory

      const before = revision
      const snapshot = await api.tasks()
      if (disposed) return
      if (before === revision) {
        tasks.value = snapshot
      } else {
        for (const task of snapshot) {
          if (!removed.has(task.id) && !tasks.value.some(item => item.id === task.id)) {
            tasks.value.push(task)
          }
        }
      }
    } catch (error) {
      report(String(error))
    }
  })

  onUnmounted(() => {
    disposed = true
    unlisten?.()
    unlistenHistory?.()
  })

  async function cancel(id: string) {
    try {
      await api.cancel(id)
    } catch (error) {
      report(String(error))
    }
  }

  async function reveal(id: string) {
    try {
      await api.reveal(id)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      const expected = ['文件已被移动或删除', '文件尚未下载完成', '下载任务不存在'].includes(message)
      report(message)
      if (expected) return
    }
  }

  async function remove(id: string) {
    if (removing.value.includes(id)) return
    removing.value.push(id)
    try {
      await api.remove(id)
      removed.add(id)
      revision++
      tasks.value = tasks.value.filter(task => task.id !== id)
      onRemoved()
    } catch (error) {
      report(String(error))
    } finally {
      removing.value = removing.value.filter(value => value !== id)
    }
  }

  async function retry(id: string) {
    if (retrying.value.includes(id)) return
    retrying.value.push(id)
    try {
      await api.retry(id)
    } catch (error) {
      report(String(error))
    } finally {
      retrying.value = retrying.value.filter(value => value !== id)
    }
  }

  return { tasks, cancel, reveal, remove, retry, removing, retrying }
}
