@echo off
rem Starts Clawd's Eyes in the background, for the Clawd Desk Stations wallpaper.
rem It shows up as a little Clawd in the system tray. Right-click him there to quit.
rem conhost --headless runs PowerShell with no window at all, so there is nothing to close by mistake
rem (Windows Terminal ignores -WindowStyle Hidden and would show a window that stops Clawd when closed).
start "" "%WINDIR%\System32\conhost.exe" --headless "%WINDIR%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -STA -File "%~dp0clawd-eyes.ps1"
