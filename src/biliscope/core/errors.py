"""异常体系。

- :class:`BiliError`     本库所有异常的基类
- :class:`BiliApiError`  接口返回非 0 业务码，携带 code / message / payload
"""

from __future__ import annotations

from typing import Any


class BiliError(RuntimeError):
    """BiliScope 的异常基类。"""


class BiliApiError(BiliError):
    """B 站接口返回非 0 业务码。

    ``-352`` 通常是签名过期或频率过高，刷新密钥后重试一次往往就好了；
    ``-412`` 是风控，重试没用，只能降速或换环境。
    """

    SIGN_EXPIRED = -352
    RISK_CONTROL = -412

    def __init__(self, code: int, message: str, payload: dict[str, Any] | None = None):
        super().__init__(f"bilibili api error {code}: {message}")
        self.code = int(code)
        self.message = message or ""
        self.payload = payload or {}

    @property
    def is_sign_expired(self) -> bool:
        return self.code == self.SIGN_EXPIRED

    @property
    def is_risk_control(self) -> bool:
        return self.code == self.RISK_CONTROL
