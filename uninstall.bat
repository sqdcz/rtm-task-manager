@echo off
REM Elevate first: removing the takeover writes to HKLM.
net session >nul 2>&1
if errorlevel 1 (
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -ArgumentList '/elevated' -Verb RunAs"
  exit /b
)

taskkill /F /IM rtm.exe >nul 2>&1
reg delete "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\Taskmgr.exe" /v Debugger /f >nul 2>&1
del "%USERPROFILE%\Desktop\RTM Task Manager.lnk" >nul 2>&1
del "%APPDATA%\Microsoft\Windows\Start Menu\Programs\RTM Task Manager.lnk" >nul 2>&1
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\RTM" /f >nul 2>&1
start "" cmd /c "timeout /t 1 >nul & rmdir /S /Q \"%LOCALAPPDATA%\Programs\RTM\""
exit /b 0
