"""评论区数据模型。"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(slots=True)
class ReplyPage:
    """一页评论。"""

    page: int
    replies: list[dict[str, Any]]
    all_count: int
    is_end: bool
    next_offset: str
    aid: int = 0
    bvid: str | None = None
    cursor: dict[str, Any] = field(default_factory=dict)
    raw: dict[str, Any] = field(default_factory=dict)

    @property
    def count(self) -> int:
        return len(self.replies)


def looks_like_soft_cap(received: int, all_count: int, *, ratio: float = 0.95) -> bool:
    """判断"翻完了"是不是匿名可见范围的软上限。

    软上限的特征：``is_end=True``、``code=0``、HTTP 200，但累计条数远小于
    ``all_count``。真翻到底时两者应当基本相等。
    """
    if all_count <= 0:
        return False
    return received < all_count * ratio
