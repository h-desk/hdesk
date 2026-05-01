import 'package:flutter/foundation.dart';
import 'package:local_notifier/local_notifier.dart';

import '../common.dart';

bool _desktopConnectionNotifierInitialized = false;
bool _desktopConnectionNotifierSetupTried = false;

Future<void> initDesktopConnectionNotifier() async {
  if (!isDesktop || _desktopConnectionNotifierSetupTried) {
    return;
  }
  _desktopConnectionNotifierSetupTried = true;
  try {
    await localNotifier.setup(
      appName: 'HDesk',
      shortcutPolicy: ShortcutPolicy.requireCreate,
    );
    _desktopConnectionNotifierInitialized = true;
  } catch (error, stackTrace) {
    debugPrint(
        'initDesktopConnectionNotifier failed: $error\n$stackTrace');
  }
}

void showConnectionEstablishedNotification({
  required String controllerName,
  required String peerId,
  String controllerTitle = '',
}) {
  if (!_desktopConnectionNotifierInitialized ||
      !isDesktop ||
      desktopType != DesktopType.main) {
    return;
  }

  final normalizedName = controllerName.trim().isNotEmpty
      ? controllerName.trim()
      : (peerId.trim().isNotEmpty ? peerId.trim() : 'Remote controller');
  final normalizedPeerId = peerId.trim();
  final normalizedTitle = controllerTitle.trim();
  final details = <String>[];
  if (normalizedTitle.isNotEmpty) {
    details.add(normalizedTitle);
  }
  if (normalizedPeerId.isNotEmpty) {
    details.add('ID: $normalizedPeerId');
  }
  final notification = LocalNotification(
    title: '$normalizedName ${translate('Connected')}',
    body: details.isEmpty ? translate('Connected') : details.join(' · '),
  );
  try {
    notification.show();
  } catch (error, stackTrace) {
    debugPrint(
        'showConnectionEstablishedNotification failed: $error\n$stackTrace');
  }
}
