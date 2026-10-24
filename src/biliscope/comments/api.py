"""评论区：接口调用与翻页生成器。"""

from __future__ import annotations

import json
from typing import Any, Iterator

import requests

from ..core.errors import BiliApiError
from ..core.http import build_session, get_json
from ..core.ratelimit import IntervalLimiter
from ..core.wbi import build_mixin_key, fetch_wbi_keys, sign_params
from .models import ReplyPage

VIEW_URL = "https://api.bilibili.com/x/web-interface/view"
REPLY_URL = "https://api.bilibili.com/x/v2/reply/wbi/main"

MODE_HOT = 3
MODE_NEW = 2
MODES = {"hot": MODE_HOT, "new": MODE_NEW}

DEFAULT_WEB_LOCATION = "1315875"
PAGE_INTERVAL = 1.0


def fetch_video_info(session: requests.Session, bvid: str) -> dict[str, Any]:
    """按 BV 号取视频信息（aid、cid、标题等）。"""
    return get_json(session, VIEW_URL, params={"bvid": bvid})["data"]


def fetch_aid(session: requests.Session, bvid: str) -> int:
    """只取 aid（评论区接口用 ``oid``，普通视频就等于 aid）。"""
    return int(fetch_video_info(session, bvid)["aid"])


def _reply_params(aid: int, offset: str, mode: int, web_location: str) -> dict[str, str]:
    return {
        "oid": str(aid),
        "type": "1",
        "mode": str(mode),
        "pagination_str": json.dumps({"offset": offset}, separators=(",", ":")),
        "plat": "1",
        "seek_rpid": "",
        "web_location": web_location,
        "x-bili-locale-json": json.dumps(
            {"c_locale": {"language": "zh", "script": "Hans"}, "always_translate": False},
            separators=(",", ":"),
        ),
    }


def iter_reply_pages(
    session: requests.Session,
    aid: int,
    *,
    mixin_key: str | None = None,
    bvid: str | None = None,
    mode: int = MODE_HOT,
    max_pages: int | None = None,
    interval: float = PAGE_INTERVAL,
    web_location: str = DEFAULT_WEB_LOCATION,
    limiter: IntervalLimiter | None = None,
    on_refresh: Any = None,
) -> Iterator[ReplyPage]:
    """逐页产出评论，一次 yield 一页。

    Args:
        session: 已建好的会话（见 :func:`biliscope.core.http.build_session`）
        aid: 视频 aid
        mixin_key: 已有的签名密钥；``None`` 表示第一次请求前自动拉取
        bvid: 仅用于回填到 :class:`ReplyPage`，便于落库
        mode: :data:`MODE_HOT`(热门) / :data:`MODE_NEW`(最新)
        max_pages: 最多翻几页；``None`` 表示翻到 ``is_end``
        interval: 相邻两页之间的最小间隔秒数
        web_location: 埋点位置，固定即可
        limiter: 自定义节流器，优先于 ``interval``
        on_refresh: 密钥刷新后的回调，签名 ``callback(mixin_key)``

    Yields:
        ReplyPage
    """
    key = mixin_key
    offset = ""
    page_no = 0
    limiter = limiter or IntervalLimiter(interval)

    while max_pages is None or page_no < max_pages:
        page_no += 1
        params = _reply_params(aid, offset, mode, web_location)
        body: dict[str, Any] = {}

        for attempt in (1, 2):
            if key is None:
                key = build_mixin_key(*fetch_wbi_keys(session))
                if on_refresh is not None:
                    on_refresh(key)

            signed = sign_params(params, key)
            resp = session.get(REPLY_URL, params=signed, timeout=20)
            resp.raise_for_status()
            body = resp.json()

            code = body.get("code")
            if code == 0:
                break
            # -352：签名过期或频率过高，丢掉旧密钥重试一次
            if code == BiliApiError.SIGN_EXPIRED and attempt == 1:
                key = None
                continue
            raise BiliApiError(int(code), str(body.get("message", "")), body)

        data = body.get("data") or {}
        cursor = data.get("cursor") or {}
        page = ReplyPage(
            page=page_no,
            replies=data.get("replies") or [],
            all_count=int(cursor.get("all_count") or 0),
            is_end=bool(cursor.get("is_end")),
            next_offset=(cursor.get("pagination_reply") or {}).get("next_offset") or "",
            aid=aid,
            bvid=bvid,
            cursor=cursor,
            raw=body,
        )
        yield page

        if page.is_end or not page.next_offset:
            return
        offset = page.next_offset
        limiter.wait()


def iter_replies(
    session: requests.Session,
    aid: int,
    *,
    max_items: int | None = None,
    **kwargs: Any,
) -> Iterator[dict[str, Any]]:
    """逐条产出评论（分页生成器外面包一层）。"""
    count = 0
    for page in iter_reply_pages(session, aid, **kwargs):
        for reply in page.replies:
            yield reply
            count += 1
            if max_items is not None and count >= max_items:
                return


def iter_pages_for(
    bvid: str,
    *,
    mode: int = MODE_HOT,
    max_pages: int | None = None,
    interval: float = PAGE_INTERVAL,
    session: requests.Session | None = None,
) -> Iterator[ReplyPage]:
    """按 BV 号直接翻页：内部建会话、查 aid、取密钥。

    传入 ``session`` 时复用调用方的会话（例如已带登录态），且不会关闭它。
    """
    owns_session = session is None
    session = session or build_session()
    try:
        aid = fetch_aid(session, bvid)
        yield from iter_reply_pages(
            session,
            aid,
            bvid=bvid,
            mode=mode,
            max_pages=max_pages,
            interval=interval,
        )
    finally:
        if owns_session:
            session.close()


def iter_replies_for(
    bvid: str,
    *,
    mode: int = MODE_HOT,
    max_pages: int | None = None,
    max_items: int | None = None,
    interval: float = PAGE_INTERVAL,
    session: requests.Session | None = None,
) -> Iterator[dict[str, Any]]:
    """按 BV 号直接逐条产出评论。"""
    count = 0
    for page in iter_pages_for(
        bvid, mode=mode, max_pages=max_pages, interval=interval, session=session
    ):
        for reply in page.replies:
            yield reply
            count += 1
            if max_items is not None and count >= max_items:
                return
