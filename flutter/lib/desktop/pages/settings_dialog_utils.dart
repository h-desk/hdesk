import 'dart:math' as math;

import 'package:flutter/material.dart';

T? resolveVisibleSelection<T>(
  T requested,
  List<T> visibleItems, {
  void Function(T requested, T fallback)? onFallback,
}) {
  if (visibleItems.isEmpty) {
    return null;
  }
  if (visibleItems.contains(requested)) {
    return requested;
  }
  final fallback = visibleItems.first;
  onFallback?.call(requested, fallback);
  return fallback;
}

double fitPreferredExtent({
  required double available,
  required double preferred,
  required double minimum,
}) {
  if (available <= 0) {
    return 0;
  }
  if (available < minimum) {
    return available;
  }
  return math.min(preferred, available);
}

Size computeSettingsDialogSize(
  Size viewport, {
  double horizontalPadding = 48,
  double heightFactor = 0.82,
  double preferredWidth = 820,
  double preferredHeight = 720,
  double minimumWidth = 420,
  double minimumHeight = 420,
}) {
  return Size(
    fitPreferredExtent(
      available: viewport.width - horizontalPadding,
      preferred: preferredWidth,
      minimum: minimumWidth,
    ),
    fitPreferredExtent(
      available: viewport.height * heightFactor,
      preferred: preferredHeight,
      minimum: minimumHeight,
    ),
  );
}
