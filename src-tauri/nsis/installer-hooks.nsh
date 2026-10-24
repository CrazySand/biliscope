; BiliScope: FFmpeg is a hard install prerequisite.
; WebView2 is handled by Tauri's downloadBootstrapper.
!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToStack '"$SYSDIR\where.exe" ffmpeg'
  Pop $0
  Pop $1
  StrCmp $0 "0" biliscope_ffmpeg_ok
  MessageBox MB_ICONSTOP|MB_OK "BiliScope 需要 FFmpeg。$\r$\n$\r$\n请先安装 FFmpeg，并把 ffmpeg.exe 所在目录加入系统 PATH，然后重新运行安装程序。$\r$\n$\r$\nWebView2 运行时会由安装程序自动检查并安装。"
  Abort
  biliscope_ffmpeg_ok:
!macroend
