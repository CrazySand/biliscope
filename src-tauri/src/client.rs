use reqwest::header::{HeaderMap, HeaderValue, COOKIE, REFERER, SET_COOKIE};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
pub const REFERER_URL: &str = "https://www.bilibili.com/";

/// Minimal HTTP client for bilibili.
///
/// Cookies are tracked manually (instead of reqwest's cookie store) so that we
/// can persist the exact login cookie header and clear it on logout.
#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    cookies: Arc<Mutex<HashMap<String, String>>>,
}

impl ApiClient {
    pub fn new(cookie_header: &str) -> Result<Self, String> {
        let mut default_headers = HeaderMap::new();
        default_headers.insert(
            reqwest::header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
        );
        default_headers.insert(
            reqwest::header::ACCEPT_LANGUAGE,
            HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
        );
        let http = reqwest::Client::builder()
            .user_agent(UA)
            .default_headers(default_headers)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            // Redirects are followed manually so that every hop's Set-Cookie
            // (notably `buvid3` from the homepage) is captured.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("初始化网络客户端失败：{e}"))?;
        Ok(Self {
            http,
            cookies: Arc::new(Mutex::new(parse_cookie_header(cookie_header))),
        })
    }

    pub fn cookie_header(&self) -> String {
        let map = match self.cookies.lock() {
            Ok(map) => map,
            Err(_) => return String::new(),
        };
        map.iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn has_cookie(&self, name: &str) -> bool {
        self.cookies
            .lock()
            .map(|map| map.contains_key(name))
            .unwrap_or(false)
    }

    pub fn clear_cookies(&self) {
        if let Ok(mut map) = self.cookies.lock() {
            map.clear();
        }
    }

    fn capture(&self, headers: &HeaderMap) {
        let mut map = match self.cookies.lock() {
            Ok(map) => map,
            Err(_) => return,
        };
        for value in headers.get_all(SET_COOKIE) {
            if let Ok(text) = value.to_str() {
                if let Some((name, val)) = parse_set_cookie(text) {
                    map.insert(name, val);
                }
            }
        }
    }

    async fn send(
        &self,
        url: &str,
        params: &[(&str, String)],
        raw_query: Option<&str>,
    ) -> Result<(reqwest::StatusCode, String), String> {
        let target = match raw_query {
            Some(query) if !query.is_empty() => format!("{url}?{query}"),
            _ => url.to_string(),
        };
        let mut last_error = String::new();
        for attempt in 0..3 {
            let mut request = self.http.get(&target).header(REFERER, REFERER_URL);
            if raw_query.is_none() && !params.is_empty() {
                request = request.query(params);
            }
            let cookie = self.cookie_header();
            if !cookie.is_empty() {
                if let Ok(value) = HeaderValue::from_str(&cookie) {
                    request = request.header(COOKIE, value);
                }
            }
            match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    self.capture(response.headers());
                    let text = response
                        .text()
                        .await
                        .map_err(|e| format!("读取响应失败：{e}"))?;
                    return Ok((status, text));
                }
                Err(error) => {
                    last_error = error.to_string();
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(400 * (attempt as u64 + 1))).await;
                    }
                }
            }
        }
        Err(format!("连接 B 站失败：{last_error}"))
    }

    /// Raw JSON without checking the bilibili business code. Needed by `nav`
    /// (`-101` still carries useful data) and by status-style endpoints.
    pub async fn get_raw(&self, url: &str, params: &[(&str, String)]) -> Result<Value, String> {
        let (status, text) = self.send(url, params, None).await?;
        if status == reqwest::StatusCode::PRECONDITION_FAILED {
            return Err("请求被 B 站风控拦截（HTTP 412），请稍后重试或更换网络".into());
        }
        if !status.is_success() {
            return Err(format!("B 站接口返回 HTTP {status}"));
        }
        serde_json::from_str::<Value>(&text)
            .map_err(|_| "B 站返回了无法识别的数据".to_string())
    }

    /// GET a URL with a caller-built (already URL-encoded) query string. Used
    /// by WBI-signed endpoints, where re-encoding would invalidate `w_rid`.
    pub async fn get_raw_query(&self, url: &str, query: &str) -> Result<Value, String> {
        let (status, text) = self.send(url, &[], Some(query)).await?;
        if status == reqwest::StatusCode::PRECONDITION_FAILED {
            return Err("请求被 B 站风控拦截（HTTP 412），请稍后重试或更换网络".into());
        }
        if !status.is_success() {
            return Err(format!("B 站接口返回 HTTP {status}"));
        }
        serde_json::from_str::<Value>(&text)
            .map_err(|_| "B 站返回了无法识别的数据".to_string())
    }

    pub async fn get_json(&self, url: &str, params: &[(&str, String)]) -> Result<Value, String> {
        let value = self.get_raw(url, params).await?;
        let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if code != 0 {
            let message = value.get("message").and_then(Value::as_str).unwrap_or("");
            return Err(api_error(code, message));
        }
        Ok(value)
    }

    /// Visit the homepage once so bilibili hands out `buvid3`, which media
    /// endpoints (and the play page) expect.
    pub async fn bootstrap(&self) -> Result<(), String> {
        let mut url = "https://www.bilibili.com/".to_string();
        for _ in 0..4 {
            let response = self.http.get(&url).header(REFERER, REFERER_URL).send().await;
            let Ok(response) = response else {
                return Ok(());
            };
            self.capture(response.headers());
            let status = response.status();
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            if status.is_redirection() {
                if let Some(location) = location {
                    url = if location.starts_with("http") {
                        location
                    } else {
                        format!("https://www.bilibili.com{location}")
                    };
                    continue;
                }
            }
            break;
        }
        Ok(())
    }
}

