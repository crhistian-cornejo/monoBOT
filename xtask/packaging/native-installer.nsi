Unicode True
ManifestDPIAware true
!include "MUI2.nsh"
!include "FileFunc.nsh"
Name "monoBOT"
OutFile "@OUTFILE@"
InstallDir "$LOCALAPPDATA\Programs\monoBOT"
InstallDirRegKey HKCU "Software\monoBOT" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "monoBOT @VERSION@"
VIProductVersion "@WINDOWS_VERSION@"
VIAddVersionKey /LANG=0 "ProductName" "monoBOT"
VIAddVersionKey /LANG=0 "ProductVersion" "@VERSION@"
VIAddVersionKey /LANG=0 "FileDescription" "monoBOT installer"
VIAddVersionKey /LANG=0 "FileVersion" "@VERSION@"
VIAddVersionKey /LANG=0 "CompanyName" "@PUBLISHER@"
VIAddVersionKey /LANG=0 "LegalCopyright" "@PUBLISHER@"

; Pictures drawn from assets/logo/monobot.svg by `cargo xtask logo`.
!define MUI_ICON "@ASSETS@\icons\icon.ico"
!define MUI_UNICON "@ASSETS@\icons\icon.ico"
!define MUI_WELCOMEFINISHPAGE_BITMAP "@ASSETS@\installer\nsis-sidebar.bmp"
!define MUI_UNWELCOMEFINISHPAGE_BITMAP "@ASSETS@\installer\nsis-sidebar.bmp"
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_RIGHT
!define MUI_HEADERIMAGE_BITMAP "@ASSETS@\installer\nsis-header.bmp"
!define MUI_HEADERIMAGE_UNBITMAP "@ASSETS@\installer\nsis-header.bmp"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchApp
!define MUI_FINISHPAGE_RUN_TEXT "$(RunApp)"
!define MUI_FINISHPAGE_NOREBOOTSUPPORT

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
; The first language is the fallback; Windows picks Spanish for Spanish users.
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Spanish"

LangString RunApp ${LANG_ENGLISH} "Open monoBOT"
LangString RunApp ${LANG_SPANISH} "Abrir monoBOT"
LangString StillOpen ${LANG_ENGLISH} "monoBOT did not close. Please try updating again."
LangString StillOpen ${LANG_SPANISH} "monoBOT no se cerró. Vuelve a intentar la actualización."
LangString Failed ${LANG_ENGLISH} "monoBOT could not be updated. The previous version was kept. Close other monoBOT windows and try again."
LangString Failed ${LANG_SPANISH} "No se pudo actualizar monoBOT. Se conservó la versión anterior. Cierra las demás ventanas de monoBOT y vuelve a intentarlo."

Var UpdatePID
Var NoRelaunch
Var HadPrevious
Function .onInit
  ReadRegStr $R1 HKCU "Software\MonoBOT Native" "InstallDir"
  StrCmp $R1 "" previous_install_done
  StrCmp $INSTDIR "$LOCALAPPDATA\Programs\monoBOT" 0 previous_install_done
  StrCpy $INSTDIR $R1
  previous_install_done:
  ${GetParameters} $R0
  ; /NORELAUNCH: an update installed while the app quits leaves it closed.
  StrCpy $NoRelaunch 0
  ClearErrors
  ${GetOptions} $R0 "/NORELAUNCH" $R2
  IfErrors +2
  StrCpy $NoRelaunch 1
  ${GetOptions} $R0 "/UPDATEPID=" $UpdatePID
  StrCmp $UpdatePID "" ready
  System::Call 'kernel32::OpenProcess(i 0x00100000, i 0, i $UpdatePID) p .r1'
  StrCmp $1 0 ready
  System::Call 'kernel32::WaitForSingleObject(p r1, i 120000) i .r2'
  System::Call 'kernel32::CloseHandle(p r1)'
  StrCmp $2 0 ready
  Abort "$(StillOpen)"
  ready:
FunctionEnd
Section "monoBOT"
  SetOutPath "$INSTDIR"
  ClearErrors
  File /oname=monocode-gpui.next.exe "@STAGE@\monocode-gpui.exe"
  IfErrors install_failed
  StrCpy $HadPrevious 0
  IfFileExists "$INSTDIR\monocode-gpui.exe" 0 replace_binary
  Delete "$INSTDIR\monocode-gpui.previous.exe"
  ClearErrors
  Rename "$INSTDIR\monocode-gpui.exe" "$INSTDIR\monocode-gpui.previous.exe"
  IfErrors install_failed
  StrCpy $HadPrevious 1
  replace_binary:
  ClearErrors
  Rename "$INSTDIR\monocode-gpui.next.exe" "$INSTDIR\monocode-gpui.exe"
  IfErrors rollback_binary
  File "@STAGE@\native-install.json"
  File /r "@STAGE@\licenses"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\monoBOT" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "DisplayName" "monoBOT"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "DisplayVersion" "@VERSION@"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "DisplayIcon" '"$INSTDIR\monocode-gpui.exe",0'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "Publisher" "@PUBLISHER@"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "EstimatedSize" "$0"
  CreateShortcut "$SMPROGRAMS\monoBOT.lnk" "$INSTDIR\monocode-gpui.exe"
  ; A silent update keeps the desktop shortcut the person deleted deleted.
  IfSilent shortcuts_done
  CreateShortcut "$DESKTOP\monoBOT.lnk" "$INSTDIR\monocode-gpui.exe"
  shortcuts_done:
  Delete "$SMPROGRAMS\MonoBOT Native.lnk"
  Delete "$DESKTOP\MonoBOT Native.lnk"
  DeleteRegKey HKCU "Software\MonoBOT Native"
  Delete "$INSTDIR\monocode-gpui.previous.exe"
  Goto install_done
  rollback_binary:
  StrCmp $HadPrevious 1 0 install_failed
  Rename "$INSTDIR\monocode-gpui.previous.exe" "$INSTDIR\monocode-gpui.exe"
  install_failed:
  Delete "$INSTDIR\monocode-gpui.next.exe"
  MessageBox MB_ICONSTOP "$(Failed)" /SD IDOK
  Abort
  install_done:
SectionEnd
; Open the app through Explorer so it is not a child of this installer.
; Endpoint protection (Sophos Lockdown, for one) treats a freshly downloaded,
; unsigned installer as untrusted and passes that to everything it starts:
; the app would then be killed the first time it opens a PowerShell terminal
; or an agent CLI that uses PowerShell. Started by Explorer, it is a normal
; app, like one opened from the Start menu.
Function LaunchApp
  Exec '"$WINDIR\explorer.exe" "$INSTDIR\monocode-gpui.exe"'
FunctionEnd
Function .onInstFailed
  StrCmp $UpdatePID "" failed_done
  StrCmp $NoRelaunch 1 failed_done
  IfFileExists "$INSTDIR\monocode-gpui.exe" 0 failed_done
  Call LaunchApp
  failed_done:
FunctionEnd
Function .onInstSuccess
  StrCmp $UpdatePID "" done
  StrCmp $NoRelaunch 1 done
  Call LaunchApp
  done:
FunctionEnd
Section "Uninstall"
  Delete "$SMPROGRAMS\monoBOT.lnk"
  Delete "$DESKTOP\monoBOT.lnk"
  Delete "$INSTDIR\monocode-gpui.exe"
  Delete "$INSTDIR\monocode-gpui.previous.exe"
  Delete "$INSTDIR\native-install.json"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$INSTDIR\licenses"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\monoBOT"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative"
SectionEnd
