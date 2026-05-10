import 'dart:async';
import 'dart:io';
import 'dart:convert';

import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/widgets/custom_password.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/desktop/pages/connection_page.dart';
import 'package:flutter_hbb/desktop/pages/desktop_setting_page.dart';
import 'package:flutter_hbb/desktop/utils/home_window_size_utils.dart';
import 'package:flutter_hbb/desktop/utils/theme_sync_utils.dart';
import 'package:flutter_hbb/desktop/widgets/refresh_wrapper.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/models/server_model.dart';
import 'package:flutter_hbb/models/state_model.dart';
import 'package:flutter_hbb/plugin/ui_manager.dart';
import 'package:flutter_hbb/utils/multi_window_manager.dart';
import 'package:flutter_hbb/utils/platform_channel.dart';
import 'package:get/get.dart';
import 'package:provider/provider.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:window_manager/window_manager.dart';
import 'package:window_size/window_size.dart' as window_size;
import '../widgets/button.dart';

class DesktopHomePage extends StatefulWidget {
  const DesktopHomePage({Key? key}) : super(key: key);

  @override
  State<DesktopHomePage> createState() => _DesktopHomePageState();
}

const borderColor = Color(0xFF2F65BA);
const _pcControlBackdoorTapThreshold = 5;
const _pcControlBackdoorWindowSize = Size(854, 560);
const _compactHomePaneWidth = 300.0;
const _refreshIndicatorSize = 14.0;
const _refreshIndicatorStrokeWidth = 1.5;
const _passwordWindowSize = Size(560, 380);
const _macOSPasswordWindowSize = Size(480, 340);
const _passwordWindowHeaderHeight = 64.0;
const _kMacOSWindowControlsInset = 78.0;

int? _passwordWindowId;
Future<void>? _passwordWindowOpenTask;

