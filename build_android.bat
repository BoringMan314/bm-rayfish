@echo off
chcp 65001 >nul 2>&1
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

rem Windows APK build. Mirrors `just apk` (UniFFI Kotlin + Gradle) but ships a
rem release APK to dist\, matching other bm-* root build_*.bat scripts.
rem
rem Needs: Rust (cargo), cargo-ndk, rustup Android targets, JDK 17+, Android SDK,
rem        NDK 27.2.12479018 (see android/app/build.gradle.kts).
rem Signing: android/keystore.properties if present, else the SDK debug keystore.
rem
rem Usage: build_android.bat [debug] [nopause]
rem   debug    assembleDebug instead of assembleRelease
rem   nopause  do not pause on success/failure

set "TAG=build_android"
set "ROOT=%~dp0"
set "DIST=%ROOT%dist"
set "ANDROID_DIR=%ROOT%android"
set "NOPAUSE="
set "VARIANT=release"
set "GRADLE_TASK=:app:assembleRelease"
set "PROG_TOTAL=7"

:parse_args
if "%~1"=="" goto :after_args
if /i "%~1"=="nopause" set "NOPAUSE=1" & shift & goto :parse_args
if /i "%~1"=="debug" (
  set "VARIANT=debug"
  set "GRADLE_TASK=:app:assembleDebug"
  shift
  goto :parse_args
)
shift
goto :parse_args

:after_args
call :show_progress 1 %PROG_TOTAL% "準備建置環境"
echo     專案: %ROOT%
echo     輸出: %DIST%\bm-rayfish-^<version^>.apk
echo     變體: %VARIANT%
echo.

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

call :read_version
if errorlevel 1 goto :end_fail
echo     版本: %VER%
echo.

call :show_progress 2 %PROG_TOTAL% "檢查工具 (cargo / cargo-ndk / Java / SDK)"
call :find_cargo
if errorlevel 1 goto :end_fail
call :find_java
if errorlevel 1 goto :end_fail
call :find_sdk
if errorlevel 1 goto :end_fail
call :ensure_local_properties
if errorlevel 1 goto :end_fail
call :ensure_ndk
if errorlevel 1 goto :end_fail
call :ensure_android_targets
call :check_cargo_ndk
if errorlevel 1 goto :end_fail
if not exist "%ANDROID_DIR%\gradlew.bat" (
  call :fail "找不到 android\gradlew.bat"
  goto :end_fail
)
echo     cargo: %CARGO%
echo     JAVA_HOME: %JAVA_HOME%
echo     ANDROID_HOME: %ANDROID_HOME%
if defined ANDROID_NDK_HOME echo     ANDROID_NDK_HOME: %ANDROID_NDK_HOME%
echo.

call :show_progress 3 %PROG_TOTAL% "清理 dist 舊 APK"
if not exist "%DIST%" mkdir "%DIST%" 2>nul
set "OLD_REMOVED=0"
for %%F in ("%DIST%\bm-rayfish-*.apk") do (
  if exist "%%~fF" (
    attrib -r "%%~fF" >nul 2>&1
    del /f /q "%%~fF" >nul 2>&1
    set "OLD_REMOVED=1"
  )
)
if "!OLD_REMOVED!"=="1" (
  echo     已刪除舊的 dist\bm-rayfish-*.apk
) else (
  echo     沒有舊 APK，略過
)
echo.

call :show_progress 4 %PROG_TOTAL% "產生 UniFFI Kotlin 綁定"
"%CARGO%" -q build -p ray-mobile
if errorlevel 1 (
  call :fail "cargo build -p ray-mobile 失敗"
  goto :end_fail
)
set "HOST_LIB="
if exist "%ROOT%target\debug\ray_mobile.dll" set "HOST_LIB=%ROOT%target\debug\ray_mobile.dll"
if not defined HOST_LIB if exist "%ROOT%target\debug\libray_mobile.dll" set "HOST_LIB=%ROOT%target\debug\libray_mobile.dll"
if not defined HOST_LIB if exist "%ROOT%target\debug\libray_mobile.so" set "HOST_LIB=%ROOT%target\debug\libray_mobile.so"
if not defined HOST_LIB (
  call :fail "找不到 host cdylib（ray_mobile.dll），無法跑 uniffi-bindgen"
  goto :end_fail
)
"%CARGO%" -q run -p ray-mobile --bin uniffi-bindgen -- generate --library "%HOST_LIB%" --language kotlin --out-dir "%ANDROID_DIR%\app\src\main\java"
if errorlevel 1 (
  call :fail "uniffi-bindgen 失敗"
  goto :end_fail
)
echo     綁定: android\app\src\main\java\uniffi\ray_mobile\
echo.

