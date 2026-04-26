import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/desktop/pages/remote_tab_page.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/models/state_model.dart';
import 'package:flutter_hbb/utils/desktop_crash_trace.dart';
import 'package:provider/provider.dart';

/// multi-tab desktop remote screen
class DesktopRemoteScreen extends StatefulWidget {
  final Map<String, dynamic> params;

  const DesktopRemoteScreen({Key? key, required this.params}) : super(key: key);

  @override
  State<DesktopRemoteScreen> createState() => _DesktopRemoteScreenState();
}

class _DesktopRemoteScreenState extends State<DesktopRemoteScreen>
    with WidgetsBindingObserver {
  bool _loggedFirstBuild = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    bind.mainInitInputSource();
    stateGlobal.getInputSource(force: true);
    DesktopCrashTrace.log(
      'DesktopRemoteScreen.initState windowId=${stateGlobal.windowId} peerId=${widget.params['id']} display=${widget.params['display']} hasScreenRect=${widget.params['screen_rect'] != null}'
    );
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    super.didChangeAppLifecycleState(state);
    DesktopCrashTrace.log(
      'DesktopRemoteScreen.lifecycle state=$state windowId=${stateGlobal.windowId}'
    );
  }

  @override
  void dispose() {
    DesktopCrashTrace.log(
      'DesktopRemoteScreen.dispose windowId=${stateGlobal.windowId} peerId=${widget.params['id']}'
    );
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!_loggedFirstBuild) {
      _loggedFirstBuild = true;
      DesktopCrashTrace.log(
        'DesktopRemoteScreen.firstBuild windowId=${stateGlobal.windowId} peerId=${widget.params['id']} sessionId=${gFFI.sessionId}'
      );
    }
    return MultiProvider(
        providers: [
          ChangeNotifierProvider.value(value: gFFI.ffiModel),
          ChangeNotifierProvider.value(value: gFFI.imageModel),
          ChangeNotifierProvider.value(value: gFFI.cursorModel),
          ChangeNotifierProvider.value(value: gFFI.canvasModel),
        ],
        child: Scaffold(
          // Set transparent background for padding the resize area out of the flutter view.
          // This allows the wallpaper goes through our resize area. (Linux only now).
          backgroundColor: isLinux ? Colors.transparent : null,
          body: ConnectionTabPage(
            params: widget.params,
          ),
        ));
  }
}
