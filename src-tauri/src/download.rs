use crate::models::{DownloadRequest, DownloadTask, FfmpegStatus};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct TaskEntry {
    task: DownloadTask,
    cancel: CancellationToken,
}

struct SlotState {
    active: usize,
    limit: usize,
}

pub struct DownloadSlots {
    state: Mutex<SlotState>,
    changed: tokio::sync::Notify,
}

pub struct Permit<'a>(&'a DownloadSlots);

impl DownloadSlots {
    fn new(limit: usize) -> Self {
        Self {
            state: Mutex::new(SlotState { active: 0, limit }),
            changed: tokio::sync::Notify::new(),
        }
    }

    pub fn set_limit(&self, limit: u8) -> Result<(), String> {
        if !(1..=4).contains(&limit) {
            return Err("同时下载数量必须在 1–4 之间".into());
        }
        self.state
            .lock()
            .map_err(|e| e.to_string())?
            .limit = limit as usize;
        self.changed.notify_waiters();
        Ok(())
    }

    async fn acquire(&self, cancel: &CancellationToken) -> Result<Permit<'_>, String> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if cancel.is_cancelled() {
                return Err("下载已取消".into());
            }
            {
                let mut state = self.state.lock().map_err(|e| e.to_string())?;
                if state.active < state.limit {
                    state.active += 1;
                    return Ok(Permit(self));
                }
            }
            tokio::select! {
                _ = cancel.cancelled() => return Err("下载已取消".into()),
                _ = changed => {}
            }
        }
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.active = state.active.saturating_sub(1);
        }
        self.0.changed.notify_waiters();
    }
}

pub struct DownloadManager {
    tasks: Mutex<HashMap<String, TaskEntry>>,
    slots: DownloadSlots,
    history_path: PathBuf,
}

impl DownloadManager {
    pub fn load(history_path: PathBuf) -> Result<Self, String> {
        let tasks = load_history(&history_path)
            .into_iter()
            .map(|task| {
                (
                    task.id.clone(),
                    TaskEntry {
                        task,
                        cancel: CancellationToken::new(),
                    },
                )
            })
            .collect();
        Ok(Self {
            tasks: Mutex::new(tasks),
            slots: DownloadSlots::new(2),
            history_path,
        })
    }

    pub fn set_limit(&self, limit: u8) -> Result<(), String> {
        self.slots.set_limit(limit)
    }

    pub fn list(&self) -> Result<Vec<DownloadTask>, String> {
        let tasks = self.tasks.lock().map_err(|e| e.to_string())?;
        Ok(Self::snapshot(&tasks))
    }

    pub fn get(&self, id: &str) -> Result<DownloadTask, String> {
        let tasks = self.tasks.lock().map_err(|e| e.to_string())?;
        tasks
            .get(id)
            .map(|entry| entry.task.clone())
            .ok_or_else(|| "下载任务不存在".to_string())
    }

    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let tasks = self.tasks.lock().map_err(|e| e.to_string())?;
        let entry = tasks.get(id).ok_or("下载任务不存在")?;
        if matches!(entry.task.status.as_str(), "queued" | "running" | "merging") {
            entry.cancel.cancel();
        }
        Ok(())
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.lock().map_err(|e| e.to_string())?;
        let entry = tasks.get(id).ok_or("下载记录不存在")?;
        if matches!(entry.task.status.as_str(), "queued" | "running" | "merging") {
            return Err("请先取消下载，再删除记录".into());
        }
        let remaining: Vec<_> = Self::snapshot(&tasks)
            .into_iter()
            .filter(|task| task.id != id)
            .collect();
        save_history(&self.history_path, &remaining)?;
        tasks.remove(id);
        Ok(())
    }

    pub fn completed_path(&self, id: &str) -> Result<String, String> {
        let tasks = self.tasks.lock().map_err(|e| e.to_string())?;
        let entry = tasks.get(id).ok_or("下载任务不存在")?;
        if entry.task.status != "completed" {
            return Err("文件尚未下载完成".into());
        }
        if !Path::new(&entry.task.path).is_file() {
            return Err("文件已被移动或删除".into());
        }
        Ok(entry.task.path.clone())
    }

    fn snapshot(tasks: &HashMap<String, TaskEntry>) -> Vec<DownloadTask> {
        let mut result: Vec<_> = tasks.values().map(|entry| entry.task.clone()).collect();
        result.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        result
    }

    fn update(&self, app: &AppHandle, id: &str, f: impl FnOnce(&mut DownloadTask)) {
        if let Ok(mut tasks) = self.tasks.lock() {
            if let Some(entry) = tasks.get_mut(id) {
                let previous_status = entry.task.status.clone();
                f(&mut entry.task);
                let status_changed = entry.task.status != previous_status;
                let _ = app.emit("download-progress", &entry.task);
                if status_changed {
                    if let Err(error) = save_history(&self.history_path, &Self::snapshot(&tasks)) {
                        let _ = app.emit("download-history-error", error);
                    }
                }
            }
        }
    }
}

