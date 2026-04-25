import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/widgets/audio_input.dart';
import 'package:flutter_hbb/common/widgets/setting_widgets.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/desktop/pages/desktop_home_page.dart';
import 'package:flutter_hbb/desktop/pages/settings_dialog_utils.dart';
import 'package:flutter_hbb/desktop/widgets/remote_toolbar.dart';
import 'package:flutter_hbb/desktop/widgets/update_progress.dart';
import 'package:flutter_hbb/mobile/widgets/dialog.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/models/printer_model.dart';
import 'package:flutter_hbb/models/server_model.dart';
import 'package:flutter_hbb/models/state_model.dart';
import 'package:flutter_hbb/plugin/manager.dart';
import 'package:flutter_hbb/plugin/widgets/desktop_settings.dart';
import 'package:get/get.dart';
import 'package:provider/provider.dart';
import 'package:url_launcher/url_launcher_string.dart';

import '../../common/widgets/dialog.dart';
import '../../common/widgets/login.dart';

const double _kSettingsDialogMaxWidth = 820;
const double _kSettingsDialogMaxHeight = 720;
const double _kCardFixedWidth = 760;
const double _kCardLeftMargin = 0;
const double _kContentHMargin = 15;
const double _kContentHSubMargin = _kContentHMargin + 33;
const double _kCheckBoxLeftMargin = 10;
const double _kRadioLeftMargin = 10;
const double _kListViewBottomMargin = 15;
const double _kTitleFontSize = 20;
const double _kContentFontSize = 15;
const Color _accentColor = MyTheme.accent;
const String _kDesktopSettingsDialogTag = 'desktop-settings-dialog';
const double _kSectionSpacing = 8;
const double _kSettingsDialogHeaderHeight = 72;
const double _kSettingsDialogChromeHeight = _kSettingsDialogHeaderHeight;

_DesktopSettingPageState? _activeSettingsPageState;

Future<void> _openPathInSystemFileManager(String path) async {
  try {
    if (Platform.isWindows) {
      await Process.start('explorer.exe', [path],
          mode: ProcessStartMode.detached);
      return;
    }
    if (Platform.isMacOS) {
      await Process.start('open', [path], mode: ProcessStartMode.detached);
      return;
    }
    if (Platform.isLinux) {
      await Process.start('xdg-open', [path],
          mode: ProcessStartMode.detached);
    }
  } catch (e) {
    debugPrint('Failed to open directory $path: $e');
  }
}

enum SettingsTabKey {
  general,
  safety,
  network,
  display,
  plugin,
  printer,
  about,
}

class DesktopSettingPage extends StatefulWidget {
  final SettingsTabKey initialTabkey;
  static final List<SettingsTabKey> tabKeys = [
    SettingsTabKey.general,
    if (!isWeb &&
        !bind.isOutgoingOnly() &&
        !bind.isDisableSettings() &&
        bind.mainGetBuildinOption(key: kOptionHideSecuritySetting) != 'Y')
      SettingsTabKey.safety,
    // hdesk: network tab hidden, server is pre-configured

    if (!bind.isIncomingOnly()) SettingsTabKey.display,
    if (!isWeb && !bind.isIncomingOnly() && bind.pluginFeatureIsEnabled())
      SettingsTabKey.plugin,
    // hdesk: printer tab removed
    SettingsTabKey.about,
  ];

  DesktopSettingPage({Key? key, required this.initialTabkey}) : super(key: key);

  static SettingsTabKey _normalizeInitialTab(SettingsTabKey page) {
    return resolveVisibleSelection<SettingsTabKey>(
          page,
          tabKeys,
          onFallback: (requested, fallback) {
            debugPrint(
              'Requested settings section $requested is hidden; falling back to $fallback.',
            );
          },
        ) ??
        SettingsTabKey.general;
  }

  static void show({SettingsTabKey initialTabkey = SettingsTabKey.general}) {
    try {
      if (tabKeys.isEmpty) {
        return;
      }
      final resolvedInitialTab = _normalizeInitialTab(initialTabkey);
      final dialogManager = gFFI.dialogManager;
      if (dialogManager.existing(_kDesktopSettingsDialogTag) &&
          _activeSettingsPageState != null) {
        _activeSettingsPageState!.scrollToSection(resolvedInitialTab);
        return;
      }
      if (dialogManager.existing(_kDesktopSettingsDialogTag)) {
        dialogManager.dismissByTag(_kDesktopSettingsDialogTag);
      }
      dialogManager.show(
        tag: _kDesktopSettingsDialogTag,
        clickMaskDismiss: true,
        backDismiss: true,
        (setState, close, context) {
          final dialogSize = computeSettingsDialogSize(
            MediaQuery.of(context).size,
            preferredWidth: _kSettingsDialogMaxWidth,
            preferredHeight: _kSettingsDialogMaxHeight,
          );
          final dialogContentHeight = dialogSize.height >
                  _kSettingsDialogChromeHeight
              ? dialogSize.height - _kSettingsDialogChromeHeight
              : dialogSize.height;
          return CustomAlertDialog(
            scrollable: false,
            titlePadding: EdgeInsets.zero,
            contentPadding: 0,
            title: Container(
              height: _kSettingsDialogHeaderHeight,
              padding: const EdgeInsets.symmetric(horizontal: 16),
              decoration: BoxDecoration(
                color: Theme.of(context).dialogTheme.backgroundColor ??
                    Theme.of(context).cardColor,
                border: Border(
                  bottom: BorderSide(color: Theme.of(context).dividerColor),
                ),
              ),
              child: Row(
                children: [
                  const SizedBox(width: 48, height: 48),
                  Expanded(
                    child: Center(
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          const Icon(Icons.settings_rounded,
                              color: _accentColor),
                          const SizedBox(width: 12),
                          Text(
                              translate('Settings'),
                            style: const TextStyle(fontSize: _kTitleFontSize),
                          ),
                        ],
                      ),
                    ),
                  ),
                  IconButton(
                    splashRadius: 18,
                    onPressed: () => close(),
                    icon: const Icon(Icons.close_rounded),
                  ),
                ],
              ),
            ),
            contentBoxConstraints: BoxConstraints(maxWidth: dialogSize.width),
            content: SizedBox(
              width: dialogSize.width,
              height: dialogContentHeight,
              child: DesktopSettingPage(initialTabkey: resolvedInitialTab),
            ),
            onCancel: close,
          );
        },
      );
    } catch (e) {
      debugPrintStack(label: '$e');
    }
  }

  @override
  State<DesktopSettingPage> createState() =>
      _DesktopSettingPageState();

  static void switch2page(SettingsTabKey page) {
    show(initialTabkey: page);
  }
}

class _DesktopSettingPageState extends State<DesktopSettingPage>
    with
        TickerProviderStateMixin,
        AutomaticKeepAliveClientMixin,
        WidgetsBindingObserver {
  final ScrollController _scrollController = ScrollController();
  late final Map<SettingsTabKey, GlobalKey> _sectionKeys = {
    for (final tab in DesktopSettingPage.tabKeys) tab: GlobalKey()
  };
  SettingsTabKey? _pendingScrollTab;
  int _pendingScrollRetries = 0;

  @override
  bool get wantKeepAlive => true;

  final RxBool _block = false.obs;
  final RxBool _canBeBlocked = false.obs;
  Timer? _videoConnTimer;

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    super.didChangeAppLifecycleState(state);
    if (state == AppLifecycleState.resumed) {
      shouldBeBlocked(_block, canBeBlocked);
    }
  }

  @override
  void initState() {
    super.initState();
    _activeSettingsPageState = this;
    WidgetsBinding.instance.addObserver(this);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _scrollToSection(DesktopSettingPage._normalizeInitialTab(
          widget.initialTabkey));
    });
    _videoConnTimer =
        periodic_immediate(Duration(milliseconds: 1000), () async {
      if (!mounted) {
        return;
      }
      _canBeBlocked.value = await canBeBlocked();
    });
  }

  @override
  void dispose() {
    super.dispose();
    if (identical(_activeSettingsPageState, this)) {
      _activeSettingsPageState = null;
    }
    WidgetsBinding.instance.removeObserver(this);
    _videoConnTimer?.cancel();
    _scrollController.dispose();
  }

  void scrollToSection(SettingsTabKey tab) {
    _scrollToSection(tab, retriesLeft: 3);
  }

  Future<void> _scrollToSection(SettingsTabKey tab,
      {int retriesLeft = 0}) async {
    final targetContext = _sectionKeys[tab]?.currentContext;
    if (targetContext == null) {
      if (retriesLeft > 0) {
        _pendingScrollTab = tab;
        _pendingScrollRetries = retriesLeft - 1;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (!mounted || _pendingScrollTab != tab) {
            return;
          }
          _scrollToSection(tab, retriesLeft: _pendingScrollRetries);
        });
      } else {
        debugPrint(
          'Unable to scroll settings dialog to section $tab: context not ready.',
        );
      }
      return;
    }
    _pendingScrollTab = null;
    _pendingScrollRetries = 0;
    await Scrollable.ensureVisible(
      targetContext,
      duration: const Duration(milliseconds: 220),
      curve: Curves.easeOutCubic,
      alignment: 0.0,
    );
  }

  Widget _buildSection(SettingsTabKey tab) {
    switch (tab) {
      case SettingsTabKey.general:
        return const _General(embedded: true);
      case SettingsTabKey.safety:
        return const _Safety(embedded: true);
      case SettingsTabKey.network:
        return const _Network(embedded: true);
      case SettingsTabKey.display:
        return const _Display(embedded: true);
      case SettingsTabKey.plugin:
        return const _Plugin(embedded: true);
      case SettingsTabKey.printer:
        return const _Printer(embedded: true);
      case SettingsTabKey.about:
        return const _About(embedded: true);
    }
  }

  List<Widget> _sections() {
    return DesktopSettingPage.tabKeys
        .map(
          (tab) => Container(
            key: _sectionKeys[tab],
            margin: const EdgeInsets.only(bottom: _kSectionSpacing),
            child: _buildSection(tab),
          ),
        )
        .toList();
  }

  Widget _buildBlock({required Widget child}) {
    // check both mouseMoveTime and videoConnCount
    return Obx(() {
      final videoConnBlock =
          _canBeBlocked.value && stateGlobal.videoConnCount > 0;
      return Stack(
        children: [
          buildRemoteBlock(
            block: _block,
            mask: false,
            use: canBeBlocked,
            child: preventMouseKeyBuilder(
              child: child,
              block: videoConnBlock,
            ),
          ),
          if (videoConnBlock)
            Container(
              color: Colors.black.withOpacity(0.5),
            )
        ],
      );
    });
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return Container(
      color: Theme.of(context).colorScheme.background,
      child: _buildBlock(
        child: Scrollbar(
          controller: _scrollController,
          thumbVisibility: true,
          child: SingleChildScrollView(
            controller: _scrollController,
            padding: const EdgeInsets.only(right: 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: _sections(),
            ),
          ),
        ),
      ),
    );
  }
}

//#region pages

class _General extends StatefulWidget {
  final bool embedded;

  const _General({Key? key, this.embedded = false}) : super(key: key);

  @override
  State<_General> createState() => _GeneralState();
}

