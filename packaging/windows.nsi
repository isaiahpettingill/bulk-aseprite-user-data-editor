Unicode True
!include "MUI2.nsh"
Name "Aseprite Metadata Editor"
OutFile "Aseprite-Metadata-Editor-windows-x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\Aseprite Metadata Editor"
RequestExecutionLevel user
Icon "..\assets\icon.ico"
!define MUI_ICON "..\assets\icon.ico"
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Section "Install"
  SetOutPath "$INSTDIR"
  File "..\target\release\aseprite-metadata-editor.exe"
  File "..\LICENSE"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\Aseprite Metadata Editor"
  CreateShortcut "$SMPROGRAMS\Aseprite Metadata Editor\Aseprite Metadata Editor.lnk" "$INSTDIR\aseprite-metadata-editor.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AsepriteMetadataEditor" "DisplayName" "Aseprite Metadata Editor"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AsepriteMetadataEditor" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AsepriteMetadataEditor" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AsepriteMetadataEditor" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AsepriteMetadataEditor" "NoRepair" 1
SectionEnd
Section "Uninstall"
  Delete "$INSTDIR\aseprite-metadata-editor.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Aseprite Metadata Editor\Aseprite Metadata Editor.lnk"
  RMDir "$SMPROGRAMS\Aseprite Metadata Editor"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AsepriteMetadataEditor"
SectionEnd