pub fn start(
    app: AppHandle,
    manager: Arc<DownloadManager>,
    request: DownloadRequest,
    directory: PathBuf,
    ffmpeg: String,
) -> Result<String, String> {
    if request.mode != "dash" && request.mode != "durl" && request.mode != "audio" {
        return Err("未知的下载模式".into());
    }
    if !valid_media_url(&request.url) {
        return Err("视频地址不合法".into());
    }
    if request.mode == "dash" {
        let audio = request.audio_url.as_deref().ok_or("DASH 下载缺少音频地址")?;
        if !valid_media_url(audio) {
            return Err("音频地址不合法".into());
        }
    }
    if !directory.is_absolute() {
        return Err("下载目录必须是绝对路径".into());
    }
    std::fs::create_dir_all(&directory).map_err(|e| format!("无法创建下载目录：{e}"))?;

    let title = sanitize_component(&request.title);
    let quality_display = if request.mode == "dash" && !request.audio_label.trim().is_empty() {
        format!("{} · {}", request.quality, request.audio_label)
    } else {
        request.quality.clone()
    };
    let quality = sanitize_component(&request.quality);
    let page_suffix = if request.page > 1 {
        format!(" P{}", request.page)
    } else {
        String::new()
    };
    let base = format!("{title} [{}]{} [{quality}]", request.bvid, page_suffix);
    let filename = unique_filename(&directory, &base, if request.mode == "audio" { audio_extension(&request.audio_format) } else { "mp4" });
    let id = Uuid::new_v4().to_string();
    let path = directory.join(&filename);
    let cancel = CancellationToken::new();
    let task = DownloadTask {
        id: id.clone(),
        created_at: now_millis(),
        title: request.title.clone(),
        cover: request.cover.clone(),
        bvid: request.bvid.clone(),
        page: request.page,
        quality: quality_display,
        quality_id: request.quality_id,
        codec: request.codec.clone(),
        mode: request.mode.clone(),
        filename,
        path: path.to_string_lossy().into_owned(),
        status: "queued".into(),
        phase: String::new(),
        downloaded: 0,
        total: None,
        speed: 0,
        error: None,
    };

    {
        let mut tasks = manager.tasks.lock().map_err(|e| e.to_string())?;
        tasks.insert(
            id.clone(),
            TaskEntry {
                task: task.clone(),
                cancel: cancel.clone(),
            },
        );
        if let Err(error) = save_history(&manager.history_path, &DownloadManager::snapshot(&tasks)) {
            tasks.remove(&id);
            return Err(error);
        }
    }
    let _ = app.emit("download-progress", &task);

    let manager_for_task = manager.clone();
    let app_for_task = app.clone();
    let request_for_task = request.clone();
    let id_for_task = id.clone();
    let directory_for_task = directory.clone();
    let ffmpeg_for_task = ffmpeg.clone();
    tauri::async_runtime::spawn(async move {
        let result = run(
            &app_for_task,
            &manager_for_task,
            &id_for_task,
            &request_for_task,
            &directory_for_task,
            &task.filename,
            &ffmpeg_for_task,
            &cancel,
        )
        .await;
        match result {
            Ok(final_path) => manager_for_task.update(&app_for_task, &id_for_task, |task| {
                if let Ok(metadata) = std::fs::metadata(&final_path) {
                    task.downloaded = metadata.len();
                    task.total = Some(metadata.len());
                }
                task.filename = final_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                task.path = final_path.to_string_lossy().into_owned();
                task.status = "completed".into();
                task.phase = String::new();
                task.speed = 0;
            }),
            Err(message) => {
                let cancelled = message == "下载已取消";
                manager_for_task.update(&app_for_task, &id_for_task, |task| {
                    task.status = if cancelled { "cancelled" } else { "failed" }.into();
                    task.phase = String::new();
                    task.speed = 0;
                    task.error = if cancelled { None } else { Some(message) };
                });
            }
        }
    });
    Ok(id)
}