call :show_progress 5 %PROG_TOTAL% "Gradle %GRADLE_TASK%（含 cargo-ndk .so）"
pushd "%ANDROID_DIR%"
call gradlew.bat --no-daemon %GRADLE_TASK%
set "GERR=!ERRORLEVEL!"
popd
if not "!GERR!"=="0" (
  call :fail "Gradle 建置失敗 (exit !GERR!)"
  goto :end_fail
)
echo.

call :show_progress 6 %PROG_TOTAL% "複製 APK 到 dist"
set "SRC_APK="
set "OUT_NAME=bm-rayfish-%VER%.apk"
if /i "%VARIANT%"=="debug" (
  if exist "%ANDROID_DIR%\app\build\outputs\apk\debug\app-debug.apk" (
    set "SRC_APK=%ANDROID_DIR%\app\build\outputs\apk\debug\app-debug.apk"
    set "OUT_NAME=bm-rayfish-%VER%-debug.apk"
  )
) else (
  if exist "%ANDROID_DIR%\app\build\outputs\apk\release\app-release.apk" (
    set "SRC_APK=%ANDROID_DIR%\app\build\outputs\apk\release\app-release.apk"
  ) else if exist "%ANDROID_DIR%\app\build\outputs\apk\release\app-release-unsigned.apk" (
    set "SRC_APK=%ANDROID_DIR%\app\build\outputs\apk\release\app-release-unsigned.apk"
    set "OUT_NAME=bm-rayfish-%VER%-unsigned.apk"
  )
)
if not defined SRC_APK (
  call :fail "Gradle 成功但找不到輸出 APK"
  goto :end_fail
)
copy /y "%SRC_APK%" "%DIST%\%OUT_NAME%" >nul
if errorlevel 1 (
  call :fail "無法複製 APK 到 dist"
  goto :end_fail
)
if not exist "%DIST%\%OUT_NAME%" (
  call :fail "複製後 dist 沒有 %OUT_NAME%"
  goto :end_fail
)
echo     來源: %SRC_APK%
echo     輸出: %DIST%\%OUT_NAME%
echo.

call :show_progress 7 %PROG_TOTAL% "建置完成"
echo     %DIST%\%OUT_NAME%
goto :end_ok

:read_version
set "VER="
for /f "usebackq tokens=2 delims==" %%A in (`findstr /b /c:"version = " "%ROOT%Cargo.toml"`) do (
  set "VER=%%~A"
  goto :strip_ver
)
call :fail "Cargo.toml 讀不到 version"
exit /b 1
:strip_ver
set "VER=!VER: =!"
set "VER=!VER:"=!"
if "!VER!"=="" (
  call :fail "Cargo.toml version 是空的"
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
  call :fail "找不到 cargo。請安裝 Rust 並把 %%USERPROFILE%%\.cargo\bin 加入 PATH"
  exit /b 1
)
exit /b 0

:check_cargo_ndk
if exist "%USERPROFILE%\.cargo\bin\cargo-ndk.exe" goto :ndk_ok
where cargo-ndk >nul 2>&1
if not errorlevel 1 goto :ndk_ok
echo     未安裝 cargo-ndk，正在執行 cargo install cargo-ndk …
"%CARGO%" install cargo-ndk
if errorlevel 1 (
  call :fail "cargo install cargo-ndk 失敗"
  exit /b 1
)
if exist "%USERPROFILE%\.cargo\bin\cargo-ndk.exe" goto :ndk_ok
where cargo-ndk >nul 2>&1
if not errorlevel 1 goto :ndk_ok
call :fail "安裝後仍找不到 cargo-ndk.exe"
exit /b 1
:ndk_ok
echo     cargo-ndk: 已就緒
exit /b 0

