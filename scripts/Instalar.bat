@echo off
setlocal
chcp 65001 >nul
cd /d "%~dp0.."
title Instalacao do Extrator de Texto
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%CD%\scripts\install.ps1"
set "EXIT_CODE=%ERRORLEVEL%"
echo.
if "%EXIT_CODE%"=="0" (
  echo Instalacao concluida.
) else (
  echo A instalacao terminou com codigo %EXIT_CODE%.
)
pause
exit /b %EXIT_CODE%
