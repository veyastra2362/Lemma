; Lemma 安装/卸载钩子
;
; 为什么需要：应用运行时会往系统里写「原生宿主登记」——两处注册表 + 两个清单文件。
; NSIS 默认只删自己安装目录里的东西，这些登记会留下来，指向一个已经不存在的 exe。
; 卸载时必须一并清掉，否则用户重装/换版本后会看到"扩展连不上宿主"的怪现象。
;
; 用户数据（设置、词库、模型，约 270 MB）单独问：默认保留，避免误删。

!macro NSIS_HOOK_PREUNINSTALL
  ; ---- 原生宿主登记 ----
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\dev.lunar.lemma"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\dev.lunar.lemma"
  Delete "$APPDATA\dev.lunar.lemma\native-host\dev.lunar.lemma.json"
  RMDir "$APPDATA\dev.lunar.lemma\native-host"
  Delete "$APPDATA\Mozilla\NativeMessagingHosts\dev.lunar.lemma.json"

  ; ---- 用户数据（可选）----
  IfFileExists "$APPDATA\dev.lunar.lemma\*.*" 0 done_data
    MessageBox MB_YESNO|MB_DEFBUTTON2 \
      "是否一并删除 Lemma 的用户数据？$\r$\n$\r$\n包括：设置、翻译缓存、词库与 OCR/朗读模型（约 270 MB）。$\r$\n位置：$APPDATA\dev.lunar.lemma" \
      /SD IDNO IDNO done_data
    RMDir /r "$APPDATA\dev.lunar.lemma"
  done_data:
!macroend