class _DesktopHomePageState extends State<DesktopHomePage>
    with AutomaticKeepAliveClientMixin, WidgetsBindingObserver {
  final _leftPaneScrollController = ScrollController();

  @override
  bool get wantKeepAlive => true;
  var systemError = '';
  StreamSubscription? _uniLinksSubscription;
  var svcStopped = false.obs;
  var watchIsCanScreenRecording = false;
  var watchIsProcessTrust = false;
  var watchIsInputMonitoring = false;
  var watchIsCanRecordAudio = false;
  Timer? _updateTimer;
  Timer? _pcControlBackdoorTapTimer;
  Worker? _titleLogoTapWorker;
  bool isCardClosed = false;
  bool _pcControlBackdoorEnabled = false;
  int _pcControlBackdoorTapCount = 0;

  final RxBool _editHover = false.obs;
  final RxBool _block = false.obs;

  final GlobalKey _childKey = GlobalKey();
  String _lastReadyServerId = '';
  bool _preserveLastReadyServerIdForThemeRefresh = false;

  bool get _needsPermissionWatch =>
      watchIsCanScreenRecording ||
      watchIsProcessTrust ||
      watchIsInputMonitoring ||
      watchIsCanRecordAudio;

  bool get _usesCompactHomeLayout =>
      !bind.isOutgoingOnly() && !_pcControlBackdoorEnabled;

  Future<void> _refreshHomeState({bool pollPermissionWatch = false}) async {
    if (!isWindows) {
      final error = await bind.mainGetError();
      if (systemError != error) {
        systemError = error;
        if (mounted) {
          setState(() {});
        }
      }
    }

    final stopped = await mainGetBoolOption(kOptionStopService);
    if (stopped != svcStopped.value) {
      svcStopped.value = stopped;
      if (stopped) {
        start_service(true);
      }
      if (mounted) {
        setState(() {});
      }
    }

    if (!pollPermissionWatch || !_needsPermissionWatch) {
      return;
    }

    if (watchIsCanScreenRecording && bind.mainIsCanScreenRecording(prompt: false)) {
      watchIsCanScreenRecording = false;
      if (mounted) {
        setState(() {});
      }
    }
    if (watchIsProcessTrust && bind.mainIsProcessTrusted(prompt: false)) {
      watchIsProcessTrust = false;
      if (mounted) {
        setState(() {});
      }
    }
    if (watchIsInputMonitoring && bind.mainIsCanInputMonitoring(prompt: false)) {
      watchIsInputMonitoring = false;
      if (mounted) {
        setState(() {});
      }
    }
    if (watchIsCanRecordAudio) {
      if (isMacOS) {
        Future.microtask(() async {
          if ((await osxCanRecordAudio() == PermissionAuthorizeType.authorized)) {
            watchIsCanRecordAudio = false;
            if (mounted) {
              setState(() {});
            }
          }
        });
      } else {
        watchIsCanRecordAudio = false;
        if (mounted) {
          setState(() {});
        }
      }
    }
  }

  void _ensurePermissionWatchTimer() {
    if (!isMacOS || !_needsPermissionWatch || _updateTimer != null) {
      return;
    }
    _updateTimer = periodic_immediate(const Duration(seconds: 1), () async {
      await _refreshHomeState(pollPermissionWatch: true);
      if (!_needsPermissionWatch) {
        _updateTimer?.cancel();
        _updateTimer = null;
      }
    });
  }

  void _resetPcControlBackdoorTapProgress() {
    _pcControlBackdoorTapTimer?.cancel();
    _pcControlBackdoorTapTimer = null;
    _pcControlBackdoorTapCount = 0;
  }

  Future<void> _togglePcControlBackdoor([bool? enabled]) async {
    final nextValue = enabled ?? !_pcControlBackdoorEnabled;
    _resetPcControlBackdoorTapProgress();
    if (_pcControlBackdoorEnabled == nextValue) {
      return;
    }
    setState(() {
      _pcControlBackdoorEnabled = nextValue;
    });
    stateGlobal.desktopHomeBackdoorExpanded.value = nextValue;
    if (!bind.isOutgoingOnly()) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && isInHomePage()) {
          _updateWindowSize();
        }
      });
    }
  }

  void _handlePcControlBackdoorTap() {
    if (bind.isOutgoingOnly()) {
      return;
    }
    _pcControlBackdoorTapTimer?.cancel();
    _pcControlBackdoorTapCount += 1;
    if (_pcControlBackdoorTapCount >= _pcControlBackdoorTapThreshold) {
      unawaited(_togglePcControlBackdoor());
      return;
    }
    _pcControlBackdoorTapTimer = Timer(
      const Duration(milliseconds: 1200),
      _resetPcControlBackdoorTapProgress,
    );
  }

  void _armTransientServerIdPreservation() {
    _preserveLastReadyServerIdForThemeRefresh = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_preserveLastReadyServerIdForThemeRefresh) {
        return;
      }
      setState(() {
        _preserveLastReadyServerIdForThemeRefresh = false;
      });
    });
  }

  Widget buildLeftPane(BuildContext context) {
    final usesCompactHomeLayout = _usesCompactHomeLayout;
    final isOutgoingOnly = bind.isOutgoingOnly();
    final textColor = Theme.of(context).textTheme.titleLarge?.color;
    return ChangeNotifierProvider.value(
      value: gFFI.serverModel,
      child: Builder(builder: (context) {
        final children = <Widget>[
          if (!isOutgoingOnly) buildPresetPasswordWarning(),
          if (bind.isCustomClient())
            Align(
              alignment: Alignment.center,
              child: loadPowered(context),
            ),
          if (!isOutgoingOnly)
            Selector<ServerModel, bool>(
              selector: (_, model) => model.desktopControlledSessions.isNotEmpty,
              builder: (context, hasControlledSessions, child) {
                return Column(
                  children: [
                    if (!hasControlledSessions) buildIDBoard(context),
                    buildControlledStatusCard(context),
                    if (!hasControlledSessions) buildPasswordBoard(context),
                  ],
                );
              },
            ),
          FutureBuilder<Widget>(
            future:
                Future.value(Obx(() => buildHelpCards(stateGlobal.updateUrl.value))),
            builder: (_, data) {
              if (data.hasData) {
                // Keep the original incoming-only auto-size behavior, but do
                // not let normal compact home rebuilds recursively shrink the
                // main window after settings-driven refreshes.
                if (shouldRecalculateHomeWindowSizeAfterHelpCardsUpdate(
                    isIncomingOnly: bind.isIncomingOnly(),
                    usesCompactHomeLayout: usesCompactHomeLayout) &&
                    isInHomePage()) {
                  // Wait for the help cards to settle into the scroll view
                  // before measuring the compact incoming-only window again.
                  Future.delayed(const Duration(milliseconds: 300), () {
                    unawaited(_updateWindowSize());
                  });
                }
                return data.data!;
              }
              return const Offstage();
            },
          ),
          buildPluginEntry(),
        ];

        return Container(
          width: usesCompactHomeLayout ? _compactHomePaneWidth : 248.0,
          color: Theme.of(context).colorScheme.background,
          child: Stack(
            children: [
              Column(
                children: [
                  SingleChildScrollView(
                    controller: _leftPaneScrollController,
                    child: Column(
                      key: _childKey,
                      children: children,
                    ),
                  ),
                  Expanded(child: Container())
                ],
              ),
              if (isOutgoingOnly)
                Positioned(
                  bottom: 6,
                  left: 12,
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: InkWell(
                      child: Obx(
                        () => Icon(
                          Icons.settings,
                          color: _editHover.value
                              ? textColor
                              : Colors.grey.withOpacity(0.5),
                          size: 22,
                        ),
                      ),
                      onTap: () {
                        if (DesktopSettingPage.tabKeys.isNotEmpty) {
                          DesktopSettingPage.switch2page(
                              DesktopSettingPage.tabKeys[0]);
                        }
                      },
                      onHover: (value) => _editHover.value = value,
                    ),
                  ),
                )
            ],
          ),
        );
      }),
    );
  }

  buildRightPane(BuildContext context) {
    return Container(
      color: Theme.of(context).scaffoldBackgroundColor,
      child: ConnectionPage(),
    );
  }

  buildIDBoard(BuildContext context) {
    final model = gFFI.serverModel;
    final textColor = Theme.of(context).textTheme.titleLarge?.color;

    // 格式化 ID：每 3 位分组，更易读
    String formatId(String id) {
      final cleaned = id.replaceAll(' ', '');
      final buffer = StringBuffer();
      for (int i = 0; i < cleaned.length; i++) {
        if (i > 0 && i % 3 == 0) buffer.write(' ');
        buffer.write(cleaned[i]);
      }
      return buffer.toString();
    }

    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 14, 12, 2),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            translate("ID"),
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w600,
              color: textColor?.withOpacity(0.6),
              letterSpacing: 0.5,
            ),
          ),
          const SizedBox(height: 8),
          ListenableBuilder(
            listenable: model.serverId,
            builder: (context, _) {
              final idText = model.serverId.text;
              if (isValidDesktopHomeServerId(idText)) {
                _lastReadyServerId = idText;
              }
              final effectiveIdText = selectDesktopHomeServerId(
                currentServerId: idText,
                lastReadyServerId: _lastReadyServerId,
                preserveLastReadyServerId:
                    _preserveLastReadyServerIdForThemeRefresh,
              );
              final ready = isValidDesktopHomeServerId(effectiveIdText);
              return GestureDetector(
                onDoubleTap: ready
                    ? () {
                        Clipboard.setData(
                            ClipboardData(text: effectiveIdText.replaceAll(' ', '')));
                        showToast(translate("Copied"));
                      }
                    : null,
                child: AnimatedContainer(
                  duration: const Duration(milliseconds: 300),
                  width: double.infinity,
                  padding:
                      const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  decoration: BoxDecoration(
                    color: ready
                        ? MyTheme.accent.withOpacity(0.08)
                        : Colors.grey.withOpacity(0.05),
                    borderRadius: BorderRadius.circular(10),
                    border: Border.all(
                      color: ready
                          ? MyTheme.accent.withOpacity(0.2)
                          : Colors.grey.withOpacity(0.15),
                      width: 1,
                    ),
                  ),
                  child: ready
                      ? Text(
                        formatId(effectiveIdText),
                          maxLines: 1,
                          softWrap: false,
                          overflow: TextOverflow.fade,
                          style: TextStyle(
                            fontSize: 20,
                            fontWeight: FontWeight.w600,
                            fontFamily: 'monospace',
                            letterSpacing: 2,
                            color: textColor,
                          ),
                        )
                      : Row(
                          children: [
                            SizedBox(
                              width: 14,
                              height: 14,
                              child: CircularProgressIndicator(
                                strokeWidth: 2,
                                color: MyTheme.accent.withOpacity(0.6),
                              ),
                            ),
                            const SizedBox(width: 10),
                            Text(
                              translate('connecting_status'),
                              style: TextStyle(
                                fontSize: 13,
                                color: textColor?.withOpacity(0.45),
                                fontStyle: FontStyle.italic,
                              ),
                            ),
                          ],
                        ),
                ),
              );
            },
          ),
        ],
      ),
    );
  }

  buildPasswordBoard(BuildContext context) {
    return ChangeNotifierProvider.value(
        value: gFFI.serverModel,
        child: Consumer<ServerModel>(
          builder: (context, model, child) {
            return buildPasswordBoard2(context, model);
          },
        ));
  }

  Widget buildControlledStatusCard(BuildContext context) {
    final textColor = Theme.of(context).textTheme.titleLarge?.color;
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final requiresScreenRecordingPermission = isMacOS &&
        !bind.isOutgoingOnly() &&
        !bind.mainIsCanScreenRecording(prompt: false);

    return Consumer<ServerModel>(
      builder: (context, model, child) {
        final activeSessions = model.desktopControlledSessions;
        if (activeSessions.isEmpty) {
          return const Offstage();
        }

        final client = activeSessions.first;
        final hasMultipleControllers = activeSessions.length > 1;
        final avatar = buildAvatarWidget(
              avatar: client.avatar,
              size: 36,
              borderRadius: 10,
              fallback: _buildControllerFallbackAvatar(client.name),
            ) ??
            _buildControllerFallbackAvatar(client.name);

        Future<void> disconnect() async {
          if (hasMultipleControllers) {
            await model.closeAllDesktopControlledSessions();
          } else {
            await model.closeDesktopControlledSession(client.id);
          }
        }

        return Container(
          margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
          decoration: BoxDecoration(
            color: isDark ? const Color(0xFF2A1F24) : const Color(0xFFFFF4F4),
            borderRadius: BorderRadius.circular(16),
            border: Border.all(
              color: const Color(0xFFE66A6A).withOpacity(isDark ? 0.45 : 0.25),
              width: 1,
            ),
          ),
          child: Padding(
            padding: const EdgeInsets.all(14),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    const Icon(
                      Icons.shield_outlined,
                      size: 16,
                      color: Color(0xFFE66A6A),
                    ),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Text(
                        '被控制中',
                        style: TextStyle(
                          fontSize: 12,
                          fontWeight: FontWeight.w600,
                          color: textColor?.withValues(alpha: 0.72),
                        ),
                      ),
                    ),
                    if (hasMultipleControllers)
                      Text(
                        '+${activeSessions.length - 1}',
                        style: const TextStyle(
                          fontSize: 12,
                          fontWeight: FontWeight.w600,
                          color: Color(0xFFE66A6A),
                        ),
                      ),
                  ],
                ),
                const SizedBox(height: 10),
                Row(
                  crossAxisAlignment: CrossAxisAlignment.center,
                  children: [
                    avatar,
                    const SizedBox(width: 10),
                    Expanded(
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        mainAxisAlignment: MainAxisAlignment.center,
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            client.name.isNotEmpty
                                ? client.name
                                : 'Remote controller',
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              fontSize: 14,
                              fontWeight: FontWeight.w600,
                              color: textColor,
                            ),
                          ),
                          if (client.title.isNotEmpty) ...[
                            const SizedBox(height: 4),
                            _buildControllerTitleBadge(client.title),
                          ],
                          if (hasMultipleControllers) ...[
                            const SizedBox(height: 4),
                            Text(
                              '另有 ${activeSessions.length - 1} 个连接仍在控制',
                              style: TextStyle(
                                fontSize: 12,
                                color: textColor?.withOpacity(0.55),
                              ),
                            ),
                          ],
                        ],
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                SizedBox(
                  width: double.infinity,
                  child: TextButton(
                    onPressed: disconnect,
                    style: TextButton.styleFrom(
                      alignment: Alignment.center,
                      foregroundColor: Colors.white,
                      backgroundColor: const Color(0xFFE66A6A),
                      padding: const EdgeInsets.symmetric(vertical: 9),
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(10),
                      ),
                    ),
                    child: Text(
                      hasMultipleControllers ? '全部断开' : translate('Disconnect'),
                      textAlign: TextAlign.center,
                      style: const TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        height: 1.0,
                      ),
                    ),
                  ),
                ),
                if (requiresScreenRecordingPermission) ...[
                  const SizedBox(height: 10),
                  Container(
                    padding: const EdgeInsets.symmetric(
                        horizontal: 12, vertical: 10),
                    decoration: BoxDecoration(
                      color: isDark
                          ? const Color(0xFF3A2A20)
                          : const Color(0xFFFFF4E8),
                      borderRadius: BorderRadius.circular(12),
                      border: Border.all(
                        color: const Color(0xFFE6A15A)
                            .withOpacity(isDark ? 0.55 : 0.35),
                        width: 1,
                      ),
                    ),
                    child: Row(
                      children: [
                        const Icon(
                          Icons.visibility_outlined,
                          size: 16,
                          color: Color(0xFFE6A15A),
                        ),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            translate('config_screen'),
                            style: TextStyle(
                              fontSize: 12,
                              height: 1.35,
                              color: textColor?.withOpacity(0.85),
                            ),
                          ),
                        ),
                        const SizedBox(width: 8),
                        TextButton(
                          onPressed: () {
                            bind.mainIsCanScreenRecording(prompt: true);
                            watchIsCanScreenRecording = true;
                            setState(() {});
                          },
                          style: TextButton.styleFrom(
                            foregroundColor: const Color(0xFFE6A15A),
                            padding: const EdgeInsets.symmetric(
                                horizontal: 10, vertical: 8),
                            minimumSize: Size.zero,
                            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                          ),
                          child: Text(translate('Configure')),
                        ),
                      ],
                    ),
                  ),
                ],
              ],
            ),
          ),
        );
      },
    );
  }

  Widget _buildControllerTitleBadge(String title) {
    final palette = _controllerTitlePalette(title);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2.5),
      decoration: BoxDecoration(
        color: palette.background,
        borderRadius: BorderRadius.circular(999),
        border: Border.all(color: palette.border, width: 1),
      ),
      child: Text(
        title,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: TextStyle(
          fontSize: 9,
          fontWeight: FontWeight.w700,
          height: 1.0,
          color: palette.foreground,
          letterSpacing: 0.1,
        ),
      ),
    );
  }

  _ControllerTitlePalette _controllerTitlePalette(String title) {
    switch (title.trim()) {
      case '传奇赞助商':
        return const _ControllerTitlePalette(
          background: Color(0xFFFFF1DB),
          border: Color(0xFFFFB74D),
          foreground: Color(0xFFE67E22),
        );
      case '钻石赞助商':
        return const _ControllerTitlePalette(
          background: Color(0xFFE6FBFF),
          border: Color(0xFF7EDAE7),
          foreground: Color(0xFF0396A6),
        );
      case '赞助商':
        return const _ControllerTitlePalette(
          background: Color(0xFFF3E8FF),
          border: Color(0xFFC6A4F5),
          foreground: Color(0xFF7E57C2),
        );
      case '挚友':
        return const _ControllerTitlePalette(
          background: Color(0xFFFFF7D9),
          border: Color(0xFFF4D35E),
          foreground: Color(0xFFA67C00),
        );
      case '铁粉':
        return const _ControllerTitlePalette(
          background: Color(0xFFF1F4F8),
          border: Color(0xFFD0D7DE),
          foreground: Color(0xFF6B7280),
        );
      case '支持者':
        return const _ControllerTitlePalette(
          background: Color(0xFFFFF0E4),
          border: Color(0xFFE0A56A),
          foreground: Color(0xFFB56A2C),
        );
      default:
        return const _ControllerTitlePalette(
          background: Color(0xFFF3F4F6),
          border: Color(0xFFD1D5DB),
          foreground: Color(0xFF4B5563),
        );
    }
  }

  Widget _buildControllerFallbackAvatar(String name) {
    final trimmed = name.trim();
    final label = trimmed.isNotEmpty ? trimmed[0].toUpperCase() : '?';
    return Container(
      width: 36,
      height: 36,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: str2color(name),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Text(
        label,
        style: const TextStyle(
          color: Colors.white,
          fontWeight: FontWeight.bold,
          fontSize: 18,
        ),
      ),
    );
  }

  buildPasswordBoard2(BuildContext context, ServerModel model) {
    RxBool refreshHover = false.obs;
    RxBool editHover = false.obs;
    final textColor = Theme.of(context).textTheme.titleLarge?.color;
    final showOneTime = model.approveMode != 'click' &&
        model.verificationMethod != kUsePermanentPassword;
    final isRefreshingOneTimePassword =
        showOneTime && model.isRefreshingTemporaryPassword;

    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 4, 12, 6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            translate("One-time Password"),
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w600,
              color: textColor?.withOpacity(0.6),
              letterSpacing: 0.5,
            ),
          ),
          const SizedBox(height: 8),
          GestureDetector(
            onDoubleTap: () {
              if (showOneTime && !isRefreshingOneTimePassword) {
                Clipboard.setData(
                    ClipboardData(text: model.serverPasswd.text));
                showToast(translate("Copied"));
              }
            },
            child: Container(
              width: double.infinity,
              padding:
                  const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              decoration: BoxDecoration(
                color: showOneTime
                    ? MyTheme.accent.withOpacity(0.08)
                    : Colors.grey.withOpacity(0.05),
                borderRadius: BorderRadius.circular(10),
                border: Border.all(
                  color: showOneTime
                      ? MyTheme.accent.withOpacity(0.2)
                      : Colors.grey.withOpacity(0.15),
                  width: 1,
                ),
              ),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      showOneTime ? model.serverPasswd.text : '------',
                      style: TextStyle(
                        fontSize: 16,
                        fontWeight: FontWeight.w500,
                        fontFamily: 'monospace',
                        letterSpacing: 2,
                        color: showOneTime
                            ? textColor
                            : textColor?.withOpacity(0.5),
                      ),
                    ),
                  ),
                  if (showOneTime)
                    Obx(() => InkWell(
                          onTap: isRefreshingOneTimePassword
                              ? null
                              : model.requestTemporaryPasswordRefresh,
                          onHover: isRefreshingOneTimePassword
                              ? null
                              : (v) => refreshHover.value = v,
                          borderRadius: BorderRadius.circular(6),
                          child: AnimatedContainer(
                            duration: const Duration(milliseconds: 150),
                            padding: const EdgeInsets.all(5),
                            decoration: BoxDecoration(
                              color: isRefreshingOneTimePassword
                                  ? MyTheme.accent.withOpacity(0.12)
                                  : refreshHover.value
                                  ? MyTheme.accent.withOpacity(0.15)
                                  : Colors.transparent,
                              borderRadius: BorderRadius.circular(6),
                            ),
                            child: isRefreshingOneTimePassword
                                ? SizedBox(
                                    width: _refreshIndicatorSize,
                                    height: _refreshIndicatorSize,
                                    child: CircularProgressIndicator(
                                      strokeWidth:
                                          _refreshIndicatorStrokeWidth,
                                      color: MyTheme.accent,
                                    ),
                                  )
                                : Icon(
                                    Icons.refresh_rounded,
                                    size: _refreshIndicatorSize,
                                    color: refreshHover.value
                                        ? MyTheme.accent
                                        : MyTheme.accent.withOpacity(0.55),
                                  ),
                          ),
                        )),
                ],
              ),
            ),
          ),
          if (!bind.isDisableSettings()) ...[
            const SizedBox(height: 6),
            MouseRegion(
              onEnter: (_) => editHover.value = true,
              onExit: (_) => editHover.value = false,
              cursor: isChangePermanentPasswordDisabled()
                  ? MouseCursor.defer
                  : SystemMouseCursors.click,
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: isChangePermanentPasswordDisabled()
                    ? null
                    : () => setPasswordDialog(),
                child: Obx(() => Align(
                      alignment: Alignment.centerRight,
                      child: Padding(
                        padding: const EdgeInsets.symmetric(
                            horizontal: 2, vertical: 2),
                        child: Text(
                          translate('Set permanent password'),
                          style: TextStyle(
                            fontSize: 12,
                            color: isChangePermanentPasswordDisabled()
                                ? textColor?.withOpacity(0.28)
                                : editHover.value
                                    ? MyTheme.accent
                                    : textColor?.withOpacity(0.52),
                            fontWeight: FontWeight.w500,
                            decoration: !isChangePermanentPasswordDisabled() &&
                                    editHover.value
                                ? TextDecoration.underline
                                : TextDecoration.none,
                          ),
                        ),
                      ),
                    )),
              ),
            ),
          ],
        ],
      ),
    );
  }

  buildTip(BuildContext context) {
    final isOutgoingOnly = bind.isOutgoingOnly();
    final textColor = Theme.of(context).textTheme.titleLarge?.color;
    final canTriggerBackdoor = !isOutgoingOnly;
    final showBackdoorClose = canTriggerBackdoor && _pcControlBackdoorEnabled;

    return Padding(
      padding:
          const EdgeInsets.only(left: 12.0, right: 12, top: 16.0, bottom: 8),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              InkWell(
                borderRadius: BorderRadius.circular(12),
                onTap: canTriggerBackdoor ? _handlePcControlBackdoorTap : null,
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Container(
                      padding: const EdgeInsets.all(8),
                      decoration: BoxDecoration(
                        color: MyTheme.accent.withOpacity(0.1),
                        borderRadius: BorderRadius.circular(10),
                      ),
                      child: Icon(
                        Icons.desktop_windows_rounded,
                        color: MyTheme.accent,
                        size: 20,
                      ),
                    ),
                    const SizedBox(width: 12),
                    Text(
                      'HDesk',
                      style: TextStyle(
                        fontSize: 18,
                        fontWeight: FontWeight.w700,
                        color: textColor,
                      ),
                    ),
                  ],
                ),
              ),
              if (showBackdoorClose) const Spacer(),
              if (showBackdoorClose)
                InkWell(
                  borderRadius: BorderRadius.circular(10),
                  onTap: () => _togglePcControlBackdoor(false),
                  child: Container(
                    padding: const EdgeInsets.all(6),
                    decoration: BoxDecoration(
                      color: Colors.grey.withOpacity(0.08),
                      borderRadius: BorderRadius.circular(10),
                      border: Border.all(
                        color: Colors.grey.withOpacity(0.14),
                      ),
                    ),
                    child: Icon(
                      Icons.keyboard_double_arrow_left_rounded,
                      size: 18,
                      color: textColor?.withOpacity(0.7),
                    ),
                  ),
                ),
            ],
          ),
          const SizedBox(height: 8),
          if (!isOutgoingOnly)
            Text(
              translate("desk_tip"),
              overflow: TextOverflow.clip,
              style: TextStyle(
                fontSize: 12,
                color: textColor?.withOpacity(0.6),
              ),
            ),
          if (isOutgoingOnly)
            Text(
              translate("outgoing_only_desk_tip"),
              overflow: TextOverflow.clip,
              style: TextStyle(
                fontSize: 12,
                color: textColor?.withOpacity(0.6),
              ),
            ),
        ],
      ),
    );
  }

  Widget buildHelpCards(String updateUrl) {
    if (systemError.isNotEmpty) {
      return buildInstallCard("", systemError, "", () {});
    }

    if (isWindows && !bind.isDisableInstallation()) {
      if (!bind.mainIsInstalled()) {
        return buildInstallCard(
            "", bind.isOutgoingOnly() ? "" : "install_tip", "Install",
            () async {
          await rustDeskWinManager.closeAllSubWindows();
          bind.mainGotoInstall();
        });
      }
    } else if (isMacOS) {
      final isOutgoingOnly = bind.isOutgoingOnly();
      if (!(isOutgoingOnly || bind.mainIsCanScreenRecording(prompt: false))) {
        return buildInstallCard("Permissions", "config_screen", "Configure",
            () async {
          bind.mainIsCanScreenRecording(prompt: true);
          watchIsCanScreenRecording = true;
          _ensurePermissionWatchTimer();
        }, help: 'Help', link: translate("doc_mac_permission"));
      } else if (!isOutgoingOnly && !bind.mainIsProcessTrusted(prompt: false)) {
        return buildInstallCard("Permissions", "config_acc", "Configure",
            () async {
          bind.mainIsProcessTrusted(prompt: true);
          watchIsProcessTrust = true;
          _ensurePermissionWatchTimer();
        }, help: 'Help', link: translate("doc_mac_permission"));
      } else if (!bind.mainIsCanInputMonitoring(prompt: false)) {
        return buildInstallCard("Permissions", "config_input", "Configure",
            () async {
          bind.mainIsCanInputMonitoring(prompt: true);
          watchIsInputMonitoring = true;
          _ensurePermissionWatchTimer();
        }, help: 'Help', link: translate("doc_mac_permission"));
      } else if (!isOutgoingOnly &&
          !svcStopped.value &&
          bind.mainIsInstalled() &&
          !bind.mainIsInstalledDaemon(prompt: false)) {
        return buildInstallCard("", "install_daemon_tip", "Install", () async {
          bind.mainIsInstalledDaemon(prompt: true);
        });
      }
      //// Disable microphone configuration for macOS. We will request the permission when needed.
      // else if ((await osxCanRecordAudio() !=
      //     PermissionAuthorizeType.authorized)) {
      //   return buildInstallCard("Permissions", "config_microphone", "Configure",
      //       () async {
      //     osxRequestAudio();
      //     watchIsCanRecordAudio = true;
      //   });
      // }
    } else if (isLinux) {
      if (bind.isOutgoingOnly()) {
        return Container();
      }
      final LinuxCards = <Widget>[];
      if (bind.isSelinuxEnforcing()) {
        // Check is SELinux enforcing, but show user a tip of is SELinux enabled for simple.
        final keyShowSelinuxHelpTip = "show-selinux-help-tip";
        if (bind.mainGetLocalOption(key: keyShowSelinuxHelpTip) != 'N') {
          LinuxCards.add(buildInstallCard(
            "Warning",
            "selinux_tip",
            "",
            () async {},
            marginTop: LinuxCards.isEmpty ? 20.0 : 5.0,
            help: 'Help',
            link:
                'https://rustdesk.com/docs/en/client/linux/#permissions-issue',
            closeButton: true,
            closeOption: keyShowSelinuxHelpTip,
          ));
        }
      }
      if (bind.mainCurrentIsWayland()) {
        LinuxCards.add(buildInstallCard(
            "Warning", "wayland_experiment_tip", "", () async {},
            marginTop: LinuxCards.isEmpty ? 20.0 : 5.0,
            help: 'Help',
            link: 'https://rustdesk.com/docs/en/client/linux/#x11-required'));
      } else if (bind.mainIsLoginWayland()) {
        LinuxCards.add(buildInstallCard("Warning",
            "Login screen using Wayland is not supported", "", () async {},
            marginTop: LinuxCards.isEmpty ? 20.0 : 5.0,
            help: 'Help',
            link: 'https://rustdesk.com/docs/en/client/linux/#login-screen'));
      }
      if (LinuxCards.isNotEmpty) {
        return Column(
          children: LinuxCards,
        );
      }
    }
    if (bind.isIncomingOnly()) {
      return Align(
        alignment: Alignment.centerRight,
        child: OutlinedButton(
          onPressed: () {
            SystemNavigator.pop(); // Close the application
            // https://github.com/flutter/flutter/issues/66631
            if (isWindows) {
              exit(0);
            }
          },
          child: Text(translate('Quit')),
        ),
      ).marginAll(14);
    }
    return Container();
  }

  Widget buildInstallCard(String title, String content, String btnText,
      GestureTapCallback onPressed,
      {double marginTop = 20.0,
      String? help,
      String? link,
      bool? closeButton,
      String? closeOption}) {
    if (bind.mainGetBuildinOption(key: kOptionHideHelpCards) == 'Y' &&
        content != 'install_daemon_tip') {
      return const SizedBox();
    }
    void closeCard() async {
      if (closeOption != null) {
        await bind.mainSetLocalOption(key: closeOption, value: 'N');
        if (bind.mainGetLocalOption(key: closeOption) == 'N') {
          setState(() {
            isCardClosed = true;
          });
        }
      } else {
        setState(() {
          isCardClosed = true;
        });
      }
    }

    return Stack(
      children: [
        Container(
          margin: EdgeInsets.fromLTRB(
              0, marginTop, 0, bind.isIncomingOnly() ? marginTop : 0),
          child: Container(
              decoration: BoxDecoration(
                  gradient: LinearGradient(
                begin: Alignment.centerLeft,
                end: Alignment.centerRight,
                colors: [
                  Color.fromARGB(255, 226, 66, 188),
                  Color.fromARGB(255, 244, 114, 124),
                ],
              )),
              padding: EdgeInsets.all(20),
              child: Column(
                  mainAxisAlignment: MainAxisAlignment.start,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: (title.isNotEmpty
                          ? <Widget>[
                              Center(
                                  child: Text(
                                translate(title),
                                style: TextStyle(
                                    color: Colors.white,
                                    fontWeight: FontWeight.bold,
                                    fontSize: 15),
                              ).marginOnly(bottom: 6)),
                            ]
                          : <Widget>[]) +
                      <Widget>[
                        if (content.isNotEmpty)
                          Text(
                            translate(content),
                            style: TextStyle(
                                height: 1.5,
                                color: Colors.white,
                                fontWeight: FontWeight.normal,
                                fontSize: 13),
                          ).marginOnly(bottom: 20)
                      ] +
                      (btnText.isNotEmpty
                          ? <Widget>[
                              Row(
                                  mainAxisAlignment: MainAxisAlignment.center,
                                  children: [
                                    FixedWidthButton(
                                      width: 150,
                                      padding: 8,
                                      isOutline: true,
                                      text: translate(btnText),
                                      textColor: Colors.white,
                                      borderColor: Colors.white,
                                      textSize: 20,
                                      radius: 10,
                                      onTap: onPressed,
                                    )
                                  ])
                            ]
                          : <Widget>[]) +
                      (help != null
                          ? <Widget>[
                              Center(
                                  child: InkWell(
                                      onTap: () async =>
                                          await launchUrl(Uri.parse(link!)),
                                      child: Text(
                                        translate(help),
                                        style: TextStyle(
                                            decoration:
                                                TextDecoration.underline,
                                            color: Colors.white,
                                            fontSize: 12),
                                      )).marginOnly(top: 6)),
                            ]
                          : <Widget>[]))),
        ),
        if (closeButton != null && closeButton == true)
          Positioned(
            top: 18,
            right: 0,
            child: IconButton(
              icon: Icon(
                Icons.close,
                color: Colors.white,
                size: 20,
              ),
              onPressed: closeCard,
            ),
          ),
      ],
    );
  }

  @override
  void initState() {
    super.initState();
    stateGlobal.desktopHomeBackdoorExpanded.value = false;
    Future.microtask(() async {
      await _refreshHomeState();
      _ensurePermissionWatchTimer();
    });
    Get.put<RxBool>(svcStopped, tag: 'stop-service');
    rustDeskWinManager.registerActiveWindowListener(onActiveWindowChanged);

    screenToMap(window_size.Screen screen) => {
          'frame': {
            'l': screen.frame.left,
            't': screen.frame.top,
            'r': screen.frame.right,
            'b': screen.frame.bottom,
          },
          'visibleFrame': {
            'l': screen.visibleFrame.left,
            't': screen.visibleFrame.top,
            'r': screen.visibleFrame.right,
            'b': screen.visibleFrame.bottom,
          },
          'scaleFactor': screen.scaleFactor,
        };

    bool isChattyMethod(String methodName) {
      switch (methodName) {
        case kWindowBumpMouse:
          return true;
      }

      return false;
    }

    rustDeskWinManager.setMethodHandler((call, fromWindowId) async {
      if (!isChattyMethod(call.method)) {
        debugPrint(
            "[Main] call ${call.method} with args ${call.arguments} from window $fromWindowId");
      }
      if (call.method == kWindowMainWindowOnTop) {
        windowOnTop(null);
      } else if (call.method == kWindowRefreshCurrentUser) {
        gFFI.userModel.refreshCurrentUser();
      } else if (call.method == kWindowGetWindowInfo) {
        final screen = (await window_size.getWindowInfo()).screen;
        if (screen == null) {
          return '';
        } else {
          return jsonEncode(screenToMap(screen));
        }
      } else if (call.method == kWindowGetScreenList) {
        return jsonEncode(
            (await window_size.getScreenList()).map(screenToMap).toList());
      } else if (call.method == kWindowEventThemeMode) {
        final mode = MyTheme.themeModeFromString(call.arguments as String? ?? 'system');
        MyTheme.applyDarkModeLocally(mode);
        _armTransientServerIdPreservation();
        final rootContext = globalKey.currentContext;
        if (rootContext != null) {
          RefreshWrapper.of(rootContext)?.rebuild();
        } else if (mounted) {
          setState(() {});
        }
        final passwordWindowId = _passwordWindowId;
        if (passwordWindowId != null) {
          try {
            await DesktopMultiWindow.invokeMethod(
                passwordWindowId, kWindowActionRebuild);
          } catch (e) {
            debugPrint('Failed to rebuild password window on theme sync: $e');
            _passwordWindowId = null;
          }
        }
        return true;
      } else if (call.method == kWindowEventRefreshHome) {
        if (mounted) {
          setState(() {});
        }
        return true;
      } else if (call.method == kWindowEventPasswordWindowClosed) {
        _passwordWindowId = null;
        _passwordWindowOpenTask = null;
        return true;
      } else if (call.method == kWindowActionRebuild) {
        reloadCurrentWindow();
      } else if (call.method == kWindowEventShow) {
        await rustDeskWinManager.registerActiveWindow(call.arguments["id"]);
      } else if (call.method == kWindowEventHide) {
        await rustDeskWinManager.unregisterActiveWindow(call.arguments['id']);
      } else if (call.method == kWindowConnect) {
        await connectMainDesktop(
          call.arguments['id'],
          isFileTransfer: call.arguments['isFileTransfer'],
          isViewCamera: call.arguments['isViewCamera'],
          isTerminal: call.arguments['isTerminal'],
          isTcpTunneling: call.arguments['isTcpTunneling'],
          isRDP: call.arguments['isRDP'],
          password: call.arguments['password'],
          forceRelay: call.arguments['forceRelay'],
          connToken: call.arguments['connToken'],
        );
      } else if (call.method == kWindowBumpMouse) {
        return RdPlatformChannel.instance
            .bumpMouse(dx: call.arguments['dx'], dy: call.arguments['dy']);
      } else if (call.method == kWindowEventMoveTabToNewWindow) {
        final args = call.arguments.split(',');
        int? windowId;
        try {
          windowId = int.parse(args[0]);
        } catch (e) {
          debugPrint("Failed to parse window id '${call.arguments}': $e");
        }
        WindowType? windowType;
        try {
          windowType = WindowType.values.byName(args[3]);
        } catch (e) {
          debugPrint("Failed to parse window type '${call.arguments}': $e");
        }
        if (windowId != null && windowType != null) {
          await rustDeskWinManager.moveTabToNewWindow(
              windowId, args[1], args[2], windowType);
        }
      } else if (call.method == kWindowEventOpenMonitorSession) {
        final args = jsonDecode(call.arguments);
        final windowId = args['window_id'] as int;
        final peerId = args['peer_id'] as String;
        final display = args['display'] as int;
        final displayCount = args['display_count'] as int;
        final windowType = args['window_type'] as int;
        final screenRect = parseParamScreenRect(args);
        await rustDeskWinManager.openMonitorSession(
            windowId, peerId, display, displayCount, screenRect, windowType);
      } else if (call.method == kWindowEventRemoteWindowCoords) {
        final windowId = int.tryParse(call.arguments);
        if (windowId != null) {
          return jsonEncode(
              await rustDeskWinManager.getOtherRemoteWindowCoords(windowId));
        }
      }
    });
    _uniLinksSubscription = listenUniLinks();
    _titleLogoTapWorker =
        ever<int>(stateGlobal.desktopHomeTitleLogoTapSignal, (_) {
      if (mounted && isInHomePage()) {
        _handlePcControlBackdoorTap();
      }
    });

    if (!bind.isOutgoingOnly()) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        _scheduleWindowSizeUpdate();
      });
    }
    WidgetsBinding.instance.addObserver(this);
  }

  void _scheduleWindowSizeUpdate({int attempts = 6}) {
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      if (!mounted || bind.isOutgoingOnly()) {
        return;
      }
      final updated = await _updateWindowSize();
      if (!updated && attempts > 1) {
        _scheduleWindowSizeUpdate(attempts: attempts - 1);
      }
    });
  }

  Future<bool> _updateWindowSize() async {
    if (!mounted || bind.isOutgoingOnly()) {
      return false;
    }
    if (!stateGlobal.desktopMainWindowReady.isTrue) {
      return false;
    }
    if (!isInHomePage()) {
      return false;
    }
    if (!_usesCompactHomeLayout) {
      final currentBounds = await windowManager.getBounds();
      await windowManager.setBounds(
        Rect.fromLTWH(
          currentBounds.left,
          currentBounds.top,
          _pcControlBackdoorWindowSize.width,
          _pcControlBackdoorWindowSize.height,
        ),
      );
      return true;
    }
    RenderObject? renderObject = _childKey.currentContext?.findRenderObject();
    if (renderObject == null) {
      return false;
    }
    if (renderObject is RenderBox) {
      final size = renderObject.size;
      final targetSize = bind.isIncomingOnly()
          ? size
          : Size(_compactHomePaneWidth, size.height);
      final currentWindowSize = await windowManager.getSize();
      final expectedWindowSize = getIncomingOnlyHomeSize();
      final shouldResizeWindow =
          (currentWindowSize.width - expectedWindowSize.width).abs() > 1 ||
              (currentWindowSize.height - expectedWindowSize.height).abs() > 1;
      if (targetSize != imcomingOnlyHomeSize || shouldResizeWindow) {
        imcomingOnlyHomeSize = targetSize;
        await windowManager.setSize(getIncomingOnlyHomeSize());
      }
      return true;
    }
    return false;
  }

  @override
  void dispose() {
    stateGlobal.desktopHomeBackdoorExpanded.value = false;
    _uniLinksSubscription?.cancel();
    Get.delete<RxBool>(tag: 'stop-service');
    _updateTimer?.cancel();
    _pcControlBackdoorTapTimer?.cancel();
    _titleLogoTapWorker?.dispose();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    super.didChangeAppLifecycleState(state);
    if (state == AppLifecycleState.resumed) {
      shouldBeBlocked(_block, canBeBlocked);
    }
  }

  Widget buildPluginEntry() {
    final entries = PluginUiManager.instance.entries.entries;
    return Offstage(
      offstage: entries.isEmpty,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          ...entries.map((entry) {
            return entry.value;
          })
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final usesCompactHomeLayout = _usesCompactHomeLayout;
    return _buildBlock(
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (usesCompactHomeLayout)
            SizedBox(
              width: _compactHomePaneWidth,
              child: buildLeftPane(context),
            )
          else ...[
            buildLeftPane(context),
            const VerticalDivider(width: 1),
            Expanded(child: buildRightPane(context)),
          ],
        ],
      ),
    );
  }

  Widget _buildBlock({required Widget child}) {
    return buildRemoteBlock(
      block: _block,
      mask: true,
      use: canBeBlocked,
      child: child,
    );
  }
}

