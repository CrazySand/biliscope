# 项目协作约定

- 产品代码：`src/`（Vue 3 + TypeScript）与 `src-tauri/`（Rust）。
- `src/biliscope/` 是 Python 协议实验台，只用于验证协议与对拍，不参与打包；不要删除（当前沙箱禁止删除已有文件）。
- 前端构建用 `npm run build`；`vite.config.ts` 关闭了 minify，避免 esbuild 大于 1MB 的临时文件在沙箱里删除失败。
- Rust 检查/测试建议把构建目录放到临时目录，避免工作区 ACL：
  `$env:CARGO_TARGET_DIR = "$env:TEMP\biliscope-target"`
- Tauri 2 新增命令或插件时，记得同步 `src-tauri/capabilities/default.json` 的权限（event / dialog / opener）。
- 除非用户明确要求，不要执行 `tauri build` 生成安装包。
- 不要把真实 Cookie、SESSDATA 写进代码、日志或截图。
