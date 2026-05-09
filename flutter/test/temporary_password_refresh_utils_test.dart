import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/models/temporary_password_refresh_utils.dart';

void main() {
  group('shouldRetryTemporaryPasswordRefresh', () {
    final now = DateTime(2026, 5, 8, 12, 0, 0);

    test('retries immediately when not already refreshing', () {
      expect(
        shouldRetryTemporaryPasswordRefresh(
          isRefreshingTemporaryPassword: false,
          lastRequestedAt: null,
          now: now,
        ),
        isTrue,
      );
    });

    test('does not retry before the retry interval elapses', () {
      expect(
        shouldRetryTemporaryPasswordRefresh(
          isRefreshingTemporaryPassword: true,
          lastRequestedAt: now.subtract(const Duration(seconds: 2)),
          now: now,
        ),
        isFalse,
      );
    });

    test('retries again after the retry interval elapses', () {
      expect(
        shouldRetryTemporaryPasswordRefresh(
          isRefreshingTemporaryPassword: true,
          lastRequestedAt: now.subtract(const Duration(seconds: 3)),
          now: now,
        ),
        isTrue,
      );
    });

    test('allows retry exactly at the interval boundary', () {
      expect(
        shouldRetryTemporaryPasswordRefresh(
          isRefreshingTemporaryPassword: true,
          lastRequestedAt: now.subtract(temporaryPasswordRefreshRetryInterval),
          now: now,
        ),
        isTrue,
      );
    });

    test('honors a custom retry interval', () {
      expect(
        shouldRetryTemporaryPasswordRefresh(
          isRefreshingTemporaryPassword: true,
          lastRequestedAt: now.subtract(const Duration(milliseconds: 2500)),
          now: now,
          retryInterval: const Duration(milliseconds: 2000),
        ),
        isTrue,
      );
    });
  });
}