class _ControllerTitlePalette {
  const _ControllerTitlePalette({
    required this.background,
    required this.border,
    required this.foreground,
  });

  final Color background;
  final Color border;
  final Color foreground;
}

void setPasswordDialog({VoidCallback? notEmptyCallback}) async {
  if (isDesktop && !isWebDesktop) {
    unawaited(_showStandalonePasswordWindow());
    return;
  }

  final p0 = TextEditingController(text: "");
  final p1 = TextEditingController(text: "");
  var errMsg0 = "";
  var errMsg1 = "";
  var canSubmit = false;
  final RxString rxPass = "".obs;
  final rules = [
    DigitValidationRule(),
    UppercaseValidationRule(),
    LowercaseValidationRule(),
    // SpecialCharacterValidationRule(),
    MinCharactersValidationRule(8),
  ];
  final maxLength = bind.mainMaxEncryptLen();

  gFFI.dialogManager.show((setState, close, context) {
    updateCanSubmit() {
      canSubmit = p0.text.trim().isNotEmpty || p1.text.trim().isNotEmpty;
    }

    submit() async {
      if (!canSubmit) {
        return;
      }
      setState(() {
        errMsg0 = "";
        errMsg1 = "";
      });
      final pass = p0.text.trim();
      if (pass.isNotEmpty) {
        final Iterable violations = rules.where((r) => !r.validate(pass));
        if (violations.isNotEmpty) {
          setState(() {
            errMsg0 =
                '${translate('Prompt')}: ${violations.map((r) => r.name).join(', ')}';
          });
          return;
        }
      }
      if (p1.text.trim() != pass) {
        setState(() {
          errMsg1 =
              '${translate('Prompt')}: ${translate("The confirmation is not identical.")}';
        });
        return;
      }
      final ok = await bind.mainSetPermanentPasswordWithResult(password: pass);
      if (!ok) {
        setState(() {
          errMsg0 = '${translate('Prompt')}: ${translate("Failed")}';
        });
        return;
      }
      if (pass.isNotEmpty) {
        notEmptyCallback?.call();
      }
      close();
    }

    return CustomAlertDialog(
      title: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(Icons.key, color: MyTheme.accent),
          Text(translate("Set Password")).paddingOnly(left: 10),
        ],
      ),
      content: ConstrainedBox(
        constraints: const BoxConstraints(minWidth: 500),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 6),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    obscureText: true,
                    decoration: InputDecoration(
                      labelText: translate('Password'),
                      errorText: errMsg0.isNotEmpty ? errMsg0 : null,
                      counterText: '',
                    ),
                    controller: p0,
                    autofocus: true,
                    onChanged: (value) {
                      rxPass.value = value.trim();
                      setState(() {
                        errMsg0 = '';
                        updateCanSubmit();
                      });
                    },
                    maxLength: maxLength,
                  ).workaroundFreezeLinuxMint(),
                ),
              ],
            ),
            Row(
              children: [
                Expanded(child: PasswordStrengthIndicator(password: rxPass)),
              ],
            ).marginOnly(top: 2, bottom: 8),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    obscureText: true,
                    decoration: InputDecoration(
                      labelText: translate('Confirmation'),
                      errorText: errMsg1.isNotEmpty ? errMsg1 : null,
                      counterText: '',
                    ),
                    controller: p1,
                    onChanged: (value) {
                      setState(() {
                        errMsg1 = '';
                        updateCanSubmit();
                      });
                    },
                    maxLength: maxLength,
                  ).workaroundFreezeLinuxMint(),
                ),
              ],
            ),
            const SizedBox(height: 8),
          ],
        ),
      ),
      actions: (() {
        final cancelButton = dialogButton(
          "Cancel",
          icon: Icon(Icons.close_rounded),
          onPressed: close,
          isOutline: true,
        );
        final okButton = dialogButton(
          "OK",
          icon: Icon(Icons.done_rounded),
          onPressed: canSubmit ? submit : null,
        );
        return [
          cancelButton,
          okButton,
        ];
      })(),
      onSubmit: canSubmit ? submit : null,
      onCancel: close,
    );
  });
}

