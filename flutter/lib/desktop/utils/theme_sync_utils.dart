bool didDesktopMainWindowAcknowledge(dynamic result) {
  return result == true;
}

bool isValidDesktopHomeServerId(String text) {
  final digits = text.replaceAll(' ', '');
  return digits.length >= 6 && RegExp(r'^\d+$').hasMatch(digits);
}

String selectDesktopHomeServerId({
  required String currentServerId,
  required String lastReadyServerId,
  required bool preserveLastReadyServerId,
}) {
  if (isValidDesktopHomeServerId(currentServerId)) {
    return currentServerId;
  }
  if (preserveLastReadyServerId &&
      isValidDesktopHomeServerId(lastReadyServerId)) {
    return lastReadyServerId;
  }
  return '';
}