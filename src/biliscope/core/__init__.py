"""基础设施：HTTP 会话、wbi 签名、节流、异常。"""

from .errors import BiliApiError, BiliError

__all__ = ["BiliApiError", "BiliError"]
