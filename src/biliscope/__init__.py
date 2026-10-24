"""BiliScope —— B 站协议观察工具箱。

当前里程碑 M1：wbi 签名 + 评论区翻页 + SQLite 落库 + 命令行。
"""

from .core.errors import BiliApiError, BiliError

__version__ = "0.1.0"
__all__ = ["BiliApiError", "BiliError", "__version__"]