Future<void> _showStandalonePasswordWindow() async {
  try {
    final existingWindowId = _passwordWindowId;
    _passwordWindowId = null;
    if (existingWindowId != null) {
      try {
        await WindowController.fromWindowId(existingWindowId).close();
      } catch (_) {
        // Ignore stale window handles and recreate a fresh password window.
      }
    }

    final openTask = () async {
      final controller = await DesktopMultiWindow.createWindow(
        jsonEncode({
          'type': WindowType.Password.index,
        }),
      );
      _passwordWindowId = controller.windowId;
      if (isWindows) {
        controller.setInitBackgroundColor(Colors.transparent);
      }
      final frameSize = isMacOS ? _macOSPasswordWindowSize : _passwordWindowSize;
      await controller.setFrame(const Offset(0, 0) & frameSize);
      await controller.center();
      await controller.setTitle(getWindowName(overrideType: WindowType.Password));
      await controller.show();
      await controller.focus();
    }();

    _passwordWindowOpenTask = openTask;
    try {
      await openTask;
    } finally {
      _passwordWindowOpenTask = null;
    }
  } catch (e) {
    debugPrintStack(label: '$e');
    DesktopSettingPage.show(initialTabkey: SettingsTabKey.safety);
  }
}

class DesktopPasswordWindowPage extends StatefulWidget {
  const DesktopPasswordWindowPage({Key? key}) : super(key: key);

