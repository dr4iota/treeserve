; Explorer integration: a "Browse with treeserve" verb on folders, on the
; background of an open folder, and on drives. %V is the clicked folder, which
; the app takes as its root argument.
;
; SHCTX resolves to HKLM or HKCU to match the installer's per-machine /
; per-user mode, so the verb is registered for whoever the install was for.
; Note that Windows 11 shows third-party verbs under "Show more options"
; unless the app ships an IExplorerCommand shell extension.

!macro TreeserveRegisterVerb ROOT
  WriteRegStr SHCTX "Software\Classes\${ROOT}\shell\treeserve" "" "Browse with treeserve"
  WriteRegStr SHCTX "Software\Classes\${ROOT}\shell\treeserve" "Icon" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "Software\Classes\${ROOT}\shell\treeserve\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" "%V"'
!macroend

!macro TreeserveUnregisterVerb ROOT
  DeleteRegKey SHCTX "Software\Classes\${ROOT}\shell\treeserve"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro TreeserveRegisterVerb "Directory"
  !insertmacro TreeserveRegisterVerb "Directory\Background"
  !insertmacro TreeserveRegisterVerb "Drive"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro TreeserveUnregisterVerb "Directory"
  !insertmacro TreeserveUnregisterVerb "Directory\Background"
  !insertmacro TreeserveUnregisterVerb "Drive"
!macroend