class _GeneralState extends State<_General> {
  final RxBool serviceStop =
      isWeb ? RxBool(false) : Get.find<RxBool>(tag: 'stop-service');
  RxBool serviceBtnEnabled = true.obs;

  bool _showAdvancedGeneralSettings() {
    return bind.mainGetBuildinOption(key: 'show-advanced-general-settings') ==
        'Y';
  }

  @override
  Widget build(BuildContext context) {
    final showAdvancedGeneralSettings = _showAdvancedGeneralSettings();
    final children = <Widget>[
      theme(),
      _Card(title: 'Language', children: [language()]),
      if (!isWeb) record(context),
    ];

    if (showAdvancedGeneralSettings) {
      children.insertAll(0, [
        if (!isWeb) service(),
      ]);
      children.addAll([
        if (!isWeb) hwcodec(),
        if (!isWeb) audio(context),
        if (!isWeb) WaylandCard(),
        other(),
      ]);
    }

    if (widget.embedded) {
      return Column(children: children);
    }

    final scrollController = ScrollController();

    return ListView(
      controller: scrollController,
      children: children,
    ).marginOnly(bottom: _kListViewBottomMargin);
  }

  Widget theme() {
    final current = MyTheme.getThemeModePreference().toShortString();
    onChanged(String value) async {
      await MyTheme.changeDarkMode(MyTheme.themeModeFromString(value));
      setState(() {});
    }

    final isOptFixed = isOptionFixed(kCommConfKeyTheme);
    return _Card(title: 'Theme', children: [
      _Radio<String>(context,
          value: 'light',
          groupValue: current,
          label: 'Light',
          onChanged: isOptFixed ? null : onChanged),
      _Radio<String>(context,
          value: 'dark',
          groupValue: current,
          label: 'Dark',
          onChanged: isOptFixed ? null : onChanged),
      _Radio<String>(context,
          value: 'system',
          groupValue: current,
          label: 'Follow System',
          onChanged: isOptFixed ? null : onChanged),
    ]);
  }

  Widget service() {
    if (bind.isOutgoingOnly()) {
      return const Offstage();
    }

    final hideStopService =
        bind.mainGetBuildinOption(key: kOptionHideStopService) == 'Y';

    return Obx(() {
      if (hideStopService && !serviceStop.value) {
        return const Offstage();
      }

      return _Card(title: 'Service', children: [
        _Button(serviceStop.value ? 'Start' : 'Stop', () {
          () async {
            serviceBtnEnabled.value = false;
            await start_service(serviceStop.value);
            // enable the button after 1 second
            Future.delayed(const Duration(seconds: 1), () {
              serviceBtnEnabled.value = true;
            });
          }();
        }, enabled: serviceBtnEnabled.value)
      ]);
    });
  }

  Widget other() {
    final showAutoUpdate = isWindows && bind.mainIsInstalled();
    final children = <Widget>[
      if (!isWeb && !bind.isIncomingOnly())
        _OptionCheckBox(context, 'Confirm before closing multiple tabs',
            kOptionEnableConfirmClosingTabs,
            isServer: false),
      _OptionCheckBox(context, 'Adaptive bitrate', kOptionEnableAbr),
      if (!isWeb) wallpaper(),
      if (!isWeb && !bind.isIncomingOnly()) ...[
        _OptionCheckBox(
          context,
          'Open connection in new tab',
          kOptionOpenNewConnInTabs,
          isServer: false,
        ),
        // though this is related to GUI, but opengl problem affects all users, so put in config rather than local
        if (isLinux)
          Tooltip(
            message: translate('software_render_tip'),
            child: _OptionCheckBox(
              context,
              "Always use software rendering",
              kOptionAllowAlwaysSoftwareRender,
            ),
          ),
        if (!isWeb)
          Tooltip(
            message: translate('texture_render_tip'),
            child: _OptionCheckBox(
              context,
              "Use texture rendering",
              kOptionTextureRender,
              optGetter: bind.mainGetUseTextureRender,
              optSetter: (k, v) async =>
                  await bind.mainSetLocalOption(key: k, value: v ? 'Y' : 'N'),
            ),
          ),
        if (isWindows)
          Tooltip(
            message: translate('d3d_render_tip'),
            child: _OptionCheckBox(
              context,
              "Use D3D rendering",
              kOptionD3DRender,
              isServer: false,
            ),
          ),
        if (!isWeb && !bind.isCustomClient())
          _OptionCheckBox(
            context,
            'Check for software update on startup',
            kOptionEnableCheckUpdate,
            isServer: false,
          ),
        if (showAutoUpdate)
          _OptionCheckBox(
            context,
            'Auto update',
            kOptionAllowAutoUpdate,
            isServer: true,
          ),
        if (isWindows && !bind.isOutgoingOnly())
          _OptionCheckBox(
            context,
            'Capture screen using DirectX',
            kOptionDirectxCapture,
          ),
        if (!bind.isIncomingOnly()) ...[
          _OptionCheckBox(
            context,
            'Enable UDP hole punching',
            kOptionEnableUdpPunch,
            isServer: false,
          ),
          _OptionCheckBox(
            context,
            'Enable IPv6 P2P connection',
            kOptionEnableIpv6Punch,
            isServer: false,
          ),
        ],
      ],
    ];

    // Add client-side wakelock option for desktop platforms
    if (!bind.isIncomingOnly()) {
      children.add(_OptionCheckBox(
        context,
        'keep-awake-during-outgoing-sessions-label',
        kOptionKeepAwakeDuringOutgoingSessions,
        isServer: false,
      ));
    }

    if (!isWeb && bind.mainShowOption(key: kOptionAllowLinuxHeadless)) {
      children.add(_OptionCheckBox(
          context, 'Allow linux headless', kOptionAllowLinuxHeadless));
    }
    if (!bind.isDisableAccount()) {
      children.add(_OptionCheckBox(
        context,
        'note-at-conn-end-tip',
        kOptionAllowAskForNoteAtEndOfConnection,
        isServer: false,
        optSetter: (key, value) async {
          if (value && !gFFI.userModel.isLogin) {
            final res = await loginDialog();
            if (res != true) return;
          }
          await mainSetLocalBoolOption(key, value);
        },
      ));
    }
    return _Card(title: 'Other', children: children);
  }

  Widget wallpaper() {
    if (bind.isOutgoingOnly()) {
      return const Offstage();
    }

    return futureBuilder(future: () async {
      final support = await bind.mainSupportRemoveWallpaper();
      return support;
    }(), hasData: (data) {
      if (data is bool && data == true) {
        bool value = mainGetBoolOptionSync(kOptionAllowRemoveWallpaper);
        return Row(
          children: [
            Flexible(
              child: _OptionCheckBox(
                context,
                'Remove wallpaper during incoming sessions',
                kOptionAllowRemoveWallpaper,
                update: (bool v) {
                  setState(() {});
                },
              ),
            ),
            if (value)
              _CountDownButton(
                text: 'Test',
                second: 5,
                onPressed: () {
                  bind.mainTestWallpaper(second: 5);
                },
              )
          ],
        );
      }

      return Offstage();
    });
  }

  Widget hwcodec() {
    final hwcodec = bind.mainHasHwcodec();
    final vram = bind.mainHasVram();
    return Offstage(
      offstage: !(hwcodec || vram),
      child: _Card(title: 'Hardware Codec', children: [
        _OptionCheckBox(
          context,
          'Enable hardware codec',
          kOptionEnableHwcodec,
          update: (bool v) {
            if (v) {
              bind.mainCheckHwcodec();
            }
          },
        )
      ]),
    );
  }

  Widget audio(BuildContext context) {
    if (bind.isOutgoingOnly()) {
      return const Offstage();
    }

    builder(devices, currentDevice, setDevice) {
      final child = ComboBox(
        keys: devices,
        values: devices,
        initialKey: currentDevice,
        onChanged: (key) async {
          setDevice(key);
          setState(() {});
        },
      ).marginOnly(left: _kContentHMargin);
      return _Card(title: 'Audio Input Device', children: [child]);
    }

    return AudioInput(builder: builder, isCm: false, isVoiceCall: false);
  }

  Widget record(BuildContext context) {
    final showRootDir = isWindows && bind.mainIsInstalled();
    return futureBuilder(future: () async {
      String user_dir = bind.mainVideoSaveDirectory(root: false);
      String root_dir =
          showRootDir ? bind.mainVideoSaveDirectory(root: true) : '';
      bool user_dir_exists = await Directory(user_dir).exists();
      bool root_dir_exists =
          showRootDir ? await Directory(root_dir).exists() : false;
      return {
        'user_dir': user_dir,
        'root_dir': root_dir,
        'user_dir_exists': user_dir_exists,
        'root_dir_exists': root_dir_exists,
      };
    }(), hasData: (data) {
      Map<String, dynamic> map = data as Map<String, dynamic>;
      String user_dir = map['user_dir']!;
      String root_dir = map['root_dir']!;
      bool root_dir_exists = map['root_dir_exists']!;
      bool user_dir_exists = map['user_dir_exists']!;
      return _Card(title: 'Recording', children: [
        if (!bind.isOutgoingOnly())
          _OptionCheckBox(context, 'Automatically record incoming sessions',
              kOptionAllowAutoRecordIncoming),
        if (!bind.isIncomingOnly())
          _OptionCheckBox(context, 'Automatically record outgoing sessions',
              kOptionAllowAutoRecordOutgoing,
              isServer: false),
        if (showRootDir && !bind.isOutgoingOnly())
          Row(
            children: [
              Text(
                  '${translate(bind.isIncomingOnly() ? "Directory" : "Incoming")}:'),
              Expanded(
                child: GestureDetector(
                    onTap: root_dir_exists
                    ? () => unawaited(
                      _openPathInSystemFileManager(root_dir))
                        : null,
                    child: Text(
                      root_dir,
                      softWrap: true,
                      style: root_dir_exists
                          ? const TextStyle(
                              decoration: TextDecoration.underline)
                          : null,
                    )).marginOnly(left: 10),
              ),
            ],
          ).marginOnly(left: _kContentHMargin),
        if (!(showRootDir && bind.isIncomingOnly()))
          Row(
            children: [
              Text(
                  '${translate((showRootDir && !bind.isOutgoingOnly()) ? "Outgoing" : "Directory")}:'),
              Expanded(
                child: GestureDetector(
                    onTap: user_dir_exists
                    ? () => unawaited(
                      _openPathInSystemFileManager(user_dir))
                        : null,
                    child: Text(
                      user_dir,
                      softWrap: true,
                      style: user_dir_exists
                          ? const TextStyle(
                              decoration: TextDecoration.underline)
                          : null,
                    )).marginOnly(left: 10),
              ),
              ElevatedButton(
                      onPressed: isOptionFixed(kOptionVideoSaveDirectory)
                          ? null
                          : () async {
                              String? initialDirectory;
                              if (await Directory.fromUri(
                                      Uri.directory(user_dir))
                                  .exists()) {
                                initialDirectory = user_dir;
                              }
                              String? selectedDirectory =
                                  await FilePicker.platform.getDirectoryPath(
                                      initialDirectory: initialDirectory);
                              if (selectedDirectory != null) {
                                await bind.mainSetLocalOption(
                                    key: kOptionVideoSaveDirectory,
                                    value: selectedDirectory);
                                setState(() {});
                              }
                            },
                      child: Text(translate('Change')))
                  .marginOnly(left: 5),
            ],
          ).marginOnly(left: _kContentHMargin),
      ]);
    });
  }