  @override
  State<DesktopPasswordWindowPage> createState() =>
      _DesktopPasswordWindowPageState();
}

class _DesktopPasswordWindowPageState extends State<DesktopPasswordWindowPage> {
  late final TextEditingController _passwordController;
  late final TextEditingController _confirmationController;
  late final RxString _password;
  late final List<ValidationRule> _rules;
  late final int _maxLength;

  String _passwordError = '';
  String _confirmationError = '';

  bool get _canSubmit =>
      _passwordController.text.trim().isNotEmpty ||
      _confirmationController.text.trim().isNotEmpty;

  @override
  void initState() {
    super.initState();
    DesktopMultiWindow.setMethodHandler((call, fromWindowId) async {
      if (call.method == kWindowActionRebuild) {
        reloadCurrentWindow();
        return true;
      }
      return null;
    });
    _passwordController = TextEditingController();
    _confirmationController = TextEditingController();
    _password = ''.obs;
    _rules = [
      DigitValidationRule(),
      UppercaseValidationRule(),
      LowercaseValidationRule(),
      MinCharactersValidationRule(8),
    ];
    _maxLength = bind.mainMaxEncryptLen();
  }

  @override
  void dispose() {
    _passwordController.dispose();
    _confirmationController.dispose();
    super.dispose();
  }

