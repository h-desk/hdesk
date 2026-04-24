@echo off
set PATH=D:\software\flutter\bin;%PATH%
cd /d D:\ideas\rustdesk\flutter
flutter build windows --release > D:\ideas\rustdesk\flutter-build-log.txt 2>&1
echo EXIT:%ERRORLEVEL%>> D:\ideas\rustdesk\flutter-build-log.txt
