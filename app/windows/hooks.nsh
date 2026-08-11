; Explorer integration: a "Browse with treesight" verb on folders, on the
; background of an open folder, and on drives. %V is the clicked folder, which
; the app takes as its root argument.
;
; SHCTX resolves to HKLM or HKCU to match the installer's per-machine /
; per-user mode, so the verb is registered for whoever the install was for.
; Note that Windows 11 shows third-party verbs under "Show more options"
; unless the app ships an IExplorerCommand shell extension.

!macro TreesightRegisterVerb ROOT
  WriteRegStr SHCTX "Software\Classes\${ROOT}\shell\treesight" "" "Browse with treesight"
  WriteRegStr SHCTX "Software\Classes\${ROOT}\shell\treesight" "Icon" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "Software\Classes\${ROOT}\shell\treesight\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" "%V"'
!macroend

!macro TreesightUnregisterVerb ROOT
  DeleteRegKey SHCTX "Software\Classes\${ROOT}\shell\treesight"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro TreesightRegisterVerb "Directory"
  !insertmacro TreesightRegisterVerb "Directory\Background"
  !insertmacro TreesightRegisterVerb "Drive"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro TreesightUnregisterVerb "Directory"
  !insertmacro TreesightUnregisterVerb "Directory\Background"
  !insertmacro TreesightUnregisterVerb "Drive"
!macroend
