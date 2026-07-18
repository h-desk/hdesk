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

Rect computeDpiAwareDialogFrame({
  required Rect visibleFrame,
  required double scaleFactor,
  required Size preferredLogicalSize,
  double horizontalPadding = 48,
  double heightFactor = 0.82,
  double minimumLogicalWidth = 420,
  double minimumLogicalHeight = 420,
}) {
  final scale = scaleFactor.isFinite && scaleFactor > 0 ? scaleFactor : 1.0;
  final logicalViewport = Size(
    visibleFrame.width / scale,
    visibleFrame.height / scale,
  );
  final logicalSize = computeSettingsDialogSize(
    logicalViewport,
    horizontalPadding: horizontalPadding,
    heightFactor: heightFactor,
    preferredWidth: preferredLogicalSize.width,
    preferredHeight: preferredLogicalSize.height,
    minimumWidth: minimumLogicalWidth,
    minimumHeight: minimumLogicalHeight,
  );
  final physicalSize = Size(
    logicalSize.width * scale,
    logicalSize.height * scale,
  );

  return Rect.fromLTWH(
    visibleFrame.left + (visibleFrame.width - physicalSize.width) / 2,
    visibleFrame.top + (visibleFrame.height - physicalSize.height) / 2,
    physicalSize.width,
    physicalSize.height,
  );
}
