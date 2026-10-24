use crate::models::{Settings, SettingsView};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

pub struct AppSettings {
    pub data_dir: PathBuf,
    path: PathBuf,
    pub inner: Mutex<Settings>,
}

pub fn load(app: &AppHandle) -> Result<AppSettings, String> {
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("应用数据目录不可用：{e}"))?;
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("创建应用数据目录失败：{e}"))?;
    let path = data_dir.join("settings.json");
    let mut settings = if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_else(|| default_settings(app))
    } else {
        default_settings(app)
    };
    if settings.download_dir.trim().is_empty() {
        settings.download_dir = default_dir(app);
    }
    Ok(AppSettings {
        data_dir,
        path,
        inner: Mutex::new(settings),
    })
}

fn default_dir(app: &AppHandle) -> String {
    app.path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("BiliScope")
        .to_string_lossy()
        .into_owned()
}

fn default_settings(app: &AppHandle) -> Settings {
    Settings {
        download_dir: default_dir(app),
        cookie: String::new(),
        max_concurrent: 2,
    }
}

pub fn view(state: &AppSettings) -> Result<SettingsView, String> {
    let settings = state.inner.lock().map_err(|e| e.to_string())?.clone();
    let has_cookie = !settings.cookie.is_empty();
    let max_concurrent = settings.max_concurrent_or_default();
    Ok(SettingsView {
        data_dir: state.data_dir.to_string_lossy().into_owned(),
        download_dir: settings.download_dir,
        max_concurrent,
        has_cookie,
    })
}

pub fn save(state: &AppSettings, settings: Settings) -> Result<SettingsView, String> {
    let dir = PathBuf::from(settings.download_dir.trim());
    if !dir.is_absolute() {
        return Err("请选择完整的下载目录路径".into());
    }
    if !(1..=4).contains(&settings.max_concurrent) {
        return Err("同时下载数量必须在 1–4 之间".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("下载目录不可用：{e}"))?;

    let mut stored = settings;
    stored.download_dir = dir.to_string_lossy().into_owned();
    {
        let current = state.inner.lock().map_err(|e| e.to_string())?;
        if stored.cookie.is_empty() {
            stored.cookie = current.cookie.clone();
        }
    }
    write_settings(state, &stored)?;
    *state.inner.lock().map_err(|e| e.to_string())? = stored;
    view(state)
}

pub fn set_cookie(state: &AppSettings, cookie: String) -> Result<(), String> {
    let mut settings = state.inner.lock().map_err(|e| e.to_string())?.clone();
    settings.cookie = cookie;
    write_settings(state, &settings)?;
    *state.inner.lock().map_err(|e| e.to_string())? = settings;
    Ok(())
}

pub fn clear_cookie(state: &AppSettings) -> Result<(), String> {
    set_cookie(state, String::new())
}

fn write_settings(state: &AppSettings, settings: &Settings) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    let temp = state
        .path
        .with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4().simple()));
    std::fs::write(&temp, &bytes).map_err(|e| format!("写入设置失败：{e}"))?;
    match std::fs::rename(&temp, &state.path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Some sandboxed/Windows setups reject rename-over-existing; fall
            // back to an in-place write so settings still persist.
            std::fs::write(&state.path, &bytes).map_err(|e| format!("保存设置失败：{e}"))?;
            let _ = std::fs::remove_file(&temp);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_download_dirs() {
        let settings = Settings {
            download_dir: "relative".into(),
            cookie: String::new(),
            max_concurrent: 2,
        };
        assert!(!PathBuf::from(&settings.download_dir).is_absolute());
    }
}
