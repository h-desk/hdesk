@echo off
set "PATH=D:\software\flutter\bin;%PATH%"
echo [Flutter Windows Build - no codegen]
cd /d D:\ideas\rustdesk
python build.py --flutter --skip-portable-pack > D:\ideas\rustdesk\flutter-build-log.txt 2>&1
echo EXIT:%ERRORLEVEL%>> D:\ideas\rustdesk\flutter-build-log.txt
