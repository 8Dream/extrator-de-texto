@echo off
setlocal EnableExtensions
chcp 65001 >nul
cd /d "%~dp0..\.."
title Testes automatizados do Extrator de Texto

set "PYTHON_EXE=%CD%\venv\Scripts\python.exe"
if not exist "%PYTHON_EXE%" goto :missing

"%PYTHON_EXE%" -m pip install -q -r "%CD%\requirements-dev.txt"
"%PYTHON_EXE%" -m pytest "%CD%\tests" -v
pause
exit /b %ERRORLEVEL%

:missing
echo Ambiente nao encontrado em "%CD%\venv". Execute scripts\Instalar.bat.
pause
exit /b 1