  Widget language() {
    return futureBuilder(future: () async {
      String langs = await bind.mainGetLangs();
      return {'langs': langs};
    }(), hasData: (res) {
      Map<String, String> data = res as Map<String, String>;
      List<dynamic> langsList = jsonDecode(data['langs']!);
      Map<String, String> langsMap = {for (var v in langsList) v[0]: v[1]};
      List<String> keys = langsMap.keys.toList();
      List<String> values = langsMap.values.toList();
      keys.insert(0, defaultOptionLang);
      values.insert(0, translate('Default'));
      String currentKey = bind.mainGetLocalOption(key: kCommConfKeyLang);
      if (!keys.contains(currentKey)) {
        currentKey = defaultOptionLang;
      }
      final isOptFixed = isOptionFixed(kCommConfKeyLang);
      return ComboBox(
        keys: keys,
        values: values,
        initialKey: currentKey,
        onChanged: (key) async {
          await bind.mainSetLocalOption(key: kCommConfKeyLang, value: key);
          if (isWeb) reloadCurrentWindow();
          if (!isWeb) reloadAllWindows();
          if (!isWeb) bind.mainChangeLanguage(lang: key);
        },
        enabled: !isOptFixed,
      ).marginOnly(left: _kContentHMargin);
    });
  }
}

enum _AccessMode {
  custom,
  full,
  view,
}

class _Safety extends StatefulWidget {
  final bool embedded;

  const _Safety({Key? key, this.embedded = false}) : super(key: key);

  @override
  State<_Safety> createState() => _SafetyState();
}

