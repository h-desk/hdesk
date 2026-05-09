bool shouldRecalculateHomeWindowSizeAfterHelpCardsUpdate({
  required bool isIncomingOnly,
  required bool usesCompactHomeLayout,
}) {
  return isIncomingOnly && usesCompactHomeLayout;
}