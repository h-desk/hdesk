import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/desktop/utils/theme_sync_utils.dart';

void main() {
  group('didDesktopMainWindowAcknowledge', () {
    test('accepts only explicit true as acknowledgement', () {
      expect(didDesktopMainWindowAcknowledge(true), isTrue);
      expect(didDesktopMainWindowAcknowledge(null), isFalse);
      expect(didDesktopMainWindowAcknowledge(false), isFalse);
      expect(didDesktopMainWindowAcknowledge('true'), isFalse);
    });
  });

  group('selectDesktopHomeServerId', () {
    test('prefers current valid id', () {
      final selected = selectDesktopHomeServerId(
        currentServerId: '123 456',
        lastReadyServerId: '654 321',
        preserveLastReadyServerId: true,
      );

      expect(selected, '123 456');
    });

    test('falls back to last ready id only during theme refresh preservation', () {
      final selected = selectDesktopHomeServerId(
        currentServerId: '',
        lastReadyServerId: '654 321',
        preserveLastReadyServerId: true,
      );

      expect(selected, '654 321');
    });

    test('returns empty when fallback is not armed', () {
      final selected = selectDesktopHomeServerId(
        currentServerId: '',
        lastReadyServerId: '654 321',
        preserveLastReadyServerId: false,
      );

      expect(selected, isEmpty);
    });
  });
}