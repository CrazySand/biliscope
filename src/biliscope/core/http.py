"""HTTP 会话与统一请求入口。"""

from __future__ import annotations

from typing import Any, Mapping

import requests

from .errors import BiliApiError

UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36"
)
DEFAULT_HEADERS = {"User-Agent": UA, "Referer": "https://www.bilibili.com/"}
DEFAULT_TIMEOUT = 15.0


def build_session(cookies: Mapping[str, str] | None = None) -> requests.Session:
    """新建带默认请求头的会话。

    Args:
        cookies: 需要预置的 cookie（M2 的登录态会用到，例如 ``SESSDATA``）
    """
    session = requests.Session()
    session.headers.update(DEFAULT_HEADERS)
    for name, value in (cookies or {}).items():
        session.cookies.set(name, value, domain=".bilibili.com")
    return session


def get_json(
    session: requests.Session,
    url: str,
    *,
    params: Mapping[str, Any] | None = None,
    timeout: float = DEFAULT_TIMEOUT,
) -> dict[str, Any]:
    """GET 并解析 JSON；业务码非 0 时抛 :class:`BiliApiError`。"""
    resp = session.get(url, params=dict(params or {}), timeout=timeout)
    resp.raise_for_status()
    body = resp.json()
    if not isinstance(body, dict):
        raise BiliApiError(-1, "unexpected response body", {"raw": body})
    code = body.get("code")
    if code != 0:
        raise BiliApiError(code if code is not None else -1, str(body.get("message", "")), body)
    return body
