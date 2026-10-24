"""SQLite 落库。

去重靠 ``replies.rpid`` 主键：同一页翻两次不会重复插入，游标回退也不会脏数据。
"""

from __future__ import annotations

import json
import sqlite3
import time
from pathlib import Path
from typing import Any

from .models import ReplyPage

SCHEMA = """
CREATE TABLE IF NOT EXISTS videos (
    bvid          TEXT PRIMARY KEY,
    aid           INTEGER,
    title         TEXT,
    first_seen_at REAL NOT NULL,
    last_seen_at  REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS replies (
    rpid       INTEGER PRIMARY KEY,
    aid        INTEGER NOT NULL,
    bvid       TEXT,
    page       INTEGER,
    mid        INTEGER,
    uname      TEXT,
    message    TEXT,
    like_count INTEGER,
    ctime      INTEGER,
    root       INTEGER,
    parent     INTEGER,
    raw        TEXT,
    fetched_at REAL NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_replies_aid ON replies(aid);
CREATE INDEX IF NOT EXISTS idx_replies_bvid ON replies(bvid);
"""


class CommentStore:
    """评论落库；``with CommentStore(path) as store: ...`` 也可用。"""

    def __init__(self, path: str | Path = "data/biliscope.db"):
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.conn = sqlite3.connect(self.path)
        self.conn.execute("PRAGMA journal_mode=WAL")
        self.conn.executescript(SCHEMA)
        self.conn.commit()

    # -- 生命周期 ---------------------------------------------------------
    def __enter__(self) -> "CommentStore":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    def close(self) -> None:
        self.conn.close()

    # -- 写入 -------------------------------------------------------------
    def touch_video(self, bvid: str, aid: int, title: str | None = None) -> None:
        now = time.time()
        self.conn.execute(
            """
            INSERT INTO videos (bvid, aid, title, first_seen_at, last_seen_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(bvid) DO UPDATE SET
                aid = excluded.aid,
                title = COALESCE(excluded.title, videos.title),
                last_seen_at = excluded.last_seen_at
            """,
            (bvid, aid, title, now, now),
        )
        self.conn.commit()

    def save_page(self, page: ReplyPage) -> int:
        """写入一页评论，返回本次新增（不含被去重覆盖）的行数。"""
        now = time.time()
        before = self.conn.total_changes
        rows = [self._row(reply, page, now) for reply in page.replies]
        self.conn.executemany(
            """
            INSERT INTO replies (
                rpid, aid, bvid, page, mid, uname, message,
                like_count, ctime, root, parent, raw, fetched_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            rows,
        )
        self.conn.commit()
        self._last_inserted = self.conn.total_changes - before
        return getattr(self, "_last_inserted", 0)

    @staticmethod
    def _row(reply: dict[str, Any], page: ReplyPage, now: float) -> tuple[Any, ...]:
        member = reply.get("member") or {}
        content = reply.get("content") or {}
        return (
            int(reply.get("rpid") or 0),
            int(page.aid),
            page.bvid,
            int(page.page),
            int(member.get("mid") or 0),
            str(member.get("uname") or ""),
            str(content.get("message") or ""),
            int(reply.get("like") or 0),
            int(reply.get("ctime") or 0),
            int(reply.get("root") or 0),
            int(reply.get("parent") or 0),
            json.dumps(reply, ensure_ascii=False),
            now,
        )

    # -- 读取 -------------------------------------------------------------
    def count_replies(self, aid: int | None = None) -> int:
        if aid is None:
            cur = self.conn.execute("SELECT COUNT(*) FROM replies")
        else:
            cur = self.conn.execute("SELECT COUNT(*) FROM replies WHERE aid = ?", (aid,))
        return int(cur.fetchone()[0])

    def count_videos(self) -> int:
        cur = self.conn.execute("SELECT COUNT(*) FROM videos")
        return int(cur.fetchone()[0])