/// Re-use a failed/cancelled task row: refresh its media URLs (the caller
/// re-fetches playurl), reset its state, and run it again.
pub fn retry(
    app: AppHandle,
    manager: Arc<DownloadManager>,
    id: String,
    request: DownloadRequest,
    directory: PathBuf,
    ffmpeg: String,
) -> Result<(), String> {
    if request.mode != "dash" && request.mode != "durl" && request.mode != "audio" {
        return Err("未知的下载模式".into());
    }
    if !valid_media_url(&request.url) {
        return Err("视频地址不合法".into());
    }
    if request.mode == "dash" {
        let audio = request.audio_url.as_deref().ok_or("DASH 下载缺少音频地址")?;
        if !valid_media_url(audio) {
            return Err("音频地址不合法".into());
        }
    }
    if !directory.is_absolute() {
        return Err("下载目录必须是绝对路径".into());
    }
    std::fs::create_dir_all(&directory).map_err(|e| format!("无法创建下载目录：{e}"))?;

    let title = sanitize_component(&request.title);
    let quality_display = if request.mode == "dash" && !request.audio_label.trim().is_empty() {
        format!("{} · {}", request.quality, request.audio_label)
    } else {
        request.quality.clone()
    };
    let quality = sanitize_component(&request.quality);
    let page_suffix = if request.page > 1 {
        format!(" P{}", request.page)
    } else {
        String::new()
    };
    let base = format!("{title} [{}]{} [{quality}]", request.bvid, page_suffix);
    let filename = unique_filename(&directory, &base, if request.mode == "audio" { audio_extension(&request.audio_format) } else { "mp4" });
    let path = directory.join(&filename);
    let cancel = CancellationToken::new();

    let updated = {
        let mut tasks = manager.tasks.lock().map_err(|e| e.to_string())?;
        let updated = {
            let entry = tasks.get_mut(&id).ok_or("下载任务不存在")?;
            if !matches!(entry.task.status.as_str(), "failed" | "cancelled") {
                return Err("只有失败或已取消的任务可以重试".into());
            }
            let task = &mut entry.task;
            task.title = request.title.clone();
            task.cover = request.cover.clone();
            task.bvid = request.bvid.clone();
            task.page = request.page;
            task.quality = quality_display;
            task.quality_id = request.quality_id;
            task.codec = request.codec.clone();
            task.mode = request.mode.clone();
            task.filename = filename;
            task.path = path.to_string_lossy().into_owned();
            task.status = "queued".into();
            task.phase = String::new();
            task.downloaded = 0;
            task.total = None;
            task.speed = 0;
            task.error = None;
            entry.cancel = cancel.clone();
            entry.task.clone()
        };
        save_history(&manager.history_path, &DownloadManager::snapshot(&tasks))?;
        updated
    };
    let _ = app.emit("download-progress", &updated);

    let manager_for_task = manager.clone();
    let app_for_task = app.clone();
    let request_for_task = request.clone();
    let id_for_task = id.clone();
    let directory_for_task = directory.clone();
    let ffmpeg_for_task = ffmpeg.clone();
    let filename_for_task = updated.filename.clone();
    tauri::async_runtime::spawn(async move {
        let result = run(
            &app_for_task,
            &manager_for_task,
            &id_for_task,
            &request_for_task,
            &directory_for_task,
            &filename_for_task,
            &ffmpeg_for_task,
            &cancel,
        )
        .await;
        match result {
            Ok(final_path) => manager_for_task.update(&app_for_task, &id_for_task, |task| {
                if let Ok(metadata) = std::fs::metadata(&final_path) {
                    task.downloaded = metadata.len();
                    task.total = Some(metadata.len());
                }
                task.filename = final_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                task.path = final_path.to_string_lossy().into_owned();
                task.status = "completed".into();
                task.phase = String::new();
                task.speed = 0;
            }),
            Err(message) => {
                let cancelled = message == "下载已取消";
                manager_for_task.update(&app_for_task, &id_for_task, |task| {
                    task.status = if cancelled { "cancelled" } else { "failed" }.into();
                    task.phase = String::new();
                    task.speed = 0;
                    task.error = if cancelled { None } else { Some(message) };
                });
            }
        }
    });
    Ok(())
}

