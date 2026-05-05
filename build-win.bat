@echo off
set "FLUTTER_ROOT=D:\software\flutter-3.41.4"
if not exist "%FLUTTER_ROOT%\bin\flutter.bat" (
	echo Missing Flutter SDK: %FLUTTER_ROOT%\bin\flutter.bat
	exit /b 1
)
set PATH=%FLUTTER_ROOT%\bin;%PATH%
cd /d D:\ideas\rustdesk\flutter
flutter build windows --release > D:\ideas\rustdesk\flutter-build-log.txt 2>&1
echo EXIT:%ERRORLEVEL%>> D:\ideas\rustdesk\flutter-build-log.txt
