use crate::client::ApiClient;
use crate::models::{PageInfo, QualityOption, VideoDetail};
use serde_json::Value;

const VIEW_URL: &str = "https://api.bilibili.com/x/web-interface/view";
const PLAYURL_URL: &str = "https://api.bilibili.com/x/player/playurl";

pub fn extract_bvid(input: &str) -> Option<String> {
    let start = input.find("BV")?;
    let candidate: String = input[start..].chars().take(12).collect();
    if candidate.chars().count() == 12 && candidate.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(candidate)
    } else {
        None
    }
}

pub fn extract_page(input: &str) -> Option<u32> {
    let index = input.find("p=")?;
    let digits: String = input[index + 2..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().ok().filter(|page| *page >= 1)
}

pub(crate) async fn fetch_view(client: &ApiClient, bvid: &str) -> Result<Value, String> {
    let value = client
        .get_json(VIEW_URL, &[("bvid", bvid.to_string())])
        .await?;
    value
        .get("data")
        .cloned()
        .ok_or_else(|| "视频信息为空".to_string())
}

async fn playurl(client: &ApiClient, bvid: &str, cid: u64, fnval: u32) -> Result<Value, String> {
    let value = client
        .get_json(
            PLAYURL_URL,
            &[
                ("bvid", bvid.to_string()),
                ("cid", cid.to_string()),
                ("qn", "127".to_string()),
                ("fnval", fnval.to_string()),
                ("fnver", "0".to_string()),
                ("fourk", "1".to_string()),
            ],
        )
        .await?;
    Ok(value
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({})))
}

pub async fn fetch_detail(
    client: &ApiClient,
    input: &str,
    page: Option<u32>,
    logged_in: bool,
) -> Result<VideoDetail, String> {
    let bvid = extract_bvid(input).ok_or("无法识别 BV 号，请粘贴视频链接或 BV 号")?;
    let view = fetch_view(client, &bvid).await?;

    let aid = view.get("aid").and_then(Value::as_u64).unwrap_or(0);
    let title = view
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("B 站视频")
        .to_string();
    let cover = view.get("pic").and_then(Value::as_str).unwrap_or("").to_string();
    let owner = view
        .pointer("/owner/name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let pages: Vec<PageInfo> = view
        .get("pages")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| PageInfo {
                    page: item.get("page").and_then(Value::as_u64).unwrap_or(1) as u32,
                    cid: item.get("cid").and_then(Value::as_u64).unwrap_or(0),
                    part: item
                        .get("part")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    duration: item.get("duration").and_then(Value::as_u64).unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();

    let requested = page.or_else(|| extract_page(input)).unwrap_or(1);
    let current = pages
        .iter()
        .find(|item| item.page == requested)
        .or_else(|| pages.first())
        .cloned()
        .ok_or("该视频没有可用分P")?;
    if current.cid == 0 {
        return Err("该视频缺少 cid，无法解析播放地址".into());
    }

    // High quality channel (DASH, needs ffmpeg) + compatibility channel
    // (single-file MP4, no merge needed).
    let dash_data = playurl(client, &bvid, current.cid, 4048).await?;
    let durl = playurl(client, &bvid, current.cid, 1)
        .await
        .unwrap_or_else(|_| serde_json::json!({}));
    let dash = dash_data
        .get("dash")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let qualities = build_qualities(&dash, &durl, current.duration);
    let accept = dash_data
        .get("accept_quality")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_u64().map(|value| value as u32))
                .collect()
        })
        .unwrap_or_default();

    let hint = if qualities.is_empty() {
        "未解析到可下载的视频流。请确认视频可正常播放，或稍后重试。".to_string()
    } else if !logged_in {
        "未登录：DASH 最高 480P，单文件最高 720P；扫码登录后可解锁更高清晰度。".to_string()
    } else {
        "已登录：清晰度以 B 站实际下发为准；4K / HDR / 杜比需要大会员。".to_string()
    };

    Ok(VideoDetail {
        bvid,
        aid,
        title,
        cover,
        duration: current.duration,
        owner,
        pages,
        page: current.page,
        qualities,
        accept,
        logged_in,
        hint,
    })
}