async fn run(
    app: &AppHandle,
    manager: &DownloadManager,
    id: &str,
    request: &DownloadRequest,
    directory: &Path,
    filename: &str,
    ffmpeg: &str,
    cancel: &CancellationToken,
) -> Result<PathBuf, String> {
    let _permit = manager.slots.acquire(cancel).await?;
    manager.update(app, id, |task| {
        task.status = "running".into();
        task.phase = "video".into();
    });

    // Stage beside the destination so final rename never crosses volumes.
    let temp_dir = directory.join(format!(".biliscope-{id}.tmp"));
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| format!("创建临时目录失败：{e}"))?;
    let output = directory.join(filename);
    let media = media_client()?;

    let result = async {
    if request.mode == "audio" {
        let source = temp_dir.join("audio-source");
        download_stream(app, manager, &media, id, "audio", &request.url, &request.backup, &source, cancel).await?;
        manager.update(app, id, |task| {
            task.status = "merging".into();
            task.phase = "audio".into();
            task.speed = 0;
        });
        merge(&source, None, &output, ffmpeg, cancel).await?;
        Ok(output)
    } else if request.mode == "dash" {
        let video_path = temp_dir.join("video.m4s");
        let audio_path = temp_dir.join("audio.m4s");
        download_stream(
            app,
            manager,
            &media,
            id,
            "video",
            &request.url,
            &request.backup,
            &video_path,
            cancel,
        )
        .await?;
        let audio_url = request.audio_url.as_deref().ok_or("缺少音频地址")?;
        download_stream(
            app,
            manager,
            &media,
            id,
            "audio",
            audio_url,
            &request.audio_backup,
            &audio_path,
            cancel,
        )
        .await?;
        manager.update(app, id, |task| {
            task.status = "merging".into();
            task.phase = "merge".into();
            task.speed = 0;
        });
        merge(&video_path, Some(&audio_path), &output, ffmpeg, cancel).await?;
        Ok(output)
    } else {
        let partial = temp_dir.join("download.part");
        download_stream(
            app,
            manager,
            &media,
            id,
            "视频",
            &request.url,
            &request.backup,
            &partial,
            cancel,
        )
        .await?;
        if output.exists() {
            return Err("目标文件已存在".into());
        }
        tokio::fs::rename(&partial, &output)
            .await
            .map_err(|e| format!("保存文件失败：{e}"))?;
        Ok(output)
    }
    }.await;

    // v1 has no resume, so partial files are never reused: always clean up.
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    result
}

fn media_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(crate::client::UA)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(3600))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|_| "无法创建下载连接".to_string())
}