:find_java
if defined JAVA_HOME if exist "%JAVA_HOME%\bin\java.exe" goto :java_ok
if exist "%ProgramFiles%\Android\Android Studio\jbr\bin\java.exe" (
  set "JAVA_HOME=%ProgramFiles%\Android\Android Studio\jbr"
  goto :java_ok
)
if exist "%LOCALAPPDATA%\Programs\Android Studio\jbr\bin\java.exe" (
  set "JAVA_HOME=%LOCALAPPDATA%\Programs\Android Studio\jbr"
  goto :java_ok
)
where java >nul 2>&1
if not errorlevel 1 goto :java_ok
call :fail "找不到 JDK 17+。請設定 JAVA_HOME，或安裝 Android Studio（內建 JBR）"
exit /b 1
:java_ok
exit /b 0

:find_sdk
if defined ANDROID_HOME if exist "%ANDROID_HOME%\platform-tools" goto :sdk_ok
if defined ANDROID_SDK_ROOT (
  set "ANDROID_HOME=%ANDROID_SDK_ROOT%"
  if exist "%ANDROID_HOME%\platform-tools" goto :sdk_ok
)
if exist "%LOCALAPPDATA%\Android\Sdk\platform-tools" (
  set "ANDROID_HOME=%LOCALAPPDATA%\Android\Sdk"
  goto :sdk_ok
)
if exist "%USERPROFILE%\AppData\Local\Android\Sdk\platform-tools" (
  set "ANDROID_HOME=%USERPROFILE%\AppData\Local\Android\Sdk"
  goto :sdk_ok
)
call :fail "找不到 Android SDK。請安裝 SDK 或設定 ANDROID_HOME"
exit /b 1
:sdk_ok
exit /b 0

:ensure_local_properties
if exist "%ANDROID_DIR%\local.properties" exit /b 0
set "SDK_PROP=%ANDROID_HOME:\=/%"
> "%ANDROID_DIR%\local.properties" echo sdk.dir=%SDK_PROP%
echo     已寫入 android\local.properties
exit /b 0

:ensure_ndk
set "PINNED_NDK=%ANDROID_HOME%\ndk\27.2.12479018"
if exist "%PINNED_NDK%\source.properties" goto :pinned_ndk_ok
if exist "%PINNED_NDK%\ndk-build.cmd" goto :pinned_ndk_ok
echo     未安裝 NDK 27.2.12479018，正在用 sdkmanager 安裝…
set "SDKMANAGER="
if exist "%ANDROID_HOME%\cmdline-tools\latest\bin\sdkmanager.bat" set "SDKMANAGER=%ANDROID_HOME%\cmdline-tools\latest\bin\sdkmanager.bat"
if not defined SDKMANAGER if exist "%ANDROID_HOME%\cmdline-tools\bin\sdkmanager.bat" set "SDKMANAGER=%ANDROID_HOME%\cmdline-tools\bin\sdkmanager.bat"
if not defined SDKMANAGER (
  call :fail "找不到 sdkmanager.bat，無法安裝 NDK。請在 Android Studio SDK Manager 安裝 NDK 27.2.12479018"
  exit /b 1
)
call "%SDKMANAGER%" --sdk_root="%ANDROID_HOME%" "ndk;27.2.12479018"
if errorlevel 1 (
  call :fail "sdkmanager 安裝 NDK 27.2.12479018 失敗"
  exit /b 1
)
if not exist "%PINNED_NDK%\source.properties" if not exist "%PINNED_NDK%\ndk-build.cmd" (
  call :fail "安裝後仍找不到 %PINNED_NDK%"
  exit /b 1
)
:pinned_ndk_ok
set "ANDROID_NDK_HOME=%PINNED_NDK%"
echo     NDK: %ANDROID_NDK_HOME%
exit /b 0

:ensure_android_targets
where rustup >nul 2>&1
if errorlevel 1 exit /b 0
rustup target add aarch64-linux-android x86_64-linux-android >nul 2>&1
exit /b 0

:fail
if not "%~1"=="" echo [%TAG%] FAIL: %~1
exit /b 1

:show_progress
set /a "_pct=(%~1*100)/%~2"
if %~1 LEQ 0 set "_pct=0"
echo [%~1/%~2 !_pct!%%] %~3
exit /b 0

:end_fail
echo.
echo [%TAG%] 建置失敗
if defined NOPAUSE exit /b 1
pause
exit /b 1

:end_ok
echo.
echo [%TAG%] OK
if defined NOPAUSE exit /b 0
pause
exit /b 0