  Future<void> _closeWindow() async {
    final windowId = stateGlobal.windowId;
    if (windowId < 0) {
      return;
    }
    try {
      await DesktopMultiWindow.invokeMethod(
          kMainWindowId, kWindowEventPasswordWindowClosed, null);
      await WindowController.fromWindowId(windowId).close();
    } catch (e) {
      debugPrint('Failed to close password window: $e');
    }
  }

  void _startWindowDrag() {
    final windowId = stateGlobal.windowId;
    if (windowId >= 0) {
      WindowController.fromWindowId(windowId).startDragging();
    }
  }

  Future<void> _submit() async {
    if (!_canSubmit) {
      return;
    }

    setState(() {
      _passwordError = '';
      _confirmationError = '';
    });

    final pass = _passwordController.text.trim();
    if (pass.isNotEmpty) {
      final violations = _rules.where((rule) => !rule.validate(pass));
      if (violations.isNotEmpty) {
        setState(() {
          _passwordError =
              '${translate('Prompt')}: ${violations.map((rule) => rule.name).join(', ')}';
        });
        return;
      }
    }

    if (_confirmationController.text.trim() != pass) {
      setState(() {
        _confirmationError =
            '${translate('Prompt')}: ${translate("The confirmation is not identical.")}';
      });
      return;
    }

    final ok = await bind.mainSetPermanentPasswordWithResult(password: pass);
    if (!ok) {
      setState(() {
        _passwordError = '${translate('Prompt')}: ${translate("Failed")}';
      });
      return;
    }

    await _closeWindow();
  }