async fn download_stream(
    app: &AppHandle,
    manager: &DownloadManager,
    media: &reqwest::Client,
    id: &str,
    phase: &str,
    url: &str,
    backup: &[String],
    path: &Path,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let mut candidates = vec![url.to_string()];
    candidates.extend(backup.iter().filter(|item| !item.is_empty()).cloned());
    let mut last_error = String::new();

    for (index, candidate) in candidates.iter().enumerate() {
        if !valid_media_url(candidate) {
            last_error = "媒体地址不合法".into();
            continue;
        }
        if index > 0 {
            manager.update(app, id, |task| {
                task.phase = format!("{phase}（备用线路 {index}）");
            });
        }
        match download_one(app, manager, media, id, phase, candidate, path, cancel).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = error;
                let _ = tokio::fs::remove_file(path).await;
                if cancel.is_cancelled() {
                    return Err("下载已取消".into());
                }
            }
        }
    }

    Err(if last_error.is_empty() {
        "下载失败".into()
    } else {
        last_error
    })
}

async fn download_one(
    app: &AppHandle,
    manager: &DownloadManager,
    media: &reqwest::Client,
    id: &str,
    phase: &str,
    url: &str,
    path: &Path,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let mut response = tokio::select! {
        _ = cancel.cancelled() => return Err("下载已取消".into()),
        result = media.get(url).header(reqwest::header::REFERER, crate::client::REFERER_URL).send() => {
            result.map_err(|_| "连接视频服务器失败，请重试".to_string())?
        }
    };
    if !response.status().is_success() {
        return Err(format!("视频服务器返回 HTTP {}", response.status()));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !(content_type.starts_with("video/")
        || content_type.starts_with("audio/")
        || content_type.starts_with("application/mp4")
        || content_type == "application/octet-stream")
    {
        return Err("视频服务器返回了非媒体内容".into());
    }
    let total = response.content_length();
    manager.update(app, id, |task| {
        task.phase = phase.to_string();
        task.downloaded = 0;
        task.total = total;
        task.speed = 0;
    });

    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|e| format!("创建临时文件失败：{e}"))?;
    let mut downloaded = 0u64;
    let mut last_bytes = 0u64;
    let mut last_update = Instant::now();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err("下载已取消".into()),
            result = response.chunk() => result.map_err(|_| "下载中断，请重试".to_string())?,
        };
        let Some(chunk) = chunk else { break };
        tokio::select! {
            _ = cancel.cancelled() => return Err("下载已取消".into()),
            result = file.write_all(&chunk) => result.map_err(|e| format!("写入文件失败：{e}"))?,
        }
        downloaded += chunk.len() as u64;
        let elapsed = last_update.elapsed().as_secs_f64();
        if elapsed >= 0.25 {
            let speed = ((downloaded - last_bytes) as f64 / elapsed) as u64;
            manager.update(app, id, |task| {
                task.downloaded = downloaded;
                task.speed = speed;
            });
            last_bytes = downloaded;
            last_update = Instant::now();
        }
    }
    file.flush()
        .await
        .map_err(|e| format!("写入文件失败：{e}"))?;
    if downloaded == 0 || total.is_some_and(|size| downloaded != size) {
        return Err("媒体数据不完整，请重试".into());
    }
    manager.update(app, id, |task| {
        task.downloaded = downloaded;
        task.speed = 0;
    });
    Ok(())
}

fn audio_extension(format: &str) -> &str {
    match format {
        "flac" => "flac",
        "eac3" => "eac3",
        "mka" => "mka",
        _ => "m4a",
    }
}

fn media_output_command(ffmpeg: &str, source: &Path, audio: Option<&Path>, output: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(ffmpeg);
    command.arg("-n").arg("-i").arg(source);
    if let Some(audio) = audio {
        command.arg("-i").arg(audio).args(["-c", "copy"]);
    } else {
        command.args(["-map", "0:a:0", "-vn", "-c:a", "copy"]);
    }
    if audio.is_some() || output.extension().and_then(|s| s.to_str()) == Some("m4a") {
        command.args(["-movflags", "+faststart"]);
    }
    command.arg(output);
    command
}

