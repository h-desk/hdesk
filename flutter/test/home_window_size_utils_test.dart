import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/desktop/utils/home_window_size_utils.dart';

void main() {
  group('shouldRecalculateHomeWindowSizeAfterHelpCardsUpdate', () {
    test('returns true only for incoming-only compact home', () {
      expect(
        shouldRecalculateHomeWindowSizeAfterHelpCardsUpdate(
          isIncomingOnly: true,
          usesCompactHomeLayout: true,
        ),
        isTrue,
      );

      expect(
        shouldRecalculateHomeWindowSizeAfterHelpCardsUpdate(
          isIncomingOnly: false,
          usesCompactHomeLayout: true,
        ),
        isFalse,
      );

      expect(
        shouldRecalculateHomeWindowSizeAfterHelpCardsUpdate(
          isIncomingOnly: true,
          usesCompactHomeLayout: false,
        ),
        isFalse,
      );
    });
  });
}