class _SafetyState extends State<_Safety> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;
  bool locked = bind.mainIsInstalled();
  ScrollController? _scrollController;

  ScrollController get _effectiveScrollController =>
      _scrollController ??= ScrollController();

  @override
  void dispose() {
    _scrollController?.dispose();
    super.dispose();
  }

  bool _showAdvancedSecuritySettings() {
    return bind.mainGetBuildinOption(key: 'show-advanced-security-settings') ==
        'Y';
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final showAdvancedSecuritySettings = _showAdvancedSecuritySettings();
    final content = Column(
      children: [
        _lock(locked, 'Unlock Security Settings', () {
          locked = false;
          setState(() => {});
        }),
        preventMouseKeyBuilder(
          block: locked,
          child: Column(children: [
            if (showAdvancedSecuritySettings) permissions(context),
            password(context),
            if (showAdvancedSecuritySettings)
              _Card(title: '2FA', children: [tfa()]),
            if (showAdvancedSecuritySettings && !isChangeIdDisabled())
              _Card(title: 'ID', children: [changeId()]),
            if (showAdvancedSecuritySettings) more(context),
          ]),
        ),
      ],
    );

    if (widget.embedded) {
      return content;
    }

    return SingleChildScrollView(
      controller: _effectiveScrollController,
      child: content,
    ).marginOnly(bottom: _kListViewBottomMargin);
  }

  Widget tfa() {
    bool enabled = !locked;
    // Simple temp wrapper for PR check
    tmpWrapper() {
      RxBool has2fa = bind.mainHasValid2FaSync().obs;
      RxBool hasBot = bind.mainHasValidBotSync().obs;
      update() async {
        has2fa.value = bind.mainHasValid2FaSync();
        setState(() {});
      }

      onChanged(bool? checked) async {
        if (checked == false) {
          CommonConfirmDialog(
              gFFI.dialogManager, translate('cancel-2fa-confirm-tip'), () {
            change2fa(callback: update);
          });
        } else {
          change2fa(callback: update);
        }
      }

      final tfa = GestureDetector(
        child: InkWell(
          child: Obx(() => Row(
                children: [
                  Checkbox(
                          value: has2fa.value,
                          onChanged: enabled ? onChanged : null)
                      .marginOnly(right: 5),
                  Expanded(
                      child: Text(
                    translate('enable-2fa-title'),
                    style:
                        TextStyle(color: disabledTextColor(context, enabled)),
                  ))
                ],
              )),
        ),
        onTap: () {
          onChanged(!has2fa.value);
        },
      ).marginOnly(left: _kCheckBoxLeftMargin);
      if (!has2fa.value) {
        return tfa;
      }
      updateBot() async {
        hasBot.value = bind.mainHasValidBotSync();
        setState(() {});
      }

      onChangedBot(bool? checked) async {
        if (checked == false) {
          CommonConfirmDialog(
              gFFI.dialogManager, translate('cancel-bot-confirm-tip'), () {
            changeBot(callback: updateBot);
          });
        } else {
          changeBot(callback: updateBot);
        }
      }

      final bot = GestureDetector(
        child: Tooltip(
          waitDuration: Duration(milliseconds: 300),
          message: translate("enable-bot-tip"),
          child: InkWell(
              child: Obx(() => Row(
                    children: [
                      Checkbox(
                              value: hasBot.value,
                              onChanged: enabled ? onChangedBot : null)
                          .marginOnly(right: 5),
                      Expanded(
                          child: Text(
                        translate('Telegram bot'),
                        style: TextStyle(
                            color: disabledTextColor(context, enabled)),
                      ))
                    ],
                  ))),
        ),
        onTap: () {
          onChangedBot(!hasBot.value);
        },
      ).marginOnly(left: _kCheckBoxLeftMargin + 30);

      final trust = Row(
        children: [
          Flexible(
            child: Tooltip(
              waitDuration: Duration(milliseconds: 300),
              message: translate("enable-trusted-devices-tip"),
              child: _OptionCheckBox(context, "Enable trusted devices",
                  kOptionEnableTrustedDevices,
                  enabled: !locked, update: (v) {
                setState(() {});
              }),
            ),
          ),
          if (mainGetBoolOptionSync(kOptionEnableTrustedDevices))
            ElevatedButton(
                onPressed: locked
                    ? null
                    : () {
                        manageTrustedDeviceDialog();
                      },
                child: Text(translate('Manage trusted devices')))
        ],
      ).marginOnly(left: 30);

      return Column(
        children: [tfa, bot, trust],
      );
    }

    return tmpWrapper();
  }

  Widget changeId() {
    return ChangeNotifierProvider.value(
        value: gFFI.serverModel,
        child: Consumer<ServerModel>(builder: ((context, model, child) {
          return _Button('Change ID', changeIdDialog,
              enabled: !locked && model.connectStatus > 0);
        })));
  }

  Widget permissions(context) {
    bool enabled = !locked;
    // Simple temp wrapper for PR check
    tmpWrapper() {
      String accessMode = bind.mainGetOptionSync(key: kOptionAccessMode);
      _AccessMode mode;
      if (accessMode == 'full') {
        mode = _AccessMode.full;
      } else if (accessMode == 'view') {
        mode = _AccessMode.view;
      } else {
        mode = _AccessMode.custom;
      }
      String initialKey;
      bool? fakeValue;
      switch (mode) {
        case _AccessMode.custom:
          initialKey = '';
          fakeValue = null;
          break;
        case _AccessMode.full:
          initialKey = 'full';
          fakeValue = true;
          break;
        case _AccessMode.view:
          initialKey = 'view';
          fakeValue = false;
          break;
      }

      return _Card(title: 'Permissions', children: [
        ComboBox(
            keys: [
              defaultOptionAccessMode,
              'full',
              'view',
            ],
            values: [
              translate('Custom'),
              translate('Full Access'),
              translate('Screen Share'),
            ],
            enabled: enabled && !isOptionFixed(kOptionAccessMode),
            initialKey: initialKey,
            onChanged: (mode) async {
              await bind.mainSetOption(key: kOptionAccessMode, value: mode);
              setState(() {});
            }).marginOnly(left: _kContentHMargin),
        Column(
          children: [
            _OptionCheckBox(
                context, 'Enable keyboard/mouse', kOptionEnableKeyboard,
                enabled: enabled, fakeValue: fakeValue),
            if (isWindows)
              _OptionCheckBox(
                  context, 'Enable remote printer', kOptionEnableRemotePrinter,
                  enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(context, 'Enable clipboard', kOptionEnableClipboard,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(
                context, 'Enable file transfer', kOptionEnableFileTransfer,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(context, 'Enable audio', kOptionEnableAudio,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(context, 'Enable camera', kOptionEnableCamera,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(context, 'Enable terminal', kOptionEnableTerminal,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(
                context, 'Enable TCP tunneling', kOptionEnableTunnel,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(
                context, 'Enable remote restart', kOptionEnableRemoteRestart,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(
                context, 'Enable recording session', kOptionEnableRecordSession,
                enabled: enabled, fakeValue: fakeValue),
            if (isWindows)
              _OptionCheckBox(context, 'Enable blocking user input',
                  kOptionEnableBlockInput,
                  enabled: enabled, fakeValue: fakeValue),
            if (_showAdvancedSecuritySettings())
              _OptionCheckBox(
                  context,
                  'Enable remote configuration modification',
                  kOptionAllowRemoteConfigModification,
                  enabled: enabled,
                  fakeValue: fakeValue),
          ],
        ),
      ]);
    }

    return tmpWrapper();
  }

  Widget password(BuildContext context) {
    return _Card(title: 'Password', children: [
      _SubButton(
        'Set permanent password',
        setPasswordDialog,
        !locked && !isChangePermanentPasswordDisabled(),
      )
    ]);
  }

  Widget more(BuildContext context) {
    bool enabled = !locked;
    return _Card(title: 'Security', children: [
      shareRdp(context, enabled),
      _OptionCheckBox(context, 'Deny LAN discovery', 'enable-lan-discovery',
          reverse: true, enabled: enabled),
      ...directIp(context),
      whitelist(),
      ...autoDisconnect(context),
      _OptionCheckBox(context, 'keep-awake-during-incoming-sessions-label',
          kOptionKeepAwakeDuringIncomingSessions,
          reverse: false, enabled: enabled),
      if (bind.mainIsInstalled())
        _OptionCheckBox(context, 'allow-only-conn-window-open-tip',
            'allow-only-conn-window-open',
            reverse: false, enabled: enabled),
      if (bind.mainIsInstalled() && !isUnlockPinDisabled()) unlockPin()
    ]);
  }

  shareRdp(BuildContext context, bool enabled) {
    onChanged(bool b) async {
      await bind.mainSetShareRdp(enable: b);
      setState(() {});
    }

    bool value = bind.mainIsShareRdp();
    return Offstage(
      offstage: !(isWindows && bind.mainIsInstalled()),
      child: GestureDetector(
          child: Row(
            children: [
              Checkbox(
                      value: value,
                      onChanged: enabled ? (_) => onChanged(!value) : null)
                  .marginOnly(right: 5),
              Expanded(
                child: Text(translate('Enable RDP session sharing'),
                    style:
                        TextStyle(color: disabledTextColor(context, enabled))),
              )
            ],
          ).marginOnly(left: _kCheckBoxLeftMargin),
          onTap: enabled ? () => onChanged(!value) : null),
    );
  }

  List<Widget> directIp(BuildContext context) {
    TextEditingController controller = TextEditingController();
    update(bool v) => setState(() {});
    RxBool applyEnabled = false.obs;
    return [
      _OptionCheckBox(context, 'Enable direct IP access', kOptionDirectServer,
          update: update, enabled: !locked),
      () {
        // Simple temp wrapper for PR check
        tmpWrapper() {
          bool enabled = option2bool(kOptionDirectServer,
              bind.mainGetOptionSync(key: kOptionDirectServer));
          if (!enabled) applyEnabled.value = false;
          controller.text =
              bind.mainGetOptionSync(key: kOptionDirectAccessPort);
          final isOptFixed = isOptionFixed(kOptionDirectAccessPort);
          return Offstage(
            offstage: !enabled,
            child: _SubLabeledWidget(
              context,
              'Port',
              Row(children: [
                SizedBox(
                  width: 95,
                  child: TextField(
                    controller: controller,
                    enabled: enabled && !locked && !isOptFixed,
                    onChanged: (_) => applyEnabled.value = true,
                    inputFormatters: [
                      FilteringTextInputFormatter.allow(RegExp(
                          r'^([0-9]|[1-9]\d|[1-9]\d{2}|[1-9]\d{3}|[1-5]\d{4}|6[0-4]\d{3}|65[0-4]\d{2}|655[0-2]\d|6553[0-5])$')),
                    ],
                    decoration: const InputDecoration(
                      hintText: '21118',
                      contentPadding:
                          EdgeInsets.symmetric(vertical: 12, horizontal: 12),
                    ),
                  ).workaroundFreezeLinuxMint().marginOnly(right: 15),
                ),
                Obx(() => ElevatedButton(
                      onPressed: applyEnabled.value &&
                              enabled &&
                              !locked &&
                              !isOptFixed
                          ? () async {
                              applyEnabled.value = false;
                              await bind.mainSetOption(
                                  key: kOptionDirectAccessPort,
                                  value: controller.text);
                            }
                          : null,
                      child: Text(
                        translate('Apply'),
                      ),
                    ))
              ]),
              enabled: enabled && !locked && !isOptFixed,
            ),
          );
        }

        return tmpWrapper();
      }(),
    ];
  }

  Widget whitelist() {
    bool enabled = !locked;
    // Simple temp wrapper for PR check
    tmpWrapper() {
      RxBool hasWhitelist = whitelistNotEmpty().obs;
      update() async {
        hasWhitelist.value = whitelistNotEmpty();
      }

      onChanged(bool? checked) async {
        changeWhiteList(callback: update);
      }

      final isOptFixed = isOptionFixed(kOptionWhitelist);
      return GestureDetector(
        child: Tooltip(
          message: translate('whitelist_tip'),
          child: Obx(() => Row(
                children: [
                  Checkbox(
                          value: hasWhitelist.value,
                          onChanged: enabled && !isOptFixed ? onChanged : null)
                      .marginOnly(right: 5),
                  Offstage(
                    offstage: !hasWhitelist.value,
                    child: MouseRegion(
                      child: const Icon(Icons.warning_amber_rounded,
                              color: Color.fromARGB(255, 255, 204, 0))
                          .marginOnly(right: 5),
                      cursor: SystemMouseCursors.click,
                    ),
                  ),
                  Expanded(
                      child: Text(
                    translate('Use IP Whitelisting'),
                    style:
                        TextStyle(color: disabledTextColor(context, enabled)),
                  ))
                ],
              )),
        ),
        onTap: enabled
            ? () {
                onChanged(!hasWhitelist.value);
              }
            : null,
      ).marginOnly(left: _kCheckBoxLeftMargin);
    }

    return tmpWrapper();
  }

  Widget hide_cm(bool enabled) {
    return ChangeNotifierProvider.value(
        value: gFFI.serverModel,
        child: Consumer<ServerModel>(builder: (context, model, child) {
          final enableHideCm = model.approveMode == 'password' &&
              model.verificationMethod == kUsePermanentPassword;
          onHideCmChanged(bool? b) {
            if (b != null) {
              bind.mainSetOption(
                  key: 'allow-hide-cm', value: bool2option('allow-hide-cm', b));
            }
          }

          return Tooltip(
              message: enableHideCm ? "" : translate('hide_cm_tip'),
              child: GestureDetector(
                onTap:
                    enableHideCm ? () => onHideCmChanged(!model.hideCm) : null,
                child: Row(
                  children: [
                    Checkbox(
                            value: model.hideCm,
                            onChanged: enabled && enableHideCm
                                ? onHideCmChanged
                                : null)
                        .marginOnly(right: 5),
                    Expanded(
                      child: Text(
                        translate('Hide connection management window'),
                        style: TextStyle(
                            color: disabledTextColor(
                                context, enabled && enableHideCm)),
                      ),
                    ),
                  ],
                ),
              ));
        }));
  }

  List<Widget> autoDisconnect(BuildContext context) {
    TextEditingController controller = TextEditingController();
    update(bool v) => setState(() {});
    RxBool applyEnabled = false.obs;
    return [
      _OptionCheckBox(
          context, 'auto_disconnect_option_tip', kOptionAllowAutoDisconnect,
          update: update, enabled: !locked),
      () {
        bool enabled = option2bool(kOptionAllowAutoDisconnect,
            bind.mainGetOptionSync(key: kOptionAllowAutoDisconnect));
        if (!enabled) applyEnabled.value = false;
        controller.text =
            bind.mainGetOptionSync(key: kOptionAutoDisconnectTimeout);
        final isOptFixed = isOptionFixed(kOptionAutoDisconnectTimeout);
        return Offstage(
          offstage: !enabled,
          child: _SubLabeledWidget(
            context,
            'Timeout in minutes',
            Row(children: [
              SizedBox(
                width: 95,
                child: TextField(
                  controller: controller,
                  enabled: enabled && !locked && !isOptFixed,
                  onChanged: (_) => applyEnabled.value = true,
                  inputFormatters: [
                    FilteringTextInputFormatter.allow(RegExp(
                        r'^([0-9]|[1-9]\d|[1-9]\d{2}|[1-9]\d{3}|[1-5]\d{4}|6[0-4]\d{3}|65[0-4]\d{2}|655[0-2]\d|6553[0-5])$')),
                  ],
                  decoration: const InputDecoration(
                    hintText: '10',
                    contentPadding:
                        EdgeInsets.symmetric(vertical: 12, horizontal: 12),
                  ),
                ).workaroundFreezeLinuxMint().marginOnly(right: 15),
              ),
              Obx(() => ElevatedButton(
                    onPressed:
                        applyEnabled.value && enabled && !locked && !isOptFixed
                            ? () async {
                                applyEnabled.value = false;
                                await bind.mainSetOption(
                                    key: kOptionAutoDisconnectTimeout,
                                    value: controller.text);
                              }
                            : null,
                    child: Text(
                      translate('Apply'),
                    ),
                  ))
            ]),
            enabled: enabled && !locked && !isOptFixed,
          ),
        );
      }(),
    ];
  }

  Widget unlockPin() {
    bool enabled = !locked;
    RxString unlockPin = bind.mainGetUnlockPin().obs;
    update() async {
      unlockPin.value = bind.mainGetUnlockPin();
    }

    onChanged(bool? checked) async {
      changeUnlockPinDialog(unlockPin.value, update);
    }

    final isOptFixed = isOptionFixed(kOptionWhitelist);
    return GestureDetector(
      child: Obx(() => Row(
            children: [
              Checkbox(
                      value: unlockPin.isNotEmpty,
                      onChanged: enabled && !isOptFixed ? onChanged : null)
                  .marginOnly(right: 5),
              Expanded(
                  child: Text(
                translate('Unlock with PIN'),
                style: TextStyle(color: disabledTextColor(context, enabled)),
              ))
            ],
          )),
      onTap: enabled
          ? () {
              onChanged(!unlockPin.isNotEmpty);
            }
          : null,
    ).marginOnly(left: _kCheckBoxLeftMargin);
  }
}

class _Network extends StatefulWidget {
  final bool embedded;

  const _Network({Key? key, this.embedded = false}) : super(key: key);

  @override
  State<_Network> createState() => _NetworkState();
}

class _NetworkState extends State<_Network> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;
  bool locked = !isWeb && bind.mainIsInstalled();

  ScrollController? _scrollController;

  ScrollController get _effectiveScrollController =>
      _scrollController ??= ScrollController();

  @override
  void dispose() {
    _scrollController?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final content = Column(
      children: [
        _lock(locked, 'Unlock Network Settings', () {
          locked = false;
          setState(() => {});
        }),
        preventMouseKeyBuilder(
          block: locked,
          child: Column(children: [
            network(context),
          ]),
        ),
      ],
    );

    if (widget.embedded) {
      return content;
    }

    return ListView(controller: _effectiveScrollController, children: [content])
        .marginOnly(bottom: _kListViewBottomMargin);
  }

  Widget network(BuildContext context) {
    final hideServer =
        bind.mainGetBuildinOption(key: kOptionHideServerSetting) == 'Y';
    final hideProxy =
        isWeb || bind.mainGetBuildinOption(key: kOptionHideProxySetting) == 'Y';
    final hideWebSocket = isWeb ||
        bind.mainGetBuildinOption(key: kOptionHideWebSocketSetting) == 'Y';

    if (hideServer && hideProxy && hideWebSocket) {
      return Offstage();
    }

    // Helper function to create network setting ListTiles
    Widget listTile({
      required IconData icon,
      required String title,
      VoidCallback? onTap,
      Widget? trailing,
      bool showTooltip = false,
      String tooltipMessage = '',
    }) {
      final titleWidget = showTooltip
          ? Row(
              children: [
                Tooltip(
                  waitDuration: Duration(milliseconds: 1000),
                  message: translate(tooltipMessage),
                  child: Row(
                    children: [
                      Text(
                        translate(title),
                        style: TextStyle(fontSize: _kContentFontSize),
                      ),
                      SizedBox(width: 5),
                      Icon(
                        Icons.help_outline,
                        size: 14,
                        color: Theme.of(context)
                            .textTheme
                            .titleLarge
                            ?.color
                            ?.withOpacity(0.7),
                      ),
                    ],
                  ),
                ),
              ],
            )
          : Text(
              translate(title),
              style: TextStyle(fontSize: _kContentFontSize),
            );

      return ListTile(
        leading: Icon(icon, color: _accentColor),
        title: titleWidget,
        enabled: !locked,
        onTap: onTap,
        trailing: trailing,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(10),
        ),
        contentPadding: EdgeInsets.symmetric(horizontal: 16),
        minLeadingWidth: 0,
        horizontalTitleGap: 10,
      );
    }

    Widget switchWidget(IconData icon, String title, String tooltipMessage,
            String optionKey) =>
        listTile(
          icon: icon,
          title: title,
          showTooltip: true,
          tooltipMessage: tooltipMessage,
          trailing: Switch(
            value: mainGetBoolOptionSync(optionKey),
            onChanged: locked || isOptionFixed(optionKey)
                ? null
                : (value) {
                    mainSetBoolOption(optionKey, value);
                    setState(() {});
                  },
          ),
        );

    final outgoingOnly = bind.isOutgoingOnly();

    final divider = const Divider(height: 1, indent: 16, endIndent: 16);
    return _Card(
      title: 'Network',
      children: [
        Container(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (!hideServer)
                listTile(
                  icon: Icons.dns_outlined,
                  title: 'ID/Relay Server',
                  onTap: () => showServerSettings(gFFI.dialogManager, setState),
                ),
              if (!hideProxy && !hideServer) divider,
              if (!hideProxy)
                listTile(
                  icon: Icons.network_ping_outlined,
                  title: 'Socks5/Http(s) Proxy',
                  onTap: changeSocks5Proxy,
                ),
              if (!hideWebSocket && (!hideServer || !hideProxy)) divider,
              if (!hideWebSocket)
                switchWidget(
                    Icons.web_asset_outlined,
                    'Use WebSocket',
                    '${translate('websocket_tip')}\n\n${translate('server-oss-not-support-tip')}',
                    kOptionAllowWebSocket),
              if (!isWeb)
                futureBuilder(
                  future: bind.mainIsUsingPublicServer(),
                  hasData: (isUsingPublicServer) {
                    if (isUsingPublicServer) {
                      return Offstage();
                    } else {
                      return Column(
                        children: [
                          if (!hideServer || !hideProxy || !hideWebSocket)
                            divider,
                          switchWidget(
                              Icons.no_encryption_outlined,
                              'Allow insecure TLS fallback',
                              'allow-insecure-tls-fallback-tip',
                              kOptionAllowInsecureTLSFallback),
                          if (!outgoingOnly) divider,
                          if (!outgoingOnly)
                            listTile(
                              icon: Icons.lan_outlined,
                              title: 'Disable UDP',
                              showTooltip: true,
                              tooltipMessage:
                                  '${translate('disable-udp-tip')}\n\n${translate('server-oss-not-support-tip')}',
                              trailing: Switch(
                                value: bind.mainGetOptionSync(
                                        key: kOptionDisableUdp) ==
                                    'Y',
                                onChanged:
                                    locked || isOptionFixed(kOptionDisableUdp)
                                        ? null
                                        : (value) async {
                                            await bind.mainSetOption(
                                                key: kOptionDisableUdp,
                                                value: value ? 'Y' : 'N');
                                            setState(() {});
                                          },
                              ),
                            ),
                        ],
                      );
                    }
                  },
                ),
            ],
          ),
        ),
      ],
    );
  }
}

class _Display extends StatefulWidget {
  final bool embedded;

  const _Display({Key? key, this.embedded = false}) : super(key: key);

  @override
  State<_Display> createState() => _DisplayState();
}

class _DisplayState extends State<_Display> {
  bool _showAdvancedDisplaySettings() {
    return bind.mainGetBuildinOption(key: 'show-advanced-display-settings') ==
        'Y';
  }

  @override
  Widget build(BuildContext context) {
    final showAdvancedDisplaySettings = _showAdvancedDisplaySettings();
    final children = <Widget>[
      imageQuality(context),
    ];

    if (showAdvancedDisplaySettings) {
      children.addAll([
        viewStyle(context),
        scrollStyle(context),
        codec(context),
        if (isDesktop) trackpadSpeed(context),
        if (!isWeb) privacyModeImpl(context),
        other(context),
      ]);
    }

    if (widget.embedded) {
      return Column(children: children);
    }

    final scrollController = ScrollController();

    return ListView(
      controller: scrollController,
      children: children,
    ).marginOnly(bottom: _kListViewBottomMargin);
  }

  Widget viewStyle(BuildContext context) {
    final isOptFixed = isOptionFixed(kOptionViewStyle);
    onChanged(String value) async {
      await bind.mainSetUserDefaultOption(key: kOptionViewStyle, value: value);
      setState(() {});
    }

    final groupValue = bind.mainGetUserDefaultOption(key: kOptionViewStyle);
    return _Card(title: 'Default View Style', children: [
      _Radio(context,
          value: kRemoteViewStyleOriginal,
          groupValue: groupValue,
          label: 'Scale original',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: kRemoteViewStyleAdaptive,
          groupValue: groupValue,
          label: 'Scale adaptive',
          onChanged: isOptFixed ? null : onChanged),
    ]);
  }

  Widget scrollStyle(BuildContext context) {
    final isOptFixed = isOptionFixed(kOptionScrollStyle);
    onChanged(String value) async {
      await bind.mainSetUserDefaultOption(
          key: kOptionScrollStyle, value: value);
      setState(() {});
    }

    final groupValue = bind.mainGetUserDefaultOption(key: kOptionScrollStyle);

    onEdgeScrollEdgeThicknessChanged(double value) async {
      await bind.mainSetUserDefaultOption(
          key: kOptionEdgeScrollEdgeThickness, value: value.round().toString());
      setState(() {});
    }

    return _Card(title: 'Default Scroll Style', children: [
      _Radio(context,
          value: kRemoteScrollStyleAuto,
          groupValue: groupValue,
          label: 'ScrollAuto',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: kRemoteScrollStyleBar,
          groupValue: groupValue,
          label: 'Scrollbar',
          onChanged: isOptFixed ? null : onChanged),
      if (!isWeb) ...[
        _Radio(context,
            value: kRemoteScrollStyleEdge,
            groupValue: groupValue,
            label: 'ScrollEdge',
            onChanged: isOptFixed ? null : onChanged),
        Offstage(
            offstage: groupValue != kRemoteScrollStyleEdge,
            child: EdgeThicknessControl(
              value: double.tryParse(bind.mainGetUserDefaultOption(
                      key: kOptionEdgeScrollEdgeThickness)) ??
                  100.0,
              onChanged: isOptionFixed(kOptionEdgeScrollEdgeThickness)
                  ? null
                  : onEdgeScrollEdgeThicknessChanged,
            )),
      ],
    ]);
  }

  Widget imageQuality(BuildContext context) {
    onChanged(String value) async {
      await bind.mainSetUserDefaultOption(
          key: kOptionImageQuality, value: value);
      setState(() {});
    }

    final isOptFixed = isOptionFixed(kOptionImageQuality);
    final groupValue = bind.mainGetUserDefaultOption(key: kOptionImageQuality);
    return _Card(title: 'Default Image Quality', children: [
      _Radio(context,
          value: kRemoteImageQualityBest,
          groupValue: groupValue,
          label: 'Good image quality',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: kRemoteImageQualityBalanced,
          groupValue: groupValue,
          label: 'Balanced',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: kRemoteImageQualityLow,
          groupValue: groupValue,
          label: 'Optimize reaction time',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: kRemoteImageQualityCustom,
          groupValue: groupValue,
          label: 'Custom',
          onChanged: isOptFixed ? null : onChanged),
      Offstage(
        offstage: groupValue != kRemoteImageQualityCustom,
        child: customImageQualitySetting(),
      )
    ]);
  }

  Widget trackpadSpeed(BuildContext context) {
    final initSpeed =
        (int.tryParse(bind.mainGetUserDefaultOption(key: kKeyTrackpadSpeed)) ??
            kDefaultTrackpadSpeed);
    final curSpeed = SimpleWrapper(initSpeed);
    void onDebouncer(int v) {
      bind.mainSetUserDefaultOption(
          key: kKeyTrackpadSpeed, value: v.toString());
      // It's better to notify all sessions that the default speed is changed.
      // But it may also be ok to take effect in the next connection.
    }

    return _Card(title: 'Default trackpad speed', children: [
      TrackpadSpeedWidget(
        value: curSpeed,
        onDebouncer: onDebouncer,
      ),
    ]);
  }

  Widget codec(BuildContext context) {
    onChanged(String value) async {
      await bind.mainSetUserDefaultOption(
          key: kOptionCodecPreference, value: value);
      setState(() {});
    }

    final groupValue =
        bind.mainGetUserDefaultOption(key: kOptionCodecPreference);
    var hwRadios = [];
    final isOptFixed = isOptionFixed(kOptionCodecPreference);
    try {
      final Map codecsJson = jsonDecode(bind.mainSupportedHwdecodings());
      final h264 = codecsJson['h264'] ?? false;
      final h265 = codecsJson['h265'] ?? false;
      if (h264) {
        hwRadios.add(_Radio(context,
            value: 'h264',
            groupValue: groupValue,
            label: 'H264',
            onChanged: isOptFixed ? null : onChanged));
      }
      if (h265) {
        hwRadios.add(_Radio(context,
            value: 'h265',
            groupValue: groupValue,
            label: 'H265',
            onChanged: isOptFixed ? null : onChanged));
      }
    } catch (e) {
      debugPrint("failed to parse supported hwdecodings, err=$e");
    }
    return _Card(title: 'Default Codec', children: [
      _Radio(context,
          value: 'auto',
          groupValue: groupValue,
          label: 'Auto',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: 'vp8',
          groupValue: groupValue,
          label: 'VP8',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: 'vp9',
          groupValue: groupValue,
          label: 'VP9',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: 'av1',
          groupValue: groupValue,
          label: 'AV1',
          onChanged: isOptFixed ? null : onChanged),
      ...hwRadios,
    ]);
  }

  Widget privacyModeImpl(BuildContext context) {
    final supportedPrivacyModeImpls = bind.mainSupportedPrivacyModeImpls();
    late final List<dynamic> privacyModeImpls;
    try {
      privacyModeImpls = jsonDecode(supportedPrivacyModeImpls);
    } catch (e) {
      debugPrint('failed to parse supported privacy mode impls, err=$e');
      return Offstage();
    }
    if (privacyModeImpls.length < 2) {
      return Offstage();
    }

    final key = 'privacy-mode-impl-key';
    onChanged(String value) async {
      await bind.mainSetOption(key: key, value: value);
      setState(() {});
    }

    String groupValue = bind.mainGetOptionSync(key: key);
    if (groupValue.isEmpty) {
      groupValue = bind.mainDefaultPrivacyModeImpl();
    }
    return _Card(
      title: 'Privacy mode',
      children: privacyModeImpls.map((impl) {
        final d = impl as List<dynamic>;
        return _Radio(context,
            value: d[0] as String,
            groupValue: groupValue,
            label: d[1] as String,
            onChanged: onChanged);
      }).toList(),
    );
  }

  Widget otherRow(String label, String key) {
    final value = bind.mainGetUserDefaultOption(key: key) == 'Y';
    final isOptFixed = isOptionFixed(key);
    onChanged(bool b) async {
      await bind.mainSetUserDefaultOption(
          key: key,
          value: b
              ? 'Y'
              : (key == kOptionEnableFileCopyPaste ? 'N' : defaultOptionNo));
      setState(() {});
    }

    return GestureDetector(
        child: Row(
          children: [
            Checkbox(
                    value: value,
                    onChanged: isOptFixed ? null : (_) => onChanged(!value))
                .marginOnly(right: 5),
            Expanded(
              child: Text(translate(label)),
            )
          ],
        ).marginOnly(left: _kCheckBoxLeftMargin),
        onTap: isOptFixed ? null : () => onChanged(!value));
  }

  Widget other(BuildContext context) {
    final children =
        otherDefaultSettings().map((e) => otherRow(e.$1, e.$2)).toList();
    return _Card(title: 'Other Default Options', children: children);
  }
}

class _Checkbox extends StatefulWidget {
  final String label;
  final bool Function() getValue;
  final Future<void> Function(bool) setValue;

  const _Checkbox(
      {Key? key,
      required this.label,
      required this.getValue,
      required this.setValue})
      : super(key: key);

  @override
  State<_Checkbox> createState() => _CheckboxState();
}

class _CheckboxState extends State<_Checkbox> {
  var value = false;

  @override
  initState() {
    super.initState();
    value = widget.getValue();
  }

  @override
  Widget build(BuildContext context) {
    onChanged(bool b) async {
      await widget.setValue(b);
      setState(() {
        value = widget.getValue();
      });
    }

    return GestureDetector(
      child: Row(
        children: [
          Checkbox(
            value: value,
            onChanged: (_) => onChanged(!value),
          ).marginOnly(right: 5),
          Expanded(
            child: Text(translate(widget.label)),
          )
        ],
      ).marginOnly(left: _kCheckBoxLeftMargin),
      onTap: () => onChanged(!value),
    );
  }
}

class _Plugin extends StatefulWidget {
  final bool embedded;

  const _Plugin({Key? key, this.embedded = false}) : super(key: key);

  @override
  State<_Plugin> createState() => _PluginState();
}

class _PluginState extends State<_Plugin> {
  @override
  void initState() {
    super.initState();
    if (!widget.embedded) {
      bind.pluginListReload();
    }
  }

  @override
  Widget build(BuildContext context) {
    return ChangeNotifierProvider.value(
      value: pluginManager,
      child: Consumer<PluginManager>(builder: (context, model, child) {
        final children = model.plugins.map((entry) => pluginCard(entry)).toList();
        if (widget.embedded && children.isEmpty) {
          return _Card(title: 'Plugin', children: [
            Text(
              translate('No plugins available.'),
            ).marginOnly(left: _kContentHMargin),
            _Button('Reload', () => bind.pluginListReload()),
          ]);
        }
        if (widget.embedded) {
          return Column(children: [
            _Card(title: 'Plugin', children: [
              _Button('Reload', () => bind.pluginListReload()),
            ]),
            ...children,
          ]);
        }
        final scrollController = ScrollController();
        return ListView(
          controller: scrollController,
          children: children,
        ).marginOnly(bottom: _kListViewBottomMargin);
      }),
    );
  }

  Widget pluginCard(PluginInfo plugin) {
    return ChangeNotifierProvider.value(
      value: plugin,
      child: Consumer<PluginInfo>(
        builder: (context, model, child) => DesktopSettingsCard(plugin: model),
      ),
    );
  }
}

class _Printer extends StatefulWidget {
  final bool embedded;

  const _Printer({this.embedded = false});

  @override
  State<_Printer> createState() => __PrinterState();
}

class __PrinterState extends State<_Printer> {
  @override
  Widget build(BuildContext context) {
    final children = [
      outgoing(context),
      incoming(context),
    ];

    if (widget.embedded) {
      return Column(children: children);
    }

    final scrollController = ScrollController();

    return ListView(controller: scrollController, children: children)
        .marginOnly(bottom: _kListViewBottomMargin);
  }

  Widget outgoing(BuildContext context) {
    final isSupportPrinterDriver =
        bind.mainGetCommonSync(key: 'is-support-printer-driver') == 'true';

    Widget tipOsNotSupported() {
      return Align(
        alignment: Alignment.topLeft,
        child: Text(translate('printer-os-requirement-tip')),
      ).marginOnly(left: _kCardLeftMargin);
    }

    Widget tipClientNotInstalled() {
      return Align(
        alignment: Alignment.topLeft,
        child:
            Text(translate('printer-requires-installed-{$appName}-client-tip')),
      ).marginOnly(left: _kCardLeftMargin);
    }

    Widget tipPrinterNotInstalled() {
      final failedMsg = ''.obs;
      platformFFI.registerEventHandler(
          'install-printer-res', 'install-printer-res', (evt) async {
        if (evt['success'] as bool) {
          setState(() {});
        } else {
          failedMsg.value = evt['msg'] as String;
        }
      }, replace: true);
      return Column(children: [
        Obx(
          () => failedMsg.value.isNotEmpty
              ? Offstage()
              : Align(
                  alignment: Alignment.topLeft,
                  child: Text(translate('printer-{$appName}-not-installed-tip'))
                      .marginOnly(bottom: 10.0),
                ),
        ),
        Obx(
          () => failedMsg.value.isEmpty
              ? Offstage()
              : Align(
                  alignment: Alignment.topLeft,
                  child: Text(failedMsg.value,
                          style: DefaultTextStyle.of(context)
                              .style
                              .copyWith(color: Colors.red))
                      .marginOnly(bottom: 10.0)),
        ),
        _Button('Install {$appName} Printer', () {
          failedMsg.value = '';
          bind.mainSetCommon(key: 'install-printer', value: '');
        })
      ]).marginOnly(left: _kCardLeftMargin, bottom: 2.0);
    }

    Widget tipReady() {
      return Align(
        alignment: Alignment.topLeft,
        child: Text(translate('printer-{$appName}-ready-tip')),
      ).marginOnly(left: _kCardLeftMargin);
    }

    final installed = bind.mainIsInstalled();
    // `is-printer-installed` may fail, but it's rare case.
    // Add additional error message here if it's really needed.
    final isPrinterInstalled =
        bind.mainGetCommonSync(key: 'is-printer-installed') == 'true';

    final List<Widget> children = [];
    if (!isSupportPrinterDriver) {
      children.add(tipOsNotSupported());
    } else {
      children.addAll([
        if (!installed) tipClientNotInstalled(),
        if (installed && !isPrinterInstalled) tipPrinterNotInstalled(),
        if (installed && isPrinterInstalled) tipReady()
      ]);
    }
    return _Card(title: 'Outgoing Print Jobs', children: children);
  }

  Widget incoming(BuildContext context) {
    onRadioChanged(String value) async {
      await bind.mainSetLocalOption(
          key: kKeyPrinterIncomingJobAction, value: value);
      setState(() {});
    }

    PrinterOptions printerOptions = PrinterOptions.load();
    return _Card(title: 'Incoming Print Jobs', children: [
      _Radio(context,
          value: kValuePrinterIncomingJobDismiss,
          groupValue: printerOptions.action,
          label: 'Dismiss',
          onChanged: onRadioChanged),
      _Radio(context,
          value: kValuePrinterIncomingJobDefault,
          groupValue: printerOptions.action,
          label: 'use-the-default-printer-tip',
          onChanged: onRadioChanged),
      _Radio(context,
          value: kValuePrinterIncomingJobSelected,
          groupValue: printerOptions.action,
          label: 'use-the-selected-printer-tip',
          onChanged: onRadioChanged),
      if (printerOptions.printerNames.isNotEmpty)
        ComboBox(
          initialKey: printerOptions.printerName,
          keys: printerOptions.printerNames,
          values: printerOptions.printerNames,
          enabled: printerOptions.action == kValuePrinterIncomingJobSelected,
          onChanged: (value) async {
            await bind.mainSetLocalOption(
                key: kKeyPrinterSelected, value: value);
            setState(() {});
          },
        ).marginOnly(left: 10),
      _OptionCheckBox(
        context,
        'auto-print-tip',
        kKeyPrinterAllowAutoPrint,
        isServer: false,
        enabled: printerOptions.action != kValuePrinterIncomingJobDismiss,
      )
    ]);
  }
}

class _About extends StatefulWidget {
  final bool embedded;

  const _About({Key? key, this.embedded = false}) : super(key: key);

  @override
  State<_About> createState() => _AboutState();
}

class _AboutState extends State<_About> {
  static const String _kAboutUpdateHandler =
      'about_check_software_update_finish';
  final RxBool _isCheckingForUpdates = false.obs;

  BoxDecoration _aboutPanelDecoration(BuildContext context) {
    return BoxDecoration(
      color: Theme.of(context).cardColor,
      borderRadius: BorderRadius.circular(16),
      border: Border.all(color: Theme.of(context).dividerColor),
    );
  }

  @override
  void initState() {
    super.initState();
    platformFFI.registerEventHandler(
      kCheckSoftwareUpdateFinish,
      _kAboutUpdateHandler,
      (Map<String, dynamic> evt) async {
        _isCheckingForUpdates.value = false;
        final updateUrl = evt['url'] is String ? evt['url'] as String : '';
        final status = evt['status'] is String ? evt['status'] as String : '';
        final error = evt['error'] is String ? evt['error'] as String : '';

        stateGlobal.updateUrl.value = updateUrl;
        if (error.isNotEmpty) {
          msgBox(
            gFFI.sessionId,
            'custom-nocancel-hasclose',
            '检查更新',
            error,
            '',
            gFFI.dialogManager,
          );
        } else if (status == 'up-to-date') {
          msgBox(
            gFFI.sessionId,
            'custom-nocancel-hasclose',
            '检查更新',
            '当前已是最新版本。',
            '',
            gFFI.dialogManager,
          );
        }
      },
      replace: true,
    );
  }

  @override
  void dispose() {
    platformFFI.unregisterEventHandler(
        kCheckSoftwareUpdateFinish, _kAboutUpdateHandler);
    super.dispose();
  }

  void _checkForUpdates() {
    if (_isCheckingForUpdates.value) {
      return;
    }
    _isCheckingForUpdates.value = true;
    stateGlobal.updateUrl.value = '';
    bind.mainGetSoftwareUpdateUrl();
  }

  Widget _buildAboutMetaChip(
    BuildContext context, {
    required String label,
    required String value,
  }) {
    final colorScheme = Theme.of(context).colorScheme;
    final titleColor = Theme.of(context).textTheme.bodySmall?.color;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      decoration: BoxDecoration(
        color: colorScheme.surface,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: Theme.of(context).dividerColor),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            label,
            style: TextStyle(
              fontSize: 11,
              fontWeight: FontWeight.w600,
              color: titleColor?.withOpacity(0.7),
            ),
          ),
          const SizedBox(height: 4),
          SelectionArea(
            child: Text(
              value,
              style: const TextStyle(
                fontSize: 15,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildAboutLinkTile(
    BuildContext context, {
    required IconData icon,
    required String label,
    required VoidCallback onTap,
  }) {
    final textColor = Theme.of(context).textTheme.bodyMedium?.color;
    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        child: Ink(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
          decoration: _aboutPanelDecoration(context),
          child: Row(
            children: [
              Icon(icon, size: 18, color: _accentColor),
              const SizedBox(width: 10),
              Expanded(
                child: Text(
                  label,
                  style: TextStyle(
                    fontSize: 14,
                    fontWeight: FontWeight.w500,
                    color: textColor,
                  ),
                ),
              ),
              Icon(
                Icons.open_in_new_rounded,
                size: 16,
                color: textColor?.withOpacity(0.55),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildMaintenanceCard(BuildContext context) {
    final canDirectUpdate = isWindows || isMacOS;
    final titleStyle = Theme.of(context)
        .textTheme
        .titleMedium
        ?.copyWith(fontWeight: FontWeight.w600);
    final bodyStyle = Theme.of(context)
        .textTheme
        .bodyMedium
        ?.copyWith(height: 1.4);
    return Obx(() {
      final updateUrl = stateGlobal.updateUrl.value;
      final hasUpdate = updateUrl.isNotEmpty;
      final isChecking = _isCheckingForUpdates.value;
      final actionButtons = <Widget>[
        OutlinedButton.icon(
          onPressed: isChecking ? null : _checkForUpdates,
          icon: const Icon(Icons.refresh_rounded, size: 16),
          label: Text(translate(
            isChecking ? 'Checking for updates...' : 'Check for updates',
          )),
        ),
      ];
      if (hasUpdate) {
        actionButtons.add(ElevatedButton.icon(
          onPressed: () {
            if (canDirectUpdate) {
              handleUpdate(updateUrl);
            } else {
              launchUrlString(kHdeskPortalUrl);
            }
          },
          icon: const Icon(Icons.system_update_alt_rounded, size: 16),
          label: Text(translate('Update')),
        ));
      }
      return Container(
        width: double.infinity,
        padding: const EdgeInsets.all(16),
        decoration: _aboutPanelDecoration(context),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Container(
                  width: 36,
                  height: 36,
                  decoration: BoxDecoration(
                    color: _accentColor.withOpacity(0.12),
                    borderRadius: BorderRadius.circular(10),
                  ),
                  child: const Icon(
                    Icons.system_update_alt_rounded,
                    color: _accentColor,
                    size: 18,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        translate('Check for software update'),
                        style: titleStyle,
                      ),
                      const SizedBox(height: 4),
                      Text(
                        hasUpdate
                            ? (canDirectUpdate
                                ? translate(
                                    'A newer version is available and can be downloaded and installed directly.')
                                : translate(
                                    'A newer version is available. Visit the website to download the update.'))
                            : translate(
                                'Check and get the latest stable version of HDesk.'),
                        style: bodyStyle,
                      ),
                    ],
                  ),
                ),
              ],
            ),
            Wrap(
              alignment: WrapAlignment.start,
              spacing: 12,
              runSpacing: 12,
              children: actionButtons,
            ).marginOnly(top: hasUpdate ? 14 : 16),
          ],
        ),
      ).marginSymmetric(vertical: 4.0);
    });
  }

  @override
  Widget build(BuildContext context) {
    return futureBuilder(future: () async {
      final version = await bind.mainGetVersion();
      final buildDate = await bind.mainGetBuildDate();
      return {
        'version': version,
        'buildDate': buildDate,
        'copyrightYear': DateTime.now().toString().substring(0, 4),
      };
    }(), hasData: (data) {
      final version = data['version'].toString();
      final buildDate = data['buildDate'].toString();
      final copyrightYear = data['copyrightYear'].toString();
      final card = _Card(title: translate('About HDesk'), children: [
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 8.0),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(16),
              decoration: _aboutPanelDecoration(context),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'HDesk',
                    style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                          fontWeight: FontWeight.w700,
                        ),
                  ),
                  const SizedBox(height: 6),
                  Text(
                    translate('Desktop remote control client'),
                    style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                          color: Theme.of(context)
                              .textTheme
                              .bodyMedium
                              ?.color
                              ?.withOpacity(0.75),
                        ),
                  ),
                  const SizedBox(height: 14),
                  Wrap(
                    spacing: 12,
                    runSpacing: 12,
                    children: [
                      _buildAboutMetaChip(
                        context,
                        label: translate('Version'),
                        value: version,
                      ),
                      _buildAboutMetaChip(
                        context,
                        label: translate('Build Date'),
                        value: buildDate,
                      ),
                    ],
                  ),
                ],
              ),
            ),
            _buildMaintenanceCard(context),
            _buildAboutLinkTile(
              context,
              icon: Icons.privacy_tip_outlined,
              label: translate('Privacy Statement'),
              onTap: () {
                launchUrlString(kHdeskPrivacyUrl);
              },
            ),
            _buildAboutLinkTile(
              context,
              icon: Icons.description_outlined,
              label: translate('End-user license agreement'),
              onTap: () {
                launchUrlString(kHdeskUserAgreementUrl);
              },
            ).marginOnly(top: 8),
            _buildAboutLinkTile(
              context,
              icon: Icons.language_rounded,
              label: translate('Website'),
              onTap: () {
                launchUrlString(kHdeskPortalUrl);
              },
            ).marginOnly(top: 8),
            const SizedBox(height: 12),
            SelectionArea(
              child: Text(
                'Copyright © $copyrightYear 苏州云极创智科技有限公司',
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: Theme.of(context)
                          .textTheme
                          .bodySmall
                          ?.color
                          ?.withOpacity(0.72),
                    ),
              ).marginSymmetric(vertical: 4.0),
            ),
          ],
        ).marginOnly(left: _kContentHMargin)
      ]);

      if (widget.embedded) {
        return card;
      }

      final scrollController = ScrollController();

      return SingleChildScrollView(
        controller: scrollController,
        child: card,
      );
    });
  }
}

