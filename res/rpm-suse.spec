Name:       hdesk
Version:    1.1.9
Release:    0
Summary:    HDesk RPM package
License:    GPL-3.0
URL:        https://apps.yunjichuangzhi.cn/hdesk/index.html
Vendor:     Yunji Chuangzhi (Shenzhen) Technology Co., Ltd.
Provides:   rustdesk
Obsoletes:  rustdesk
Requires:   gtk3 libxcb1 libXfixes3 alsa-utils libXtst6 libva2 pam gstreamer-plugins-base gstreamer-plugin-pipewire
Recommends: libayatana-appindicator3-1 xdotool

# https://docs.fedoraproject.org/en-US/packaging-guidelines/Scriptlets/

%description
HDesk remote desktop client software.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

%global __python %{__python3}

%install
mkdir -p %{buildroot}/usr/bin/
mkdir -p %{buildroot}/usr/share/hdesk/
mkdir -p %{buildroot}/usr/share/hdesk/files/
mkdir -p %{buildroot}/usr/share/icons/hicolor/256x256/apps/
mkdir -p %{buildroot}/usr/share/icons/hicolor/scalable/apps/
install -m 755 $HBB/target/release/rustdesk %{buildroot}/usr/bin/rustdesk
install $HBB/libsciter-gtk.so %{buildroot}/usr/share/hdesk/libsciter-gtk.so
install $HBB/res/rustdesk.service %{buildroot}/usr/share/hdesk/files/hdesk.service
install $HBB/res/128x128@2x.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/rustdesk.png
install $HBB/res/scalable.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/rustdesk.svg
install $HBB/res/128x128@2x.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/hdesk.png
install $HBB/res/scalable.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/hdesk.svg
install $HBB/res/rustdesk.desktop %{buildroot}/usr/share/hdesk/files/rustdesk.desktop
install $HBB/res/rustdesk-link.desktop %{buildroot}/usr/share/hdesk/files/rustdesk-link.desktop

%files
/usr/bin/rustdesk
/usr/share/hdesk/libsciter-gtk.so
/usr/share/hdesk/files/hdesk.service
/usr/share/icons/hicolor/256x256/apps/rustdesk.png
/usr/share/icons/hicolor/scalable/apps/rustdesk.svg
/usr/share/icons/hicolor/256x256/apps/hdesk.png
/usr/share/icons/hicolor/scalable/apps/hdesk.svg
/usr/share/hdesk/files/rustdesk.desktop
/usr/share/hdesk/files/rustdesk-link.desktop

%changelog
# let's skip this for now

%pre
# can do something for centos7
case "$1" in
  1)
    # for install
  ;;
  2)
    # for upgrade
    systemctl stop hdesk || true
    systemctl stop rustdesk || true
  ;;
esac

%post
systemctl disable rustdesk || true
rm -f /etc/systemd/system/rustdesk.service /usr/lib/systemd/system/rustdesk.service || true
rm -f /usr/share/applications/hdesk.desktop /usr/share/applications/hdesk-link.desktop /usr/share/applications/rustdesk.desktop /usr/share/applications/rustdesk-link.desktop || true
cp /usr/share/hdesk/files/hdesk.service /etc/systemd/system/hdesk.service
cp /usr/share/hdesk/files/rustdesk.desktop /usr/share/applications/
cp /usr/share/hdesk/files/rustdesk-link.desktop /usr/share/applications/
systemctl daemon-reload
systemctl enable hdesk
systemctl start hdesk
update-desktop-database

%preun
case "$1" in
  0)
    # for uninstall
    systemctl stop hdesk || true
    systemctl disable hdesk || true
    systemctl stop rustdesk || true
    systemctl disable rustdesk || true
    rm /etc/systemd/system/hdesk.service || true
    rm /etc/systemd/system/rustdesk.service || true
  ;;
  1)
    # for upgrade
  ;;
esac

%postun
case "$1" in
  0)
    # for uninstall
    rm /usr/share/applications/hdesk.desktop || true
    rm /usr/share/applications/hdesk-link.desktop || true
    rm /usr/share/applications/rustdesk.desktop || true
    rm /usr/share/applications/rustdesk-link.desktop || true
    update-desktop-database
  ;;
  1)
    # for upgrade
  ;;
esac
