set sh1 to "launchctl unload -w /Library/LaunchDaemons/cn.yunjichuangzhi.hdesk_service.plist;"
set sh2 to "/bin/rm /Library/LaunchDaemons/cn.yunjichuangzhi.hdesk_service.plist;"
set sh3 to "/bin/rm /Library/LaunchAgents/cn.yunjichuangzhi.hdesk_server.plist;"

set sh to sh1 & sh2 & sh3
do shell script sh with prompt "HDesk wants to unload daemon" with administrator privileges