//#endregion

//#region components

// ignore: non_constant_identifier_names
Widget _Card(
    {required String title,
    required List<Widget> children,
    List<Widget>? title_suffix}) {
  return Row(
    children: [
      Flexible(
        child: SizedBox(
          width: _kCardFixedWidth,
          child: Card(
            child: Column(
              children: [
                Row(
                  children: [
                    Expanded(
                        child: Text(
                      translate(title),
                      textAlign: TextAlign.start,
                      style: const TextStyle(
                        fontSize: _kTitleFontSize,
                      ),
                    )),
                    ...?title_suffix
                  ],
                ).marginOnly(left: _kContentHMargin, top: 10, bottom: 10),
                ...children
                    .map((e) => e.marginOnly(top: 4, right: _kContentHMargin)),
              ],
            ).marginOnly(bottom: 10),
          ).marginOnly(left: _kCardLeftMargin, top: 15),
        ),
      ),
    ],
  );
}

// ignore: non_constant_identifier_names
Widget _OptionCheckBox(
  BuildContext context,
  String label,
  String key, {
  Function(bool)? update,
  bool reverse = false,
  bool enabled = true,
  Icon? checkedIcon,
  bool? fakeValue,
  bool isServer = true,
  bool Function()? optGetter,
  Future<void> Function(String, bool)? optSetter,
}) {
  getOpt() => optGetter != null
      ? optGetter()
      : (isServer
          ? mainGetBoolOptionSync(key)
          : mainGetLocalBoolOptionSync(key));
  bool value = getOpt();
  final isOptFixed = isOptionFixed(key);
  if (reverse) value = !value;
  var ref = value.obs;
  onChanged(option) async {
    if (option != null) {
      if (reverse) option = !option;
      final setter =
          optSetter ?? (isServer ? mainSetBoolOption : mainSetLocalBoolOption);
      await setter(key, option);
      final readOption = getOpt();
      if (reverse) {
        ref.value = !readOption;
      } else {
        ref.value = readOption;
      }
      update?.call(readOption);
    }
  }

  if (fakeValue != null) {
    ref.value = fakeValue;
    enabled = false;
  }

  return GestureDetector(
    child: Obx(
      () => Row(
        children: [
          Checkbox(
                  value: ref.value,
                  onChanged: enabled && !isOptFixed ? onChanged : null)
              .marginOnly(right: 5),
          Offstage(
            offstage: !ref.value || checkedIcon == null,
            child: checkedIcon?.marginOnly(right: 5),
          ),
          Expanded(
              child: Text(
            translate(label),
            style: TextStyle(color: disabledTextColor(context, enabled)),
          ))
        ],
      ),
    ).marginOnly(left: _kCheckBoxLeftMargin),
    onTap: enabled && !isOptFixed
        ? () {
            onChanged(!ref.value);
          }
        : null,
  );
}