pub fn api_error(code: i64, message: &str) -> String {
    match code {
        -101 => "账号未登录，请先扫码登录".into(),
        -352 => "请求签名校验失败，请重试".into(),
        -412 => "请求被 B 站风控拦截，请稍后重试".into(),
        -403 => "没有访问权限".into(),
        -404 => "视频不存在或已删除".into(),
        62002 => "该视频不可见".into(),
        62004 => "稿件审核中".into(),
        _ => {
            if message.is_empty() {
                format!("B 站接口错误 {code}")
            } else {
                format!("B 站接口错误 {code}：{message}")
            }
        }
    }
}

fn parse_cookie_header(header: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for part in header.split(';') {
        if let Some((name, value)) = part.trim().split_once('=') {
            let name = name.trim();
            let value = value.trim();
            if !name.is_empty() && !value.is_empty() {
                map.insert(name.to_string(), value.to_string());
            }
        }
    }
    map
}

fn parse_set_cookie(text: &str) -> Option<(String, String)> {
    let first = text.split(';').next()?.trim();
    let (name, value) = first.split_once('=')?;
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() || value.is_empty() {
        return None;
    }
    Some((name.to_string(), value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_round_trips_cookie_headers() {
        let map = parse_cookie_header("SESSDATA=abc; bili_jct=def; buvid3=xyz");
        assert_eq!(map.get("SESSDATA").unwrap(), "abc");
        assert_eq!(map.get("bili_jct").unwrap(), "def");
        let client = ApiClient::new("SESSDATA=abc; buvid3=xyz").unwrap();
        assert!(client.cookie_header().contains("SESSDATA=abc"));
        assert!(client.has_cookie("buvid3"));
        client.clear_cookies();
        assert!(client.cookie_header().is_empty());
    }

    #[test]
    fn captures_only_the_cookie_pair_from_set_cookie() {
        let (name, value) = parse_set_cookie("SESSDATA=abc%2C123; Path=/; Domain=.bilibili.com; HttpOnly").unwrap();
        assert_eq!(name, "SESSDATA");
        assert_eq!(value, "abc%2C123");
        assert!(parse_set_cookie("garbage").is_none());
    }
}
