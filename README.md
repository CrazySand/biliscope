# BiliScope

B 站视频下载器。Tauri 2 桌面壳 + Vue 3 界面 + Rust 后端，形态参考 `拾影`，协议实现参考 `bilibili-video-download`。

## 功能

- **扫码登录**：官方 passport 二维码接口，登录态独立会话，保存在本机。
- **链接解析**：粘贴 BV 号或完整视频链接，显示封面、标题、UP 主、时长、分 P。
- **双通道清晰度**：
  - `DASH`：登录后可解锁 1080P 及以上，音视频分离，需要 ffmpeg 合并。
  - `单文件`：匿名最高 720P MP4，自带音轨；应用统一要求 FFmpeg 才允许下载。
- **下载队列**：并发 1–4、进度/速度、取消、失败后重试、在资源管理器中定位。
- **设置**：保存目录、并发数、FFmpeg 环境检测。

## 环境

- Windows 10/11
- Node.js 22+ 与 npm
- Rust stable（MSVC 工具链）、Visual Studio Build Tools、Windows SDK
- FFmpeg 7+，并且 `ffmpeg.exe` 所在目录必须已加入系统 `PATH`
- Microsoft Edge WebView2 Runtime（安装程序使用 Tauri bootstrapper，缺失时会自动下载安装）

## 开发

```powershell
# 安装依赖。普通机器可直接 npm install；
# 本机 DSH 沙箱禁止 npm 生命周期脚本的子进程管道，所以用了 --ignore-scripts。
npm install --ignore-scripts

# 启动桌面应用（Vite + Tauri）
npm run tauri dev

# 仅构建前端
npm run build

# Rust 检查与测试
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

本机开发时 `src-tauri` 的构建产物放在 `%TEMP%\biliscope-target`，避免工作区 ACL 限制：

```powershell
$env:CARGO_TARGET_DIR = "$env:TEMP\biliscope-target"
npm run tauri dev
```

### Windows 安装包

双击根目录 `build.cmd`，或在终端执行 `build.cmd --no-pause`。脚本检查 Node.js、npm、Cargo 和本地 Tauri CLI，随后执行 `npm run build:exe`，依次构建前端、Rust release 程序与 NSIS 安装包。还需要已安装 MSVC C++ Build Tools 和 Windows SDK；缺少依赖时脚本会提示并退出，不会自动安装。

安装包在打包成功后发布到 `%USERPROFILE%\Downloads\BiliScope-release\`，请从这个目录运行或分发 `*-setup.exe`。项目根目录 `release/` 保留归档副本；沙箱工作区可能给文件附加 Low 完整性标签，直接运行该副本可能无法写入普通安装目录，因此发布文件直接从 Cargo 输出复制到用户下载目录，不修改工作区安全策略。Rust 中间构建文件仍保留在 `%TEMP%\biliscope-target`。默认完成或失败后暂停，方便查看结果；`--no-pause` 适合终端或自动化调用。`build.cmd --check --no-pause` 只检查基础命令与依赖是否存在，不执行打包，也不验证完整编译工具链。

FFmpeg 不打入安装包，目标机器仍需预先安装并加入 PATH。

### FFmpeg 检测

所有视频和仅音频下载均要求 FFmpeg；仅音频优先保留最高档可用音轨，不重新编码。查找顺序：

1. 环境变量 `BILISCOPE_FFMPEG`
2. 开发版的 `tools/ffmpeg/ffmpeg.exe`（不随安装包分发）
3. `PATH` 里的 `ffmpeg`

开发版默认直接从环境解析，不再保存或读取设置里的 `ffmpegPath`；旧 `settings.json` 里的同名字段会被忽略，下次保存设置时自动移除。

安装包会在预安装阶段执行 `where ffmpeg`，找不到时**直接中止安装**，文件为 `src-tauri/nsis/installer-hooks.nsh`。所以正式分发的机器必须先安装 FFmpeg 并把目录加入系统 PATH。

WebView2 不单独做中止检查：Tauri 的 `downloadBootstrapper` 会在缺失时自动下载并安装运行时，装完后再启动应用。

## 数据目录

```text
%LOCALAPPDATA%\site.crazysand.biliscope\
├─ settings.json     保存目录、并发数、登录 Cookie
├─ downloads.json    下载记录
└─ EBWebView\        WebView2 数据
```

登录 Cookie（`SESSDATA` 等）相当于账号会话，当前为本地明文存储；不要分享 `settings.json`，后续计划接 Windows DPAPI。

## 项目结构

```text
src/                     Vue 3 + TypeScript 前端
├─ App.vue               主界面
├─ components/           链接输入、视频卡、登录弹窗、下载列表、设置
├─ composables/          下载任务事件订阅
├─ api.ts / types.ts
└─ biliscope/            Python 协议实验台（不参与打包）