// ignore: non_constant_identifier_names
Widget _Radio<T>(BuildContext context,
    {required T value,
    required T groupValue,
    required String label,
    required Function(T value)? onChanged,
    bool autoNewLine = true}) {
  final onChange2 = onChanged != null
      ? (T? value) {
          if (value != null) {
            onChanged(value);
          }
        }
      : null;
  return GestureDetector(
    child: Row(
      children: [
        Radio<T>(value: value, groupValue: groupValue, onChanged: onChange2),
        Expanded(
          child: Text(translate(label),
                  overflow: autoNewLine ? null : TextOverflow.ellipsis,
                  style: TextStyle(
                      fontSize: _kContentFontSize,
                      color: disabledTextColor(context, onChange2 != null)))
              .marginOnly(left: 5),
        ),
      ],
    ).marginOnly(left: _kRadioLeftMargin),
    onTap: () => onChange2?.call(value),
  );
}

class WaylandCard extends StatefulWidget {
  const WaylandCard({Key? key}) : super(key: key);

  @override
  State<WaylandCard> createState() => _WaylandCardState();
}

class _WaylandCardState extends State<WaylandCard> {
  final restoreTokenKey = 'wayland-restore-token';
  static const _kClearShortcutsInhibitorEventKey =
      'clear-gnome-shortcuts-inhibitor-permission-res';
  final _clearShortcutsInhibitorFailedMsg = ''.obs;
  // Don't show the shortcuts permission reset button for now.
  // Users can change it manually:
  //   "Settings" -> "Apps" -> "RustDesk" -> "Permissions" -> "Inhibit Shortcuts".
  // For resetting(clearing) the permission from the portal permission store, you can
  // use (replace <desktop-id> with the RustDesk desktop file ID):
  //   busctl --user call org.freedesktop.impl.portal.PermissionStore \
  //   /org/freedesktop/impl/portal/PermissionStore org.freedesktop.impl.portal.PermissionStore \
  //   DeletePermission sss "gnome" "shortcuts-inhibitor" "<desktop-id>"
  // On a native install this is typically "rustdesk.desktop"; on Flatpak it is usually
  // the exported desktop ID derived from the Flatpak app-id (e.g. "com.rustdesk.RustDesk.desktop").
  //
  // We may add it back in the future if needed.
  final showResetInhibitorPermission = false;

