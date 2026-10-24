use crate::client::ApiClient;
use crate::models::{LoginState, QrCode, QrPoll};

const QR_GENERATE: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/generate";
const QR_POLL: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";
const NAV: &str = "https://api.bilibili.com/x/web-interface/nav";

pub async fn qr_generate(client: &ApiClient) -> Result<QrCode, String> {
    if !client.has_cookie("buvid3") {
        client.bootstrap().await?;
    }
    let value = client.get_json(QR_GENERATE, &[]).await?;
    let data = value.get("data").ok_or("扫码接口返回数据为空")?;
    let url = data
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("扫码接口未返回二维码地址")?;
    let key = data
        .get("qrcode_key")
        .and_then(|v| v.as_str())
        .ok_or("扫码接口未返回二维码密钥")?;
    Ok(QrCode {
        url: url.to_string(),
        key: key.to_string(),
    })
}

/// `data.code`: 86101 not scanned, 86090 scanned/waiting, 86038 expired, 0 success.
pub async fn qr_poll(client: &ApiClient, key: &str) -> Result<QrPoll, String> {
    let value = client
        .get_json(QR_POLL, &[("qrcode_key", key.to_string())])
        .await?;
    let data = value.get("data").ok_or("扫码状态返回数据为空")?;
    let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
    let message = data
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(QrPoll { code, message })
}

pub async fn current_user(client: &ApiClient) -> Result<LoginState, String> {
    if client.cookie_header().is_empty() {
        return Ok(LoginState::default());
    }
    let value = client.get_raw(NAV, &[]).await?;
    let data = value
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Ok(LoginState {
        logged_in: data
            .get("isLogin")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        uname: data
            .get("uname")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        mid: data.get("mid").and_then(|v| v.as_u64()).unwrap_or(0),
        avatar: data
            .get("face")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        vip: data.get("vipStatus").and_then(|v| v.as_i64()).unwrap_or(0) == 1,
    })
}
