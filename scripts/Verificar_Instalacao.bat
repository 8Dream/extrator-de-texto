@echo off
setlocal EnableExtensions
chcp 65001 >nul
cd /d "%~dp0.."
title Verificar Extrator de Texto

set "PYTHON_EXE=%CD%\venv\Scripts\python.exe"
if not exist "%PYTHON_EXE%" goto :missing

"%PYTHON_EXE%" "%CD%\src\extrator_core.py" --check
pause
exit /b %ERRORLEVEL%

:missing
echo Ambiente nao encontrado em "%CD%\venv". Execute scripts\Instalar.bat.
pause
exit /b 1
