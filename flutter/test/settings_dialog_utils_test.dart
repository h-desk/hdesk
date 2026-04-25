import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/desktop/pages/settings_dialog_utils.dart';

void main() {
  group('resolveVisibleSelection', () {
    test('returns requested item when visible', () {
      final result = resolveVisibleSelection('display', ['general', 'display']);
      expect(result, 'display');
    });

    test('falls back to first visible item and reports fallback', () {
      String? fallbackMessage;
      final result = resolveVisibleSelection(
        'plugin',
        ['general', 'about'],
        onFallback: (requested, fallback) {
          fallbackMessage = '$requested->$fallback';
        },
      );
      expect(result, 'general');
      expect(fallbackMessage, 'plugin->general');
    });

    test('returns null for empty visible list', () {
      final result = resolveVisibleSelection<String>('general', const []);
      expect(result, isNull);
    });
  });

  group('computeSettingsDialogSize', () {
    test('clamps to preferred size inside normal viewport', () {
      final size = computeSettingsDialogSize(const Size(1440, 900));
      expect(size.width, 820);
      expect(size.height, 720);
    });

    test('does not exceed a narrow viewport', () {
      final size = computeSettingsDialogSize(const Size(360, 500));
      expect(size.width, 312);
      expect(size.height, 410);
    });

    test('uses available size when smaller than minimum threshold', () {
      final size = computeSettingsDialogSize(const Size(430, 460));
      expect(size.width, 382);
      expect(size.height, closeTo(377.2, 0.001));
    });

    test('never expands beyond zero available width', () {
      final size = computeSettingsDialogSize(const Size(40, 100));
      expect(size.width, 0);
      expect(size.height, 82);
    });
  });
}
