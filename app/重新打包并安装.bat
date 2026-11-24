@echo off
cd /d "%~dp0"
set "PATH=C:\Program Files\nodejs;%APPDATA%\npm;%USERPROFILE%\.cargo\bin;%PATH%"
where npm
where cargo
chcp 65001 >nul
echo.
echo 正在打包 TD 工程启动器，大约需要几分钟...
call npm run build
if errorlevel 1 (
  echo.
  echo 打包失败。请把上面的报错内容复制给 Claude。
  pause
  exit /b 1
)
for %%f in ("src-tauri\target\release\bundle\nsis\*_0.2.0_x64-setup.exe") do start "" "%%f"
echo 打包完成，安装程序已打开。
pause
