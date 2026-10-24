use crate::client::ApiClient;
use md5::{Digest, Md5};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const NAV_URL: &str = "https://api.bilibili.com/x/web-interface/nav";

/// Bilibili's WBI mixin table (64 indices into `img_key + sub_key`).
const MIXIN_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22,
    25, 54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

// Same encoding as Python's `quote(value, safe="")`: unreserved characters
// (`-._~`) stay literal, everything else becomes %XX.
const WBI_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Fetch `wbi_img` from nav and derive the 32-char mixin key.
pub async fn fetch_mixin_key(client: &ApiClient) -> Result<String, String> {
    let value = client.get_raw(NAV_URL, &[]).await?;
    let wbi = value
        .pointer("/data/wbi_img")
        .ok_or("nav 响应缺少 wbi_img，无法生成签名")?;
    let img = wbi
        .get("img_url")
        .and_then(Value::as_str)
        .ok_or("nav 响应缺少 img_url")?;
    let sub = wbi
        .get("sub_url")
        .and_then(Value::as_str)
        .ok_or("nav 响应缺少 sub_url")?;
    build_mixin_key(key_from_url(img)?, key_from_url(sub)?)
}

fn key_from_url(url: &str) -> Result<&str, String> {
    let file = url
        .rsplit('/')
        .next()
        .and_then(|name| name.split('.').next())
        .filter(|name| !name.is_empty())
        .ok_or("wbi_url 格式异常")?;
    Ok(file)
}

fn build_mixin_key(img_key: &str, sub_key: &str) -> Result<String, String> {
    let origin = format!("{img_key}{sub_key}");
    let bytes = origin.as_bytes();
    if bytes.len() < 64 {
        return Err("wbi 密钥长度不足".into());
    }
    let mut mixed = String::with_capacity(32);
    for &index in MIXIN_TAB.iter().take(32) {
        mixed.push(bytes[index] as char);
    }
    Ok(mixed)
}

fn encode_wbi(value: &str) -> String {
    let filtered: String = value
        .chars()
        .filter(|ch| !matches!(ch, '!' | '\'' | '(' | ')' | '*'))
        .collect();
    utf8_percent_encode(&filtered, WBI_ENCODE_SET).to_string()
}

/// Sign params: sorted `k=v` pairs + `wts`, MD5(query + mixin_key) as `w_rid`.
pub fn sign_query(params: &[(&str, String)], mixin_key: &str, wts: u64) -> String {
    let mut map: BTreeMap<String, String> = params
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect();
    map.insert("wts".into(), wts.to_string());
    let query = map
        .iter()
        .map(|(key, value)| format!("{}={}", encode_wbi(key), encode_wbi(value)))
        .collect::<Vec<_>>()
        .join("&");
    let mut hasher = Md5::new();
    hasher.update(query.as_bytes());
    hasher.update(mixin_key.as_bytes());
    let w_rid = format!("{:x}", hasher.finalize());
    format!("{query}&w_rid={w_rid}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_known_mixin_key() {
        let origin = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+/";
        let key = build_mixin_key(&origin[..32], &origin[32..]).unwrap();
        assert_eq!(key, "UVsc1ixGpYkF6dTJBRfXHjQtDCoNmMPn");
    }

    #[test]
    fn signs_known_query_vector() {
        let params = vec![
            ("foo", "114".to_string()),
            ("bar", "514".to_string()),
            ("zab", "1919810".to_string()),
        ];
        let signed = sign_query(&params, "0123456789abcdef0123456789abcdef", 1702204169);
        assert_eq!(
            signed,
            "bar=514&foo=114&wts=1702204169&zab=1919810&w_rid=4094ae7c68692e516c8a40b5dfec6c58"
        );
    }

    #[test]
    fn strips_reserved_characters_before_encoding() {
        assert_eq!(encode_wbi("a b!'()*中"), "a%20b%E4%B8%AD");
    }
}
