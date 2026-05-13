const temporaryPasswordRefreshRetryInterval = Duration(seconds: 3);

bool shouldRetryTemporaryPasswordRefresh({
  required bool isRefreshingTemporaryPassword,
  required DateTime? lastRequestedAt,
  required DateTime now,
  Duration retryInterval = temporaryPasswordRefreshRetryInterval,
}) {
  if (!isRefreshingTemporaryPassword || lastRequestedAt == null) {
    return true;
  }
  return now.difference(lastRequestedAt) >= retryInterval;
}