  ButtonStyle _actionButtonStyle(BuildContext context, {bool outline = false}) {
    final radius = BorderRadius.circular(12);
    if (outline) {
      return OutlinedButton.styleFrom(
        minimumSize: const Size(104, 42),
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
        shape: RoundedRectangleBorder(borderRadius: radius),
      );
    }
    return ElevatedButton.styleFrom(
      elevation: 0,
      minimumSize: const Size(104, 42),
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
      shape: RoundedRectangleBorder(borderRadius: radius),
    );
  }

  @override
  Widget build(BuildContext context) {
    final body = Scaffold(
      backgroundColor: Theme.of(context).colorScheme.background,
      body: Column(
        children: [
          GestureDetector(
            behavior: HitTestBehavior.opaque,
            onPanStart: (_) => _startWindowDrag(),
            child: Container(
              height: _passwordWindowHeaderHeight,
              padding: isMacOS
                  ? EdgeInsets.zero
                  : const EdgeInsets.symmetric(horizontal: 16),
              decoration: BoxDecoration(
                color: Theme.of(context).dialogTheme.backgroundColor ??
                    Theme.of(context).cardColor,
                border: Border(
                  bottom: BorderSide(color: Theme.of(context).dividerColor),
                ),
              ),
              child: Row(
                children: [
                  if (isMacOS) ...[
                    const SizedBox(width: _kMacOSWindowControlsInset),
                    Expanded(
                      child: Center(
                        child: FittedBox(
                          fit: BoxFit.scaleDown,
                          child: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              const Icon(Icons.key, color: MyTheme.accent),
                              const SizedBox(width: 12),
                              Text(
                                translate('Set Password'),
                                style: const TextStyle(
                                  fontSize: 18,
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                    const SizedBox(width: _kMacOSWindowControlsInset),
                  ] else ...[
                    const Icon(Icons.key, color: MyTheme.accent),
                    const SizedBox(width: 12),
                    Expanded(
                      child: Text(
                        translate('Set Password'),
                        style: const TextStyle(
                          fontSize: 18,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ),
                    IconButton(
                      splashRadius: 18,
                      onPressed: _closeWindow,
                      icon: const Icon(Icons.close_rounded),
                    ),
                  ],
                ],
              ),
            ),
          ),
          Expanded(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(20, 18, 20, 20),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  TextField(
                    controller: _passwordController,
                    obscureText: true,
                    autofocus: true,
                    maxLength: _maxLength,
                    decoration: InputDecoration(
                      labelText: translate('Password'),
                      errorText:
                          _passwordError.isEmpty ? null : _passwordError,
                      counterText: '',
                    ),
                    onChanged: (value) {
                      _password.value = value.trim();
                      setState(() {
                        _passwordError = '';
                      });
                    },
                  ).workaroundFreezeLinuxMint(),
                  const SizedBox(height: 6),
                  PasswordStrengthIndicator(password: _password),
                  const SizedBox(height: 14),
                  TextField(
                    controller: _confirmationController,
                    obscureText: true,
                    maxLength: _maxLength,
                    decoration: InputDecoration(
                      labelText: translate('Confirmation'),
                      errorText: _confirmationError.isEmpty
                          ? null
                          : _confirmationError,
                      counterText: '',
                    ),
                    onChanged: (_) {
                      setState(() {
                        _confirmationError = '';
                      });
                    },
                  ).workaroundFreezeLinuxMint(),
                  const Spacer(),
                  Padding(
                    padding: const EdgeInsets.only(top: 16, bottom: 6),
                    child: Align(
                      alignment: Alignment.centerRight,
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          OutlinedButton.icon(
                            onPressed: _closeWindow,
                            style: _actionButtonStyle(context, outline: true),
                            icon: const Icon(Icons.close_rounded),
                            label: Text(translate('Cancel')),
                          ),
                          const SizedBox(width: 10),
                          ElevatedButton.icon(
                            onPressed: _canSubmit ? _submit : null,
                            style: _actionButtonStyle(context),
                            icon: const Icon(Icons.done_rounded),
                            label: Text(translate('OK')),
                          ),
                        ],
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );

    return workaroundWindowBorder(context, body);
  }
}
