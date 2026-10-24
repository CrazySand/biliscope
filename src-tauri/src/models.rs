use serde::{Deserialize, Serialize};

fn default_max_concurrent() -> u8 {
    2
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub download_dir: String,
    #[serde(default)]
    pub cookie: String,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u8,
}

impl Settings {
    pub fn max_concurrent_or_default(&self) -> u8 {
        if (1..=4).contains(&self.max_concurrent) {
            self.max_concurrent
        } else {
            default_max_concurrent()
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub data_dir: String,
    pub download_dir: String,
    pub max_concurrent: u8,
    pub has_cookie: bool,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoginState {
    pub logged_in: bool,
    pub uname: String,
    pub mid: u64,
    pub avatar: String,
    pub vip: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrCode {
    pub url: String,
    pub key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrPoll {
    pub code: i32,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub page: u32,
    pub cid: u64,
    pub part: String,
    pub duration: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityOption {
    pub id: u32,
    pub label: String,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: String,
    pub bandwidth: u64,
    pub size: Option<u64>,
    /// `dash` needs ffmpeg merging; `durl` is a single MP4 with audio.
    pub mode: String,
    pub url: String,
    pub backup: Vec<String>,
    pub audio_label: String,
    #[serde(default)]
    pub audio_format: String,
    pub audio_url: Option<String>,
    pub audio_backup: Vec<String>,
    pub audio_size: Option<u64>,
    pub needs_vip: bool,
    pub recommended: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDetail {
    pub bvid: String,
    pub aid: u64,
    pub title: String,
    pub cover: String,
    pub duration: u64,
    pub owner: String,
    pub pages: Vec<PageInfo>,
    pub page: u32,
    pub qualities: Vec<QualityOption>,
    pub accept: Vec<u32>,
    pub logged_in: bool,
    pub hint: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    pub id: String,
    pub created_at: u64,
    pub title: String,
    pub cover: String,
    pub bvid: String,
    pub page: u32,
    pub quality: String,
    #[serde(default)]
    pub quality_id: u32,
    #[serde(default)]
    pub codec: String,
    #[serde(default)]
    pub mode: String,
    pub filename: String,
    pub path: String,
    pub status: String,
    pub phase: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: u64,
    pub error: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub title: String,
    pub cover: String,
    pub bvid: String,
    pub page: u32,
    pub quality: String,
    pub mode: String,
    #[serde(default)]
    pub quality_id: u32,
    #[serde(default)]
    pub codec: String,
    pub url: String,
    pub backup: Vec<String>,
    pub audio_url: Option<String>,
    pub audio_backup: Vec<String>,
    pub audio_label: String,
    #[serde(default)]
    pub audio_format: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegStatus {
    pub found: bool,
    pub path: String,
    pub version: String,
    /// environment | development (debug only) | path | none
    pub source: String,
}