async fn merge(
    video: &Path,
    audio: Option<&Path>,
    output: &Path,
    ffmpeg: &str,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let log_path = output.with_extension("ffmpeg.log");
    let log = std::fs::File::create(&log_path).map_err(|e| format!("创建 ffmpeg 日志失败：{e}"))?;
    let mut command = media_output_command(ffmpeg, video, audio, output);
    command        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log));
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动 ffmpeg 失败：{e}"))?;
    let status = tokio::select! {
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            let _ = tokio::fs::remove_file(&log_path).await;
            return Err("下载已取消".into());
        }
        result = child.wait() => result.map_err(|e| format!("等待 ffmpeg 失败：{e}"))?,
    };
    if !status.success() {
        let text = std::fs::read_to_string(&log_path).unwrap_or_default();
        let last = text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("未知错误")
            .trim()
            .to_string();
        let _ = std::fs::remove_file(&log_path);
        if text.contains("Permission denied") {
            return Err(format!(
                "ffmpeg 无法写入保存目录（权限被拒绝）：{last}。请换一个可写的保存位置，或检查 Windows 受控文件夹访问/安全软件。"
            ));
        }
        return Err(format!("FFmpeg 处理失败：{last}"));
    }
    let _ = std::fs::remove_file(&log_path);
    Ok(())
}

pub fn resolve_ffmpeg() -> FfmpegStatus {
    let env_path = std::env::var("BILISCOPE_FFMPEG").unwrap_or_default();
    let mut candidates: Vec<(String, &'static str)> = Vec::new();

    let env_path = env_path.trim();
    if !env_path.is_empty() {
        candidates.push((env_path.to_string(), "environment"));
    }
    #[cfg(debug_assertions)]
    {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/ffmpeg/ffmpeg.exe");
        if local.is_file() {
            candidates.push((local.to_string_lossy().into_owned(), "development"));
        }
    }
    candidates.push(("ffmpeg".into(), "path"));

    for (candidate, source) in candidates {
        if let Ok(output) = std::process::Command::new(&candidate)
            .arg("-version")
            .output()
        {
            if output.status.success() {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let version = text.lines().next().unwrap_or("").trim().to_string();
                let path = if source == "path" {
                    resolve_command_path(&candidate).unwrap_or(candidate)
                } else {
                    candidate
                };
                return FfmpegStatus {
                    found: true,
                    path,
                    version,
                    source: source.to_string(),
                };
            }
        }
    }
    FfmpegStatus {
        found: false,
        path: String::new(),
        version: String::new(),
        source: "none".into(),
    }
}

#[cfg(windows)]
fn resolve_command_path(command: &str) -> Option<String> {
    let output = std::process::Command::new("where.exe")
        .arg(command)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(not(windows))]
fn resolve_command_path(_command: &str) -> Option<String> {
    None
}

pub fn valid_media_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.host_str().is_some_and(|host| {
                !host.is_empty()
                    && !host.eq_ignore_ascii_case("localhost")
                    && !host.starts_with("127.")
                    && host != "0.0.0.0"
            })
    })
}

fn sanitize_component(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control()
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let limited: String = trimmed.chars().take(80).collect();
    if limited.is_empty() {
        "bilibili_video".into()
    } else {
        limited
    }
}

fn unique_filename(dir: &Path, base: &str, ext: &str) -> String {
    let mut index = 0;
    loop {
        let name = if index == 0 {
            format!("{base}.{ext}")
        } else {
            format!("{base} ({index}).{ext}")
        };
        if !dir.join(&name).exists() {
            return name;
        }
        index += 1;
        if index > 1000 {
            return format!("{base}.{ext}");
        }
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn load_history(path: &Path) -> Vec<DownloadTask> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return Vec::new(),
    };
    let mut tasks: Vec<DownloadTask> = serde_json::from_str(&text).unwrap_or_default();
    for task in &mut tasks {
        task.speed = 0;
        if matches!(task.status.as_str(), "queued" | "running" | "merging") {
            task.status = "failed".into();
            task.phase = String::new();
            task.error = Some("应用重启，下载已中断，请重新下载".into());
        } else if task.status == "completed" {
            // Older records stored the last stream's byte count (audio) instead
            // of the merged file size; repair it from disk when possible.
            if let Ok(metadata) = std::fs::metadata(&task.path) {
                task.downloaded = metadata.len();
                task.total = Some(metadata.len());
            }
        }
    }
    tasks
}