struct AudioChoice {
    label: String,
    format: String,
    url: String,
    backup: Vec<String>,
    size: Option<u64>,
}

fn pick_audio(dash: &Value, duration: u64) -> Option<AudioChoice> {
    let mut candidates: Vec<(u32, String, String, Vec<String>, u64, String)> = Vec::new();
    let mut push = |item: &Value| {
        let id = item.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
        let url = item
            .get("baseUrl")
            .or_else(|| item.get("base_url"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if url.is_empty() {
            return;
        }
        let backup = string_list(item.get("backupUrl").or_else(|| item.get("backup_url")));
        let bandwidth = item.get("bandwidth").and_then(Value::as_u64).unwrap_or(0);
        let codec = item.get("codecs").and_then(Value::as_str).unwrap_or("");
        let format = if codec.eq_ignore_ascii_case("flac") || id == 30251 { "flac" }
            else if codec == "ec-3" || id == 30250 { "eac3" }
            else if codec.starts_with("mp4a") || matches!(id, 30280 | 30232 | 30216) { "m4a" }
            else { "mka" };
        candidates.push((id, audio_name(id), url.to_string(), backup, bandwidth, format.into()));
    };

    if let Some(items) = dash.get("audio").and_then(Value::as_array) {
        for item in items {
            push(item);
        }
    }
    for container in ["dolby", "flac"] {
        if let Some(audio) = dash.get(container).and_then(|value| value.get("audio")) {
            match audio {
                Value::Array(items) => {
                    for item in items {
                        push(item);
                    }
                }
                Value::Object(_) => push(audio),
                _ => {}
            }
        }
    }
    drop(push);

    let best = candidates.into_iter().min_by(|a, b| {
        audio_rank(a.0)
            .cmp(&audio_rank(b.0))
            .then_with(|| b.4.cmp(&a.4))
    })?;
    let size = if best.4 > 0 && duration > 0 {
        Some(best.4.saturating_mul(duration) / 8)
    } else {
        None
    };
    Some(AudioChoice {
        format: best.5,
        label: best.1,
        url: best.2,
        backup: best.3,
        size,
    })
}

fn build_qualities(dash: &Value, durl: &Value, duration: u64) -> Vec<QualityOption> {
    let audio = pick_audio(dash, duration);
    let mut options = Vec::new();

    if let Some(videos) = dash.get("video").and_then(Value::as_array) {
        for item in videos {
            let id = item.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
            let codecs = item.get("codecs").and_then(Value::as_str).unwrap_or("");
            let url = item
                .get("baseUrl")
                .or_else(|| item.get("base_url"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if url.is_empty() {
                continue;
            }
            let width = item.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
            let height = item.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
            let fps = item.get("frameRate").and_then(Value::as_str).unwrap_or("");
            let bandwidth = item.get("bandwidth").and_then(Value::as_u64).unwrap_or(0);
            let backup = string_list(item.get("backupUrl").or_else(|| item.get("backup_url")));
            let size = if bandwidth > 0 && duration > 0 {
                Some(bandwidth.saturating_mul(duration) / 8)
            } else {
                None
            };
            options.push(QualityOption {
                id,
                label: quality_name(id).to_string(),
                codec: codec_name(codecs).to_string(),
                width,
                height,
                fps: fps_label(fps),
                bandwidth,
                size,
                mode: "dash".into(),
                url: url.to_string(),
                backup,
                audio_format: audio.as_ref().map(|item| item.format.clone()).unwrap_or_default(),
                audio_label: audio.as_ref().map(|item| item.label.clone()).unwrap_or_default(),
                audio_url: audio.as_ref().map(|item| item.url.clone()),
                audio_backup: audio
                    .as_ref()
                    .map(|item| item.backup.clone())
                    .unwrap_or_default(),
                audio_size: audio.as_ref().and_then(|item| item.size),
                needs_vip: id >= 120 || id == 125 || id == 126 || id == 127,
                recommended: false,
            });
        }
    }

    if let Some(items) = durl.get("durl").and_then(Value::as_array) {
        if items.len() == 1 {
            let item = &items[0];
            let url = item.get("url").and_then(Value::as_str).unwrap_or("");
            if !url.is_empty() {
                let id = durl.get("quality").and_then(Value::as_u64).unwrap_or(0) as u32;
                let size = item.get("size").and_then(Value::as_u64);
                let bandwidth = size
                    .map(|value| {
                        if duration > 0 {
                            value.saturating_mul(8) / duration
                        } else {
                            0
                        }
                    })
                    .unwrap_or(0);
                options.push(QualityOption {
                    id,
                    label: format!("{} · 单文件", quality_name(id)),
                    codec: "MP4".into(),
                    width: 0,
                    height: 0,
                    fps: String::new(),
                    bandwidth,
                    size,
                    mode: "durl".into(),
                    url: url.to_string(),
                    backup: string_list(item.get("backupUrl").or_else(|| item.get("backup_url"))),
                    audio_label: "含音轨".into(),
                    audio_format: "m4a".into(),
                    audio_url: None,
                    audio_backup: Vec::new(),
                    audio_size: None,
                    needs_vip: id >= 120,
                    recommended: false,
                });
            }
        }
    }

    options.sort_by(|a, b| {
        b.id.cmp(&a.id)
            .then_with(|| codec_rank(&a.codec).cmp(&codec_rank(&b.codec)))
            .then_with(|| b.bandwidth.cmp(&a.bandwidth))
    });
    options.dedup_by(|a, b| a.id == b.id && a.codec == b.codec && a.mode == b.mode);
    if let Some(first) = options.first_mut() {
        first.recommended = true;
    }
    options
}

fn quality_name(id: u32) -> &'static str {
    match id {
        127 => "8K 超高清",
        126 => "杜比视界",
        125 => "HDR 真彩",
        120 => "4K 超清",
        116 => "1080P60",
        112 => "1080P+",
        80 => "1080P",
        74 => "720P60",
        64 => "720P",
        32 => "480P",
        16 => "360P",
        _ => "未知清晰度",
    }
}

fn audio_name(id: u32) -> String {
    match id {
        30251 => "Hi-Res 无损",
        30250 => "杜比全景声",
        30280 => "192K AAC",
        30232 => "132K AAC",
        30216 => "64K AAC",
        _ => "音频",
    }
    .to_string()
}

fn audio_rank(id: u32) -> u8 {
    match id {
        30251 => 0,
        30250 => 1,
        30280 => 2,
        30232 => 3,
        30216 => 4,
        _ => 9,
    }
}

fn codec_name(codecs: &str) -> &'static str {
    if codecs.starts_with("avc1") {
        "AVC"
    } else if codecs.starts_with("hvc1") || codecs.starts_with("hev1") {
        "HEVC"
    } else if codecs.starts_with("av01") {
        "AV1"
    } else if codecs.starts_with("mp4a") {
        "AAC"
    } else if codecs.eq_ignore_ascii_case("flac") {
        "FLAC"
    } else if codecs.eq_ignore_ascii_case("ec-3") || codecs.eq_ignore_ascii_case("eac3") {
        "E-AC-3"
    } else if codecs.is_empty() {
        "-"
    } else {
        "OTHER"
    }
}

fn codec_rank(codec: &str) -> u8 {
    match codec {
        "AVC" => 0,
        "HEVC" => 1,
        "AV1" => 2,
        _ => 3,
    }
}

fn fps_label(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if let Ok(value) = raw.parse::<f64>() {
        return format!("{value:.0}");
    }
    if let Some((left, right)) = raw.split_once('/') {
        if let (Ok(a), Ok(b)) = (left.parse::<f64>(), right.parse::<f64>()) {
            if b != 0.0 {
                return format!("{:.0}", a / b);
            }
        }
    }
    raw.to_string()
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        Some(Value::String(text)) if !text.is_empty() => vec![text.clone()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_highest_available_audio_and_preserves_its_format() {
        let mut dash = serde_json::json!({
            "audio": [
                {"id":30216,"bandwidth":64000,"baseUrl":"https://example.com/low"},
                {"id":30280,"bandwidth":192000,"baseUrl":"https://example.com/high"}
            ],
            "dolby":{"audio":[{"id":30250,"baseUrl":"https://example.com/dolby"}]},
            "flac":{"audio":{"id":30251,"baseUrl":"https://example.com/lossless"}}
        });
        let best = pick_audio(&dash, 60).unwrap();
        assert_eq!(best.format, "flac");
        assert!(best.url.ends_with("lossless"));
        dash["flac"] = Value::Null;
        assert_eq!(pick_audio(&dash, 60).unwrap().format, "eac3");
        dash["dolby"] = Value::Null;
        let best = pick_audio(&dash, 60).unwrap();
        assert_eq!(best.format, "m4a");
        assert!(best.url.ends_with("high"));
    }

    #[test]
    fn parses_bv_and_page_from_links() {
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/BV1JsJ86TEQW/?p=2&spm_id_from=x").unwrap(),
            "BV1JsJ86TEQW"
        );
        assert_eq!(extract_bvid("BV1JsJ86TEQW").unwrap(), "BV1JsJ86TEQW");
        assert_eq!(
            extract_page("https://www.bilibili.com/video/BV1JsJ86TEQW/?p=3").unwrap(),
            3
        );
        assert!(extract_bvid("https://example.com/video/123").is_none());
        assert!(extract_page("https://www.bilibili.com/video/BV1JsJ86TEQW/").is_none());
    }

    #[test]
    fn names_quality_and_codec() {
        assert_eq!(quality_name(80), "1080P");
        assert_eq!(quality_name(127), "8K 超高清");
        assert_eq!(codec_name("avc1.640033"), "AVC");
        assert_eq!(codec_name("hvc1.1.6.L120.90"), "HEVC");
        assert_eq!(codec_name("av01.0.08M.08.0.110.01.01.01.0"), "AV1");
        assert_eq!(fps_label("16000/672"), "24");
        assert_eq!(fps_label("60"), "60");
    }

    #[test]
    fn builds_dash_and_single_file_options() {
        let dash = serde_json::json!({
            "duration": 100,
            "video": [
                {"id": 80, "codecs": "avc1.640028", "width": 1920, "height": 1080, "frameRate": "30", "bandwidth": 1000000, "baseUrl": "https://example.com/v.mp4", "backupUrl": ["https://example.com/b.mp4"]},
                {"id": 80, "codecs": "hvc1.1.6", "width": 1920, "height": 1080, "frameRate": "30", "bandwidth": 900000, "baseUrl": "https://example.com/v2.mp4"}
            ],
            "audio": [{"id": 30280, "bandwidth": 192000, "baseUrl": "https://example.com/a.m4s"}],
            "dolby": {"type": 0, "audio": null},
            "flac": null
        });
        let durl = serde_json::json!({
            "quality": 64,
            "durl": [{"url": "https://example.com/single.mp4", "size": 1234567}]
        });
        let options = build_qualities(&dash, &durl, 100);
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].id, 80);
        assert_eq!(options[0].codec, "AVC");
        assert!(options[0].recommended);
        assert_eq!(options[0].audio_label, "192K AAC");
        assert!(options.iter().any(|item| item.mode == "durl"));
    }

    /// Live protocol smoke test. Run with:
    /// `cargo test live_anonymous_fetch -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_anonymous_fetch() {
        let client = crate::client::ApiClient::new("").unwrap();
        client.bootstrap().await.unwrap();
        println!(
            "cookies: buvid3={} header_len={}",
            client.has_cookie("buvid3"),
            client.cookie_header().len()
        );
        let detail = fetch_detail(&client, "BV1JsJ86TEQW", None, false)
            .await
            .unwrap();
        println!(
            "title={} qualities={} first={} mode={} accept={:?}",
            detail.title,
            detail.qualities.len(),
            detail.qualities[0].label,
            detail.qualities[0].mode,
            detail.accept
        );
        let cid = detail.pages[0].cid;
        let dash = playurl(&client, &detail.bvid, cid, 4048).await.unwrap();
        println!(
            "dash keys: {:?}",
            dash.as_object()
                .map(|map| map.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        );
        assert_eq!(detail.bvid, "BV1JsJ86TEQW");
        assert!(!detail.qualities.is_empty());
    }
}
