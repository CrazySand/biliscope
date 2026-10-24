mod auth;
mod client;
mod comments;
mod commands;
mod download;
mod models;
mod settings;
mod video;
mod wbi;

use commands::AppState;
use std::sync::Arc;
use tauri::Manager;

pub fn run() {
    let command_handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        commands::get_settings,
        commands::save_settings,
        commands::login_state,
        commands::login_qr_generate,
        commands::login_qr_poll,
        commands::logout,
        commands::fetch_video,
        commands::start_download,
        commands::list_tasks,
        commands::retry_download,
        commands::cancel_download,
        commands::remove_download,
        commands::reveal_download,
        commands::check_ffmpeg,
        commands::start_comment_crawl,
        commands::cancel_comment_crawl,
        commands::list_comment_jobs,
        commands::list_comment_videos,
        commands::add_comment_video,
        commands::delete_comment_video,
        commands::comment_stats,
        commands::list_comments,
        commands::export_comments,
    ];

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let settings = settings::load(app.handle()).map_err(std::io::Error::other)?;
            let cookie = settings
                .inner
                .lock()
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .cookie
                .clone();
            let client = client::ApiClient::new(&cookie).map_err(std::io::Error::other)?;
            let history_path = settings.data_dir.join("downloads.json");
            let downloads = download::DownloadManager::load(history_path)
                .map_err(std::io::Error::other)?;
            let limit = settings
                .inner
                .lock()
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .max_concurrent_or_default();
            downloads.set_limit(limit).map_err(std::io::Error::other)?;
            let comments =
                comments::CommentCrawlManager::new(settings.data_dir.join("comments.db"));

            let handle = app.handle().clone();
            app.manage(AppState {
                client,
                settings,
                downloads: Arc::new(downloads),
                comments: Arc::new(comments),
            });

            tauri::async_runtime::spawn(async move {
                let client = {
                    let state = handle.state::<AppState>();
                    state.client.clone()
                };
                let _ = client.bootstrap().await;
            });
            Ok(())
        })
        .invoke_handler(move |invoke| {
            if invoke.message.webview_ref().label() != "main" {
                invoke.resolver.reject("此窗口无权调用本地应用功能");
                return true;
            }
            command_handler(invoke)
        })
        .run(tauri::generate_context!())
        .expect("BiliScope 启动失败");
}
