use crate::auth;
use crate::client::ApiClient;
use crate::comments::{
    self, CommentCrawlJob, CommentCrawlManager, CommentRow, CommentStats, CommentStore, CommentVideo, ExportResult,
};
use crate::download::{self, DownloadManager};
use crate::models::{
    DownloadRequest, DownloadTask, FfmpegStatus, LoginState, QrCode, QrPoll, Settings,
    SettingsView, VideoDetail,
};
use crate::settings::{self, AppSettings};
use crate::video;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

pub struct AppState {
    pub client: ApiClient,
    pub settings: AppSettings,
    pub downloads: Arc<DownloadManager>,
    pub comments: Arc<CommentCrawlManager>,
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<SettingsView, String> {
    settings::view(&state.settings)
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<SettingsView, String> {
    let view = settings::save(&state.settings, settings)?;
    state.downloads.set_limit(view.max_concurrent)?;
    let _ = app.emit("settings-changed", &view);
    Ok(view)
}

#[tauri::command]
pub async fn login_state(state: State<'_, AppState>) -> Result<LoginState, String> {
    auth::current_user(&state.client).await
}

#[tauri::command]
pub async fn login_qr_generate(state: State<'_, AppState>) -> Result<QrCode, String> {
    auth::qr_generate(&state.client).await
}

#[tauri::command]
pub async fn login_qr_poll(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<QrPoll, String> {
    let poll = auth::qr_poll(&state.client, &key).await?;
    if poll.code == 0 {
        let cookie = state.client.cookie_header();
        if cookie.is_empty() {
            return Err("登录成功但没有拿到 Cookie，请重试".into());
        }
        settings::set_cookie(&state.settings, cookie)?;
        let _ = app.emit("login-changed", true);
    }
    Ok(poll)
}

#[tauri::command]
pub async fn logout(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.client.clear_cookies();
    settings::clear_cookie(&state.settings)?;
    let _ = state.client.bootstrap().await;
    let _ = app.emit("login-changed", false);
    Ok(())
}

#[tauri::command]
pub async fn fetch_video(
    state: State<'_, AppState>,
    input: String,
    page: Option<u32>,
) -> Result<VideoDetail, String> {
    let logged_in = auth::current_user(&state.client)
        .await
        .map(|user| user.logged_in)
        .unwrap_or(false);
    video::fetch_detail(&state.client, &input, page, logged_in).await
}

#[tauri::command]
pub fn start_download(
    app: AppHandle,
    state: State<'_, AppState>,
    request: DownloadRequest,
) -> Result<String, String> {
    let settings = state.settings.inner.lock().map_err(|e| e.to_string())?.clone();
    let ffmpeg = download::resolve_ffmpeg();
    if !ffmpeg.found {
        return Err(
            "所有视频和音频下载都需要 FFmpeg。请先安装并在设置中确认检测可用。"
                .into(),
        );
    }
    download::start(
        app,
        state.downloads.clone(),
        request,
        std::path::PathBuf::from(&settings.download_dir),
        ffmpeg.path,
    )
}

#[tauri::command]
pub fn list_tasks(state: State<'_, AppState>) -> Result<Vec<DownloadTask>, String> {
    state.downloads.list()
}

#[tauri::command]
pub async fn retry_download(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let task = state.downloads.get(&id)?;
    if !matches!(task.status.as_str(), "failed" | "cancelled") {
        return Err("只有失败或已取消的任务可以重试".into());
    }

    let ffmpeg = download::resolve_ffmpeg();
    if !ffmpeg.found {
        return Err(
            "所有视频和音频下载都需要 FFmpeg。请先安装并在设置中确认检测可用。"
                .into(),
        );
    }

    // Re-fetch playurl instead of persisting signed media URLs: the old URLs
    // may have expired by the time the user retries.
    let logged_in = auth::current_user(&state.client)
        .await
        .map(|user| user.logged_in)
        .unwrap_or(false);
    let detail = video::fetch_detail(&state.client, &task.bvid, Some(task.page), logged_in).await?;
    let quality = if task.mode == "audio" {
        detail.qualities.iter().find(|item| item.audio_url.is_some())
            .or_else(|| detail.qualities.iter().find(|item| item.mode == "durl"))
    } else if task.quality_id > 0 {
        detail
            .qualities
            .iter()
            .find(|item| {
                item.id == task.quality_id && item.codec == task.codec && (task.mode == "audio" || item.mode == task.mode)
            })
            .or_else(|| {
                detail
                    .qualities
                    .iter()
                    .find(|item| item.id == task.quality_id && (task.mode == "audio" || item.mode == task.mode))
            })
    } else {
        // Records created before quality metadata was stored: match by label.
        let label = task.quality.split(" · ").next().unwrap_or("");
        detail.qualities.iter().find(|item| {
            (item.label == task.quality || item.label == label)
                && (task.mode.is_empty() || (task.mode == "audio" || item.mode == task.mode))
        })
    }
    .ok_or("当前清晰度不可用，请重新解析并选择")?;

    let request = DownloadRequest {
        title: detail.title.clone(),
        cover: detail.cover.clone(),
        bvid: detail.bvid.clone(),
        page: detail.page,
        quality: if task.mode == "audio" { format!("仅音频 · {}", quality.audio_label) } else { quality.label.clone() },
        mode: if task.mode == "audio" { "audio".into() } else { quality.mode.clone() },
        quality_id: quality.id,
        codec: quality.codec.clone(),
        url: if task.mode == "audio" { quality.audio_url.clone().unwrap_or_else(|| quality.url.clone()) } else { quality.url.clone() },
        backup: if task.mode == "audio" && quality.audio_url.is_some() { quality.audio_backup.clone() } else { quality.backup.clone() },
        audio_url: quality.audio_url.clone(),
        audio_backup: quality.audio_backup.clone(),
        audio_label: quality.audio_label.clone(),
        audio_format: quality.audio_format.clone(),
    };

    let settings = state
        .settings
        .inner
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    download::retry(
        app,
        state.downloads.clone(),
        id,
        request,
        std::path::PathBuf::from(&settings.download_dir),
        ffmpeg.path,
    )
}

#[tauri::command]
pub fn cancel_download(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.downloads.cancel(&id)
}

#[tauri::command]
pub fn remove_download(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.downloads.remove(&id)
}

#[tauri::command]
pub fn reveal_download(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let path = state.downloads.completed_path(&id)?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn check_ffmpeg() -> Result<FfmpegStatus, String> {
    Ok(download::resolve_ffmpeg())
}

#[tauri::command]
pub async fn start_comment_crawl(
    app: AppHandle,
    state: State<'_, AppState>,
    bvid: String,
    mode: Option<String>,
    max_pages: Option<u32>,
) -> Result<CommentCrawlJob, String> {
    let logged_in = auth::current_user(&state.client)
        .await
        .map(|user| user.logged_in)
        .unwrap_or(false);
    comments::start(
        app,
        state.comments.clone(),
        state.client.clone(),
        bvid,
        mode.unwrap_or_else(|| "hot".into()),
        max_pages,
        logged_in,
    )
}

#[tauri::command]
pub fn cancel_comment_crawl(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.comments.cancel(&id)
}

#[tauri::command]
pub fn list_comment_jobs(state: State<'_, AppState>) -> Result<Vec<CommentCrawlJob>, String> {
    state.comments.list()
}

#[tauri::command]
pub fn list_comment_videos(state: State<'_, AppState>) -> Result<Vec<CommentVideo>, String> {
    CommentStore::open(state.comments.db_path())?.videos()
}

#[tauri::command]
pub fn comment_stats(
    state: State<'_, AppState>,
    bvid: String,
) -> Result<CommentStats, String> {
    let store = CommentStore::open(state.comments.db_path())?;
    store.counts(&bvid)
}

#[tauri::command]
pub fn list_comments(
    state: State<'_, AppState>,
    bvid: String,
    offset: Option<u32>,
    limit: Option<u32>,
    roots_only: Option<bool>,
) -> Result<Vec<CommentRow>, String> {
    let store = CommentStore::open(state.comments.db_path())?;
    store.list(
        &bvid,
        offset.unwrap_or(0),
        limit.unwrap_or(50).min(500),
        roots_only.unwrap_or(false),
    )
}

#[tauri::command]
pub fn export_comments(
    state: State<'_, AppState>,
    bvid: String,
    format: String,
    path: String,
) -> Result<ExportResult, String> {
    let path = std::path::PathBuf::from(path);
    if !path.is_absolute() {
        return Err("导出路径必须是绝对路径".into());
    }
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Err("导出目录不存在".into());
        }
    }
    let store = CommentStore::open(state.comments.db_path())?;
    let count = match format.as_str() {
        "csv" => store.export_csv(&bvid, &path)?,
        "json" => store.export_json(&bvid, &path)?,
        _ => return Err("不支持的导出格式".into()),
    };
    Ok(ExportResult {
        path: path.to_string_lossy().into_owned(),
        count,
    })
}

#[tauri::command]
pub fn delete_comment_video(state: State<'_, AppState>, bvid: String) -> Result<(), String> {
    state.comments.delete_video(&bvid)
}

#[tauri::command]
pub fn add_comment_video(
    state: State<'_, AppState>,
    bvid: String,
    aid: u64,
    title: String,
) -> Result<(), String> {
    if !bvid.starts_with("BV") || bvid.len() != 12 || !bvid.bytes().all(|c| c.is_ascii_alphanumeric()) || aid == 0 || aid > i64::MAX as u64 || title.trim().is_empty() {
        return Err("视频信息无效，请重新解析视频".into());
    }
    CommentStore::open(state.comments.db_path())?.touch_video(&bvid, aid, &title)
}
