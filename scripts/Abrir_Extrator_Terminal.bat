@echo off
setlocal EnableExtensions
chcp 65001 >nul
cd /d "%~dp0.."
title Extrator de Texto (modo terminal)

set "PYTHON_EXE=%CD%\venv\Scripts\python.exe"
if not exist "%PYTHON_EXE%" goto :missing

"%PYTHON_EXE%" "%CD%\src\extrator_core.py"
set "EXIT_CODE=%ERRORLEVEL%"
if not "%EXIT_CODE%"=="0" (
  echo.
  echo O programa terminou com codigo %EXIT_CODE%.
  pause
)
exit /b %EXIT_CODE%

:missing
echo.
echo Ambiente Python nao encontrado em "%CD%\venv".
echo Execute primeiro o arquivo scripts\Instalar.bat.
echo.
pause
exit /b 1
