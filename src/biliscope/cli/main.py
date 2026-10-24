"""命令行入口（``bs``）。"""

from __future__ import annotations

import argparse
import json
import sys
from typing import Sequence

import requests

from .. import __version__
from ..comments.api import MODES, PAGE_INTERVAL, iter_pages_for
from ..comments.models import looks_like_soft_cap
from ..comments.store import CommentStore
from ..core.errors import BiliApiError

TLS_HINT = """[TLS 握手失败] 当前运行环境阻断了 HTTPS。
  1) 若在 DSH / 沙箱终端里运行：换一个普通 PowerShell 窗口再试
  2) 若普通终端仍失败：检查系统代理、VPN、安全软件
  3) 自检：curl.exe -s -o NUL -w "%{http_code}" https://www.baidu.com"""


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="bs", description="BiliScope 命令行")
    parser.add_argument("--version", action="version", version=f"biliscope {__version__}")
    sub = parser.add_subparsers(dest="command", required=True)

    comments = sub.add_parser("comments", help="采集某视频的评论区")
    comments.add_argument("bvid", help="BV 号，例如 BV1JsJ86TEQW")
    comments.add_argument("--pages", type=int, default=3, metavar="N",
                          help="最多翻几页（默认 3；0 表示翻到 is_end）")
    comments.add_argument("--mode", choices=sorted(MODES), default="hot",
                          help="排序：hot 热门 / new 最新（默认 hot）")
    comments.add_argument("--interval", type=float, default=PAGE_INTERVAL,
                          help="相邻两页的最小间隔秒数（默认 1.0）")
    comments.add_argument("--db", default="data/biliscope.db",
                          help="SQLite 路径（默认 data/biliscope.db）")
    comments.add_argument("--no-store", action="store_true", help="只打印，不落库")
    comments.add_argument("--json", action="store_true", help="逐条输出原始 JSON")
    comments.set_defaults(func=cmd_comments)
    return parser


def cmd_comments(args: argparse.Namespace) -> int:
    mode = MODES[args.mode]
    max_pages = None if args.pages == 0 else args.pages
    store = None if args.no_store else CommentStore(args.db)

    received = 0
    pages = 0
    ended = False
    all_count = 0

    try:
        for page in iter_pages_for(
            args.bvid, mode=mode, max_pages=max_pages, interval=args.interval
        ):
            pages = page.page
            all_count = page.all_count
            received += page.count

            if store is not None and page.replies:
                store.save_page(page)

            if args.json:
                for reply in page.replies:
                    print(json.dumps(reply, ensure_ascii=False))
            else:
                print(f"[第 {page.page} 页] 共 {page.all_count} 条, "
                      f"is_end={page.is_end}, next={page.next_offset!r}")
                for reply in page.replies:
                    member = reply.get("member") or {}
                    content = reply.get("content") or {}
                    name = member.get("uname") or "?"
                    text = (content.get("message") or "").replace("\n", " ")[:60]
                    print(f"   {name}: {text}")

            if page.is_end:
                ended = True
    finally:
        if store is not None:
            store.close()

    stream = sys.stderr if args.json else sys.stdout
    print(f"[完成] {pages} 页 / {received} 条", file=stream)
    if ended and looks_like_soft_cap(received, all_count):
        print(f"[提示] all_count={all_count}，但只拿到 {received} 条："
              f"这是匿名的软上限，未登录最多翻约 18~19 页。", file=stream)
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return int(args.func(args) or 0)
    except BiliApiError as exc:
        print(f"[接口错误] code={exc.code} {exc.message}", file=sys.stderr)
        if exc.is_risk_control:
            print("           风控，重试无用；降速或换环境。", file=sys.stderr)
        return 2
    except requests.exceptions.SSLError:
        print(TLS_HINT, file=sys.stderr)
        return 3
    except KeyboardInterrupt:
        print("[中断]", file=sys.stderr)
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