  @override
  void initState() {
    super.initState();
    if (showResetInhibitorPermission) {
      platformFFI.registerEventHandler(
          _kClearShortcutsInhibitorEventKey, _kClearShortcutsInhibitorEventKey,
          (evt) async {
        if (!mounted) return;
        if (evt['success'] == true) {
          setState(() {});
        } else {
          _clearShortcutsInhibitorFailedMsg.value =
              evt['msg'] as String? ?? 'Unknown error';
        }
      });
    }
  }

  @override
  void dispose() {
    if (showResetInhibitorPermission) {
      platformFFI.unregisterEventHandler(
          _kClearShortcutsInhibitorEventKey, _kClearShortcutsInhibitorEventKey);
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return futureBuilder(
      future: bind.mainHandleWaylandScreencastRestoreToken(
          key: restoreTokenKey, value: "get"),
      hasData: (restoreToken) {
        final hasShortcutsPermission = showResetInhibitorPermission &&
            bind.mainGetCommonSync(
                    key: "has-gnome-shortcuts-inhibitor-permission") ==
                "true";

        final children = [
          if (restoreToken.isNotEmpty)
            _buildClearScreenSelection(context, restoreToken),
          if (hasShortcutsPermission)
            _buildClearShortcutsInhibitorPermission(context),
        ];
        return Offstage(
          offstage: children.isEmpty,
          child: _Card(title: 'Wayland', children: children),
        );
      },
    );
  }

  Widget _buildClearScreenSelection(BuildContext context, String restoreToken) {
    onConfirm() async {
      final msg = await bind.mainHandleWaylandScreencastRestoreToken(
          key: restoreTokenKey, value: "clear");
      gFFI.dialogManager.dismissAll();
      if (msg.isNotEmpty) {
        msgBox(gFFI.sessionId, 'custom-nocancel', 'Error', msg, '',
            gFFI.dialogManager);
      } else {
        setState(() {});
      }
    }

    showConfirmMsgBox() => msgBoxCommon(
            gFFI.dialogManager,
            'Confirmation',
            Text(
              translate('confirm_clear_Wayland_screen_selection_tip'),
            ),
            [
              dialogButton('OK', onPressed: onConfirm),
              dialogButton('Cancel',
                  onPressed: () => gFFI.dialogManager.dismissAll())
            ]);

    return _Button(
      'Clear Wayland screen selection',
      showConfirmMsgBox,
      tip: 'clear_Wayland_screen_selection_tip',
      style: ButtonStyle(
        backgroundColor: MaterialStateProperty.all<Color>(
            Theme.of(context).colorScheme.error.withOpacity(0.75)),
      ),
    );
  }

  Widget _buildClearShortcutsInhibitorPermission(BuildContext context) {
    onConfirm() {
      _clearShortcutsInhibitorFailedMsg.value = '';
      bind.mainSetCommon(
          key: "clear-gnome-shortcuts-inhibitor-permission", value: "");
      gFFI.dialogManager.dismissAll();
    }

    showConfirmMsgBox() => msgBoxCommon(
            gFFI.dialogManager,
            'Confirmation',
            Text(
              translate('confirm-clear-shortcuts-inhibitor-permission-tip'),
            ),
            [
              dialogButton('OK', onPressed: onConfirm),
              dialogButton('Cancel',
                  onPressed: () => gFFI.dialogManager.dismissAll())
            ]);

    return Column(children: [
      Obx(
        () => _clearShortcutsInhibitorFailedMsg.value.isEmpty
            ? Offstage()
            : Align(
                alignment: Alignment.topLeft,
                child: Text(_clearShortcutsInhibitorFailedMsg.value,
                        style: DefaultTextStyle.of(context)
                            .style
                            .copyWith(color: Colors.red))
                    .marginOnly(bottom: 10.0)),
      ),
      _Button(
        'Reset keyboard shortcuts permission',
        showConfirmMsgBox,
        tip: 'clear-shortcuts-inhibitor-permission-tip',
        style: ButtonStyle(
          backgroundColor: MaterialStateProperty.all<Color>(
              Theme.of(context).colorScheme.error.withOpacity(0.75)),
        ),
      ),
    ]);
  }
}

// ignore: non_constant_identifier_names
Widget _Button(String label, Function() onPressed,
    {bool enabled = true, String? tip, ButtonStyle? style}) {
  var button = ElevatedButton(
    onPressed: enabled ? onPressed : null,
    child: Text(
      translate(label),
    ).marginSymmetric(horizontal: 15),
    style: style,
  );
  StatefulWidget child;
  if (tip == null) {
    child = button;
  } else {
    child = Tooltip(message: translate(tip), child: button);
  }
  return Row(children: [
    child,
  ]).marginOnly(left: _kContentHMargin);
}

// ignore: non_constant_identifier_names
Widget _SubButton(String label, Function() onPressed, [bool enabled = true]) {
  return Row(
    children: [
      ElevatedButton(
        onPressed: enabled ? onPressed : null,
        child: Text(
          translate(label),
        ).marginSymmetric(horizontal: 15),
      ),
    ],
  ).marginOnly(left: _kContentHSubMargin);
}

// ignore: non_constant_identifier_names
Widget _SubLabeledWidget(BuildContext context, String label, Widget child,
    {bool enabled = true}) {
  return Row(
    children: [
      Text(
        '${translate(label)}: ',
        style: TextStyle(color: disabledTextColor(context, enabled)),
      ),
      SizedBox(
        width: 10,
      ),
      child,
    ],
  ).marginOnly(left: _kContentHSubMargin);
}

Widget _lock(
  bool locked,
  String label,
  Function() onUnlock,
) {
  return Offstage(
      offstage: !locked,
      child: Row(
        children: [
          Flexible(
            child: SizedBox(
              width: _kCardFixedWidth,
              child: Card(
                child: ElevatedButton(
                  child: SizedBox(
                      height: 25,
                      child: Row(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            const Icon(
                              Icons.security_sharp,
                              size: 20,
                            ),
                            Text(translate(label)).marginOnly(left: 5),
                          ]).marginSymmetric(vertical: 2)),
                  onPressed: () async {
                    final unlockPin = bind.mainGetUnlockPin();
                    if (unlockPin.isEmpty || isUnlockPinDisabled()) {
                      bool checked = await callMainCheckSuperUserPermission();
                      if (checked) {
                        onUnlock();
                      }
                    } else {
                      checkUnlockPinDialog(unlockPin, onUnlock);
                    }
                  },
                ).marginSymmetric(horizontal: 2, vertical: 4),
              ).marginOnly(left: _kCardLeftMargin),
            ).marginOnly(top: 10),
          ),
        ],
      ));
}