src-tauri/               Rust 后端
├─ src/client.rs         HTTP + Cookie 管理（rustls）
├─ src/auth.rs           二维码登录 / nav 校验
├─ src/video.rs          view / playurl / 清晰度模型
├─ src/download.rs       下载队列、DASH 合并、ffmpeg 探测
├─ src/settings.rs       设置持久化
├─ src/commands.rs       Tauri 命令
├─ nsis/installer-hooks.nsh 安装前 FFmpeg PATH 检查
└─ capabilities/         窗口权限（event / dialog / opener）

docs/history/            本地历史讨论归档（已 gitignore）
tools/ffmpeg/            开发用 FFmpeg 可执行文件（已 gitignore，不打包）
lab/                     临时协议探测脚本和输出（已 gitignore）
artifacts/
├─ previews/             Vue 组件隔离预览 HTML
├─ screenshots/          UI 验证截图（已 gitignore）
└─ archive/              旧验证快照（已 gitignore）
downloads/               本地下载文件（已 gitignore）
dist/                    前端构建输出（已 gitignore）
node_modules/            npm 依赖（已 gitignore）
.npm-cache/              项目内 npm 缓存（已 gitignore）
```

预览页面：启动 `npm run dev` 后访问 `/artifacts/previews/video-card-preview.html`、
`/artifacts/previews/download-layout-preview.html` 或 `/artifacts/previews/comment-library-preview.html`。
这些页面仅供开发验证，不是生产构建入口。

双击根目录 `dev.cmd` 可启动桌面开发环境；它会设置临时 Rust 构建目录，并在未显式指定时使用 `tools/ffmpeg/ffmpeg.exe`。
`src/biliscope/` 与根目录 `pyproject.toml` 保留为 Python 协议实验台，不参与 Tauri 打包。

## 已知限制 / TODO

- 只识别 BV 号或完整视频链接；`b23.tv` 短链、`av` 号待支持。
- 评论记录可从下载队列标题栏独立进入，切换视频或重启后可查看 SQLite 中保存的结果，并导出 CSV/JSON。仅分页采集主评论，保留每条主评论的子评论数量（rcount），不请求或保存子评论内容；CSV/JSON 仅导出主评论及其数量字段，历史子评论不删除但不再导出；界面分页展示全部已保存主评论，每页 20 条；按首次入库顺序展示，重复采集保留原位置，新评论追加在后；旧数据按首次采集时间恢复顺序，同毫秒记录以评论 ID 稳定排列。
- 评论采集范围为「前 N 页」或「全部可见」。每次从第 1 页开始，已有评论按 ID 去重保留；「重新采集并合并」不是断点续采，扩大页数表示扩大从第一页起的总范围。运行中的任务保存在内存，重启后不自动恢复。
- 没有断点续传；失败可点击重试，从头下载。
- 登录 Cookie 明文存储，后续接 DPAPI。
- 未配置代码签名、自动更新、单实例。

## 免责声明

仅供学习与个人备份使用。请遵守哔哩哔哩用户协议与当地法律，不要分发受版权保护的内容。

- 评论用户头像链接保存在原始记录的 `member.avatar` 中（兼容旧字段 `member.face`）；列表接口提供 `avatarUrl`，CSV 提供 `avatar_url`，JSON 保留原始头像字段。历史记录可直接读取已保存的链接，重新采集时更新头像链接；不下载头像文件。

- 评论记录支持按视频删除：视频选择框旁的删除按钮经确认后清理该视频的本地评论与任务记录；运行中的采集必须先停止。已导出的文件与 B 站原评论不受影响。
