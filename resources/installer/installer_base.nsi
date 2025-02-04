;-------------------------------------------------------------------------------
; Includes
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WinVer.nsh"
!include "x64.nsh"

;-------------------------------------------------------------------------------
; Constants
!define PRODUCT_NAME "VVVST"
!define PRODUCT_DESCRIPTION "VoicevoxのVSTプラグイン"
!define COPYRIGHT "Copyright (c) 2024 Nanashi."
# !define COPYRIGHT "Copyright (c) 2024 Hiroshiba Kazuyuki"
!define PRODUCT_VERSION "{version}.0"
!define SETUP_VERSION "{version}.0"

;-------------------------------------------------------------------------------
; Attributes
Name "VVVST"
OutFile "build/VVVST-{version}-windows-setup.exe"
InstallDir "$PROGRAMFILES64\Common Files\VST3\VVVST.vst3"
RequestExecutionLevel admin ; user|highest|admin

;-------------------------------------------------------------------------------
; Version Info
VIProductVersion "${PRODUCT_VERSION}"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "ProductVersion" "${PRODUCT_VERSION}"
VIAddVersionKey "FileDescription" "${PRODUCT_DESCRIPTION}"
VIAddVersionKey "LegalCopyright" "${COPYRIGHT}"
VIAddVersionKey "FileVersion" "${SETUP_VERSION}"

;-------------------------------------------------------------------------------
; Modern UI Appearance
!define MUI_ICON "resources\installer\VVVST.ico"

;-------------------------------------------------------------------------------
; Installer Pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

;-------------------------------------------------------------------------------
; Uninstaller Pages
!insertmacro MUI_UNPAGE_WELCOME
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_DIRECTORY
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

;-------------------------------------------------------------------------------
; Languages
!insertmacro MUI_LANGUAGE "Japanese"

;-------------------------------------------------------------------------------
; Installer Sections
Section "VVVST" Vvvst
	SetOutPath "$INSTDIR"
  File "resources\installer\VVVST.ico"
  File "resources\installer\desktop.ini"
  File /r "build\release\bin\vvvst.vst3\"
  System::Call "shlwapi::PathMakeSystemFolder(t '$INSTDIR') i."

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\VVVST" \
                   "DisplayName" "VVVST"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\VVVST" \
                   "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
SectionEnd

; https://gist.github.com/jhandley/1ec569242170454c593a3b1642cc995e
Section "WebView2"
  ReadRegStr $0 HKLM \
    "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"

  ${If} ${Errors}
  ${OrIf} $0 == ""

  SetDetailsPrint both
  DetailPrint "Installing: WebView2 Runtime"
  SetDetailsPrint listonly

  InitPluginsDir
  CreateDirectory "$pluginsdir\webview2bootstrapper"
  SetOutPath "$pluginsdir\webview2bootstrapper"
  File "Resources\installer\external\MicrosoftEdgeWebview2Setup.exe"
  ExecWait '"$pluginsdir\webview2bootstrapper\MicrosoftEdgeWebview2Setup.exe" /silent /install'

  SetDetailsPrint both

  ${EndIf}
SectionEnd

Section "Visual Studio Runtime"
  SetOutPath "$INSTDIR"
  File "resources\installer\external\vcredist_x64.exe"
  ExecWait "$INSTDIR\vcredist_x64.exe /install /quiet /norestart"
  Delete "$INSTDIR\vcredist_x64.exe"
SectionEnd

;-------------------------------------------------------------------------------
; Uninstaller Sections
Section "Uninstall"
	RMDir /r "$INSTDIR"
  Delete "$INSTDIR\Uninstall.exe"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\VVVST"
SectionEnd
