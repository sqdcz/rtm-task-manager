@echo off
setlocal
set "DEST=%LOCALAPPDATA%\Programs\RTM"
set "K=HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\RTM"

if not exist "%DEST%" mkdir "%DEST%"
copy /Y "%~dp0rtm.exe" "%DEST%\rtm.exe" >nul
copy /Y "%~dp0uninstall.bat" "%DEST%\uninstall.bat" >nul

powershell -NoProfile -ExecutionPolicy Bypass -Command "$ws=New-Object -ComObject WScript.Shell; $d=[Environment]::GetFolderPath('Desktop'); $s=$ws.CreateShortcut($d+'\RTM Task Manager.lnk'); $s.TargetPath='%DEST%\rtm.exe'; $s.WorkingDirectory='%DEST%'; $s.Description='Rust Task Manager'; $s.Save(); $p=[Environment]::GetFolderPath('Programs'); $s2=$ws.CreateShortcut($p+'\RTM Task Manager.lnk'); $s2.TargetPath='%DEST%\rtm.exe'; $s2.WorkingDirectory='%DEST%'; $s2.Save()"

reg add "%K%" /v DisplayName /t REG_SZ /d "RTM Task Manager" /f >nul
reg add "%K%" /v DisplayVersion /t REG_SZ /d "1.8" /f >nul
reg add "%K%" /v Publisher /t REG_SZ /d "RTM" /f >nul
reg add "%K%" /v InstallLocation /t REG_SZ /d "%DEST%" /f >nul
reg add "%K%" /v DisplayIcon /t REG_SZ /d "%DEST%\rtm.exe" /f >nul
reg add "%K%" /v UninstallString /t REG_SZ /d "\"%DEST%\uninstall.bat\"" /f >nul
reg add "%K%" /v NoModify /t REG_DWORD /d 1 /f >nul
reg add "%K%" /v NoRepair /t REG_DWORD /d 1 /f >nul

REM Admin is only required for the Task Manager takeover (HKLM). Re-launch
REM elevated when needed; the staged files above are already in place.
if /i "%~1"=="/takeover" goto :takeover
net session >nul 2>&1
if errorlevel 1 (
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -ArgumentList '/takeover' -Verb RunAs"
  goto :launch
)

:takeover
echo.
set /p ANS=Replace the built-in Task Manager with RTM [Y/N]?
if /i "%ANS%"=="Y" (
  reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\Taskmgr.exe" /v Debugger /t REG_SZ /d "\"%DEST%\rtm.exe\"" /f >nul
  echo RTM is now the default task manager.
  echo Ctrl+Shift+Esc, the taskbar menu and Ctrl+Alt+Del all open RTM.
  echo Run uninstall.bat to restore the original.
) else (
  echo Kept the original Task Manager.
)

:launch
start "" "%DEST%\rtm.exe"
exit /b 0