fn save_history(path: &Path, tasks: &[DownloadTask]) -> Result<(), String> {
    let parent = path.parent().ok_or("下载记录路径无效")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("创建下载记录目录失败：{e}"))?;
    let bytes = serde_json::to_vec_pretty(tasks).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".downloads-{}.tmp", Uuid::new_v4().simple()));
    std::fs::write(&temp, &bytes).map_err(|e| format!("写入下载记录失败：{e}"))?;
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::write(path, &bytes).map_err(|e| format!("保存下载记录失败：{e}"))?;
            let _ = std::fs::remove_file(&temp);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_audio_uses_native_container_without_transcoding() {
        for format in ["flac", "eac3", "mka"] {
            assert_eq!(audio_extension(format), format);
            let output = PathBuf::from(format!("audio.{}", audio_extension(format)));
            let command = media_output_command("ffmpeg", Path::new("audio.m4s"), None, &output);
            let args: Vec<_> = command.as_std().get_args().map(|s| s.to_string_lossy().into_owned()).collect();
            assert!(args.windows(2).any(|a| a == ["-c:a", "copy"]));
            assert!(!args.contains(&"-movflags".to_string()));
        }
        assert_eq!(audio_extension("../../bad"), "m4a");
    }

    #[test]
    fn audio_output_selects_only_audio_and_never_overwrites() {
        let command = media_output_command("ffmpeg", Path::new("input.mp4"), None, Path::new("output.m4a"));
        let args: Vec<_> = command.as_std().get_args().map(|s| s.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|a| a == ["-map", "0:a:0"]));
        assert!(args.contains(&"-vn".to_string()));
        assert!(args.windows(2).any(|a| a == ["-c:a", "copy"]));
        assert!(!args.contains(&"-b:a".to_string()));
        assert!(args.contains(&"-n".to_string()));
        assert!(!args.contains(&"-y".to_string()));
        assert_eq!(args.last().unwrap(), "output.m4a");
        let video = media_output_command("ffmpeg", Path::new("video.m4s"), Some(Path::new("audio.m4s")), Path::new("output.mp4"));
        let args: Vec<_> = video.as_std().get_args().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 2);
        assert!(args.windows(2).any(|a| a == ["-c", "copy"]));
        assert!(!args.contains(&"-vn".to_string()));
    }

    #[test]
    fn sanitizes_titles_and_avoids_unsafe_names() {
        assert_eq!(sanitize_component("a/b:c*?"), "a_b_c__");
        assert_eq!(sanitize_component("   "), "bilibili_video");
        assert!(sanitize_component(&"长".repeat(200)).chars().count() <= 80);
    }

    #[test]
    fn accepts_https_media_and_rejects_credentials_or_plain_http() {
        assert!(valid_media_url("https://upos-sz-mirror.bilivideo.com/a.m4s?x=1"));
        assert!(valid_media_url("https://x.edge.mountaintoys.cn:4483/a.mp4"));
        assert!(!valid_media_url("http://example.com/a.mp4"));
        assert!(!valid_media_url("https://user:pass@example.com/a.mp4"));
        assert!(!valid_media_url("https://127.0.0.1/a.mp4"));
    }

    #[test]
    fn unique_filename_skips_existing_files() {
        let root = std::env::temp_dir().join(format!("biliscope-name-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.mp4"), b"x").unwrap();
        assert_eq!(unique_filename(&root, "a", "mp4"), "a (1).mp4");
        std::fs::remove_dir_all(root).unwrap();
    }
}
