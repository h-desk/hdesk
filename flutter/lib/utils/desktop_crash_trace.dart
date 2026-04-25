import 'dart:io';

class DesktopCrashTrace {
  DesktopCrashTrace._();

  static void log(String message) {
    try {
      final path = _resolveLogPath();
      if (path == null || path.isEmpty) {
        return;
      }
      final file = File(path);
      file.parent.createSync(recursive: true);
      file.writeAsStringSync(
        '[${DateTime.now().toIso8601String()}][pid=$pid] $message\n',
        mode: FileMode.append,
        flush: true,
      );
    } catch (_) {}
  }

  static String? _resolveLogPath() {
    final appData = Platform.environment['APPDATA'];
    if (appData != null && appData.isNotEmpty) {
      return '$appData\\HDesk\\log\\flutter_crash_trace.log';
    }
    final home = Platform.environment['HOME'];
    if (home != null && home.isNotEmpty) {
      return '$home/.config/HDesk/flutter_crash_trace.log';
    }
    return null;
  }
}