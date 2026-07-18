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

  group('computeDpiAwareDialogFrame', () {
    test('centers the preferred logical size at 100 percent scaling', () {
      final frame = computeDpiAwareDialogFrame(
        visibleFrame: const Rect.fromLTWH(0, 0, 1920, 1040),
        scaleFactor: 1,
        preferredLogicalSize: const Size(820, 720),
      );

      expect(frame, const Rect.fromLTWH(550, 160, 820, 720));
    });

    test('converts logical size to physical pixels at 200 percent scaling', () {
      final frame = computeDpiAwareDialogFrame(
        visibleFrame: const Rect.fromLTWH(0, 0, 1920, 1040),
        scaleFactor: 2,
        preferredLogicalSize: const Size(820, 720),
      );

      expect(frame.left, closeTo(140, 0.001));
      expect(frame.top, closeTo(93.6, 0.001));
      expect(frame.width, closeTo(1640, 0.001));
      expect(frame.height, closeTo(852.8, 0.001));
    });

    test('uses password window sizing rules at 150 percent scaling', () {
      final frame = computeDpiAwareDialogFrame(
        visibleFrame: const Rect.fromLTWH(0, 0, 1920, 1040),
        scaleFactor: 1.5,
        preferredLogicalSize: const Size(560, 380),
        horizontalPadding: 32,
        heightFactor: 0.9,
        minimumLogicalWidth: 360,
        minimumLogicalHeight: 320,
      );

      expect(frame.left, closeTo(540, 0.001));
      expect(frame.top, closeTo(235, 0.001));
      expect(frame.width, closeTo(840, 0.001));
      expect(frame.height, closeTo(570, 0.001));
    });

    test('falls back to one for an invalid scale factor', () {
      final frame = computeDpiAwareDialogFrame(
        visibleFrame: const Rect.fromLTWH(100, 50, 1000, 700),
        scaleFactor: 0,
        preferredLogicalSize: const Size(560, 380),
      );

      expect(frame.left, closeTo(320, 0.001));
      expect(frame.top, closeTo(210, 0.001));
      expect(frame.width, closeTo(560, 0.001));
      expect(frame.height, closeTo(380, 0.001));
    });
  });
}