class _CountDownButton extends StatefulWidget {
  _CountDownButton({
    Key? key,
    required this.text,
    required this.second,
    required this.onPressed,
  }) : super(key: key);
  final String text;
  final VoidCallback? onPressed;
  final int second;

  @override
  State<_CountDownButton> createState() => _CountDownButtonState();
}

class _CountDownButtonState extends State<_CountDownButton> {
  bool _isButtonDisabled = false;

  late int _countdownSeconds = widget.second;

  Timer? _timer;

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  void _startCountdownTimer() {
    _timer = Timer.periodic(Duration(seconds: 1), (timer) {
      if (_countdownSeconds <= 0) {
        setState(() {
          _isButtonDisabled = false;
        });
        timer.cancel();
      } else {
        setState(() {
          _countdownSeconds--;
        });
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return ElevatedButton(
      onPressed: _isButtonDisabled
          ? null
          : () {
              widget.onPressed?.call();
              setState(() {
                _isButtonDisabled = true;
                _countdownSeconds = widget.second;
              });
              _startCountdownTimer();
            },
      child: Text(
        _isButtonDisabled ? '$_countdownSeconds s' : translate(widget.text),
      ),
    );
  }
}

//#endregion

//#region dialogs

void changeSocks5Proxy() async {
  var socks = await bind.mainGetSocks();

  String proxy = '';
  String proxyMsg = '';
  String username = '';
  String password = '';
  if (socks.length == 3) {
    proxy = socks[0];
    username = socks[1];
    password = socks[2];
  }
  var proxyController = TextEditingController(text: proxy);
  var userController = TextEditingController(text: username);
  var pwdController = TextEditingController(text: password);
  RxBool obscure = true.obs;

  // proxy settings
  // The following option is a not real key, it is just used for custom client advanced settings.
  const String optionProxyUrl = "proxy-url";
  final isOptFixed = isOptionFixed(optionProxyUrl);

  var isInProgress = false;
  gFFI.dialogManager.show((setState, close, context) {
    submit() async {
      setState(() {
        proxyMsg = '';
        isInProgress = true;
      });
      cancel() {
        setState(() {
          isInProgress = false;
        });
      }

      proxy = proxyController.text.trim();
      username = userController.text.trim();
      password = pwdController.text.trim();

      if (proxy.isNotEmpty) {
        String domainPort = proxy;
        if (domainPort.contains('://')) {
          domainPort = domainPort.split('://')[1];
        }
        proxyMsg = translate(await bind.mainTestIfValidServer(
            server: domainPort, testWithProxy: false));
        if (proxyMsg.isEmpty) {
          // ignore
        } else {
          cancel();
          return;
        }
      }
      await bind.mainSetSocks(
          proxy: proxy, username: username, password: password);
      close();
    }

    return CustomAlertDialog(
      title: Text(translate('Socks5/Http(s) Proxy')),
      content: ConstrainedBox(
        constraints: const BoxConstraints(minWidth: 500),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                if (!isMobile)
                  ConstrainedBox(
                    constraints: const BoxConstraints(minWidth: 140),
                    child: Align(
                        alignment: Alignment.centerRight,
                        child: Row(
                          children: [
                            Text(
                              translate('Server'),
                            ).marginOnly(right: 4),
                            Tooltip(
                              waitDuration: Duration(milliseconds: 0),
                              message: translate("default_proxy_tip"),
                              child: Icon(
                                Icons.help_outline_outlined,
                                size: 16,
                                color: Theme.of(context)
                                    .textTheme
                                    .titleLarge
                                    ?.color
                                    ?.withOpacity(0.5),
                              ),
                            ),
                          ],
                        )).marginOnly(right: 10),
                  ),
                Expanded(
                  child: TextField(
                    decoration: InputDecoration(
                      errorText: proxyMsg.isNotEmpty ? proxyMsg : null,
                      labelText: isMobile ? translate('Server') : null,
                      helperText:
                          isMobile ? translate("default_proxy_tip") : null,
                      helperMaxLines: isMobile ? 3 : null,
                    ),
                    controller: proxyController,
                    autofocus: true,
                    enabled: !isOptFixed,
                  ).workaroundFreezeLinuxMint(),
                ),
              ],
            ).marginOnly(bottom: 8),
            Row(
              children: [
                if (!isMobile)
                  ConstrainedBox(
                      constraints: const BoxConstraints(minWidth: 140),
                      child: Text(
                        '${translate("Username")}:',
                        textAlign: TextAlign.right,
                      ).marginOnly(right: 10)),
                Expanded(
                  child: TextField(
                    controller: userController,
                    decoration: InputDecoration(
                      labelText: isMobile ? translate('Username') : null,
                    ),
                    enabled: !isOptFixed,
                  ).workaroundFreezeLinuxMint(),
                ),
              ],
            ).marginOnly(bottom: 8),
            Row(
              children: [
                if (!isMobile)
                  ConstrainedBox(
                      constraints: const BoxConstraints(minWidth: 140),
                      child: Text(
                        '${translate("Password")}:',
                        textAlign: TextAlign.right,
                      ).marginOnly(right: 10)),
                Expanded(
                  child: Obx(() => TextField(
                        obscureText: obscure.value,
                        decoration: InputDecoration(
                            labelText: isMobile ? translate('Password') : null,
                            suffixIcon: IconButton(
                                onPressed: () => obscure.value = !obscure.value,
                                icon: Icon(obscure.value
                                    ? Icons.visibility_off
                                    : Icons.visibility))),
                        controller: pwdController,
                        enabled: !isOptFixed,
                        maxLength: bind.mainMaxEncryptLen(),
                      ).workaroundFreezeLinuxMint()),
                ),
              ],
            ),
            // NOT use Offstage to wrap LinearProgressIndicator
            if (isInProgress)
              const LinearProgressIndicator().marginOnly(top: 8),
          ],
        ),
      ),
      actions: [
        dialogButton('Cancel', onPressed: close, isOutline: true),
        if (!isOptFixed) dialogButton('OK', onPressed: submit),
      ],
      onSubmit: submit,
      onCancel: close,
    );
  });
}

//#endregion
