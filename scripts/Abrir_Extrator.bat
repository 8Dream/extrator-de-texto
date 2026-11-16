@echo off
setlocal EnableExtensions
cd /d "%~dp0.."

set "PYTHONW_EXE=%CD%\venv\Scripts\pythonw.exe"
if not exist "%PYTHONW_EXE%" goto :missing

start "" "%PYTHONW_EXE%" "%CD%\src\extrator.py"
exit /b 0

:missing
echo.
echo Ambiente Python nao encontrado em "%CD%\venv".
echo Execute primeiro o arquivo scripts\Instalar.bat.
echo.
pause
exit /b 1
