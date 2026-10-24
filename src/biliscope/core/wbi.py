"""wbi 签名。

算法四步：

1. ``GET /x/web-interface/nav`` 取 ``wbi_img.img_url`` / ``sub_url``
   （**未登录也会下发**，此时业务码是 -101，但 ``data.wbi_img`` 是完整的）；
2. 从 URL 里抠出两个 32 位密钥，拼成 64 字符，按置换表 ``MIXIN_TAB`` 重排后取前 32 位；
3. query 参数按 key 升序，k/v 分别做 URL 编码并删除 ``!'()*`` 五个字符；
4. ``w_rid = md5(规范化 query + mixin_key)``，同时带上秒级 ``wts``。

密钥每天轮换，``w_rid`` 只覆盖参数本身、不绑定账号。
"""

from __future__ import annotations

import hashlib
import re
import time
from typing import Any, Mapping
from urllib.parse import quote

import requests

from .errors import BiliApiError

NAV_URL = "https://api.bilibili.com/x/web-interface/nav"

#: 64 → 64 的固定置换表，取前 32 位作为 mixin_key
MIXIN_TAB = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35,
    27, 43, 5, 49, 33, 9, 42, 19, 29, 28, 14, 39, 12, 38, 41, 13,
    37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4,
    22, 25, 54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
]

_STRIP = re.compile(r"[!'()*]")


def key_from_url(url: str) -> str:
    """``.../bfs/wbi/7cd0....png`` → ``7cd0...``"""
    return url.rsplit("/", 1)[-1].split(".")[0]


def build_mixin_key(img_key: str, sub_key: str) -> str:
    """把两个密钥按置换表重排成 32 位 mixin_key。"""
    origin = img_key + sub_key
    if len(origin) < 64:
        raise ValueError(f"wbi keys too short: {len(origin)} chars, expected 64")
    return "".join(origin[i] for i in MIXIN_TAB)[:32]


def _encode(value: Any) -> str:
    """URL 编码，并按 B 站规则删除 ``!'()*`` 五个字符。"""
    return quote(_STRIP.sub("", str(value)), safe="")


def sign_params(
    params: Mapping[str, Any],
    mixin_key: str,
    *,
    wts: int | str | None = None,
) -> dict[str, str]:
    """给参数补上 ``wts`` 并计算 ``w_rid``。

    Args:
        params: 原始 query 参数；若已含 ``w_rid`` 会被忽略
        mixin_key: :func:`build_mixin_key` 的结果
        wts: 指定时间戳；``None`` 表示取当前秒（测试时可传固定值复算）
    """
    signed = {k: str(v) for k, v in params.items() if k != "w_rid"}
    signed["wts"] = str(int(time.time()) if wts is None else wts)
    query = "&".join(f"{_encode(k)}={_encode(signed[k])}" for k in sorted(signed))
    signed["w_rid"] = hashlib.md5((query + mixin_key).encode("utf-8")).hexdigest()
    return signed


def fetch_wbi_keys(session: requests.Session, *, timeout: float = 15.0) -> tuple[str, str]:
    """拉取当天的 ``(img_key, sub_key)``。

    注意不能用通用的 ``get_json``：匿名访问时该接口返回 ``code=-101``
    （账号未登录），但密钥照样在 ``data.wbi_img`` 里，按错误处理会误伤。
    """
    resp = session.get(NAV_URL, timeout=timeout)
    resp.raise_for_status()
    body = resp.json()
    wbi_img = (body.get("data") or {}).get("wbi_img") or {}
    img_url, sub_url = wbi_img.get("img_url"), wbi_img.get("sub_url")
    if not img_url or not sub_url:
        raise BiliApiError(
            int(body.get("code") or -1),
            str(body.get("message") or "no wbi_img in nav response"),
            body,
        )
    return key_from_url(img_url), key_from_url(sub_url)
