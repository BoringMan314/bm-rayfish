@echo off
chcp 65001 >nul 2>&1
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

rem Portable Windows zip. Does not call scripts\build-windows-msi.ps1.
rem
rem Zip: dist\bm-rayfish-<version>.zip
rem   bm-rayfish.exe
rem   wintun.dll
rem
rem Needs: Rust (cargo), rustup target x86_64-pc-windows-msvc, MSVC rc.exe.
rem
rem Usage: build_windows.bat [nopause]
rem   nopause  do not pause on success/failure

set "TAG=build_windows"
set "ROOT=%~dp0"
set "DIST=%ROOT%dist"
set "NOPAUSE="
set "PROG_TOTAL=7"
set "TARGET=x86_64-pc-windows-msvc"

:parse_args
if "%~1"=="" goto :after_args
if /i "%~1"=="nopause" set "NOPAUSE=1" & shift & goto :parse_args
shift
goto :parse_args

:after_args
if not exist "%ROOT%build" mkdir "%ROOT%build" 2>nul
if not exist "%DIST%" mkdir "%DIST%" 2>nul
if not exist "%ROOT%icons" mkdir "%ROOT%icons" 2>nul
if not exist "%ROOT%screenshot" mkdir "%ROOT%screenshot" 2>nul

call :show_progress 1 %PROG_TOTAL% "Prepare build environment"
echo     Project: %ROOT%
echo     Zip: %DIST%\bm-rayfish-VERSION.zip
echo(

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

call :read_version
if errorlevel 1 goto :end_fail
echo     Version: %VER%
echo(

call :show_progress 2 %PROG_TOTAL% "Check tools cargo / MSVC target"
call :find_cargo
if errorlevel 1 goto :end_fail
call :ensure_msvc_target
echo     cargo: %CARGO%
echo(

call :show_progress 3 %PROG_TOTAL% "Remove old dist zip"
set "OLD_REMOVED=0"
for %%F in ("%DIST%\bm-rayfish-*.zip") do (
  if exist "%%~fF" (
    attrib -r "%%~fF" >nul 2>&1
    del /f /q "%%~fF" >nul 2>&1
    set "OLD_REMOVED=1"
  )
)
if "!OLD_REMOVED!"=="1" (
  echo     Removed old dist\bm-rayfish-*.zip
) else (
  echo     No old zip
)
echo(

call :show_progress 4 %PROG_TOTAL% "cargo build --release --bin ray"
"%CARGO%" -q build --release --locked --target %TARGET% --features desktop --bin ray
if errorlevel 1 (
  call :fail "cargo build --bin ray failed"
  goto :end_fail
)
set "RAY_EXE=%ROOT%target\%TARGET%\release\ray.exe"
if not exist "%RAY_EXE%" (
  call :fail "build succeeded but %RAY_EXE% is missing"
  goto :end_fail
)
echo(

set "OUT_ZIP=%DIST%\bm-rayfish-%VER%.zip"
call :show_progress 5 %PROG_TOTAL% "Pack dist zip"
powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%scripts\pack-windows-portable.ps1" -RayExe "%RAY_EXE%" -Version "%VER%" -OutputZip "%OUT_ZIP%"
if errorlevel 1 (
  call :fail "zip pack failed"
  goto :end_fail
)
if not exist "%OUT_ZIP%" (
  call :fail "zip missing after pack: %OUT_ZIP%"
  goto :end_fail
)
echo     Zip: %OUT_ZIP%
echo(

call :show_progress 6 %PROG_TOTAL% "Clear build staging"
for /d %%D in ("%ROOT%build\*") do rd /s /q "%%~fD" >nul 2>&1
del /f /q "%ROOT%build\*" >nul 2>&1
if not exist "%ROOT%build\.gitkeep" type nul > "%ROOT%build\.gitkeep"
echo(

call :show_progress 7 %PROG_TOTAL% "Done"
echo     %OUT_ZIP%
goto :end_ok

:read_version
set "VER="
for /f "usebackq tokens=2 delims==" %%A in (`findstr /b /c:"version = " "%ROOT%Cargo.toml"`) do (
  if not defined VER set "VER=%%~A"
)
set "VER=!VER: =!"
set "VER=!VER:"=!"
if "!VER!"=="" (
  call :fail "Cargo.toml version not found"
  exit /b 1
)
exit /b 0

:find_cargo
set "CARGO="
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "CARGO=%USERPROFILE%\.cargo\bin\cargo.exe"
if not defined CARGO (
  where cargo >nul 2>&1
  if not errorlevel 1 set "CARGO=cargo"
)
if not defined CARGO (
  call :fail "cargo not found. Install Rust and add %%USERPROFILE%%\.cargo\bin to PATH"
  exit /b 1
)
exit /b 0

:ensure_msvc_target
where rustup >nul 2>&1
if errorlevel 1 exit /b 0
rustup target add %TARGET% >nul 2>&1
exit /b 0

:fail
if not "%~1"=="" echo [%TAG%] FAIL: %~1
exit /b 1

:show_progress
set /a "_pct=(%~1*100)/%~2"
if %~1 LEQ 0 set "_pct=0"
echo([%~1/%~2 !_pct!%%] %~3
exit /b 0

:end_fail
echo(
echo [%TAG%] FAILED
if defined NOPAUSE exit /b 1
pause
exit /b 1

:end_ok
echo(
echo [%TAG%] OK
if defined NOPAUSE exit /b 0
pause
exit /b 0
