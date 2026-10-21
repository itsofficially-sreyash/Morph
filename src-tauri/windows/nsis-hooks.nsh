; Morph's additions to Tauri's NSIS installer (bundle.windows.nsis.installerHooks).

; Uninstalling (not updating): remove the Explorer right-click entry and start-at-sign-in,
; and quit a running Morph. `--remove-integration` hands off to a running Morph, which
; cleans up and exits; the short wait lets it exit before the files are deleted.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --remove-integration'
    Sleep 1000
  ${EndIf}
!macroend
