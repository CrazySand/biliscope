"""请求节流：保证相邻两次请求之间至少间隔 ``interval`` 秒。"""

from __future__ import annotations

import time


class IntervalLimiter:
    """简单的固定间隔节流器。

    只做"别打太快"，不做并发令牌桶——M1 阶段单线程顺序翻页已经够用。
    """

    def __init__(self, interval: float = 1.0):
        self.interval = max(0.0, float(interval))
        self._last = 0.0

    def wait(self) -> float:
        """阻塞到可以发下一次请求，返回实际睡眠秒数。"""
        if self.interval <= 0:
            return 0.0
        remaining = self.interval - (time.monotonic() - self._last)
        slept = 0.0
        if remaining > 0:
            time.sleep(remaining)
            slept = remaining
        self._last = time.monotonic()
        return slept
