import 'dart:ffi' hide Size;

import 'package:ffi/ffi.dart';

import 'package:win32/win32.dart' as win32;

/// Get windows target build number.
///
/// [Note]
/// Please use this function wrapped with `Platform.isWindows`.
int getWindowsTargetBuildNumber_() {
  final rtlGetVersion = DynamicLibrary.open('ntdll.dll').lookupFunction<
      Void Function(Pointer<win32.OSVERSIONINFOEX>),
      void Function(Pointer<win32.OSVERSIONINFOEX>)>('RtlGetVersion');
  final osVersionInfo = _getOSVERSIONINFOEXPointer();
  rtlGetVersion(osVersionInfo);
  int buildNumber = osVersionInfo.ref.dwBuildNumber;
  calloc.free(osVersionInfo);
  return buildNumber;
}

/// Get Windows OS version pointer
///
/// [Note]
/// Please use this function wrapped with `Platform.isWindows`.
Pointer<win32.OSVERSIONINFOEX> _getOSVERSIONINFOEXPointer() {
  final pointer = calloc<win32.OSVERSIONINFOEX>();
  pointer.ref
    ..dwOSVersionInfoSize = sizeOf<win32.OSVERSIONINFOEX>()
    ..dwBuildNumber = 0
    ..dwMajorVersion = 0
    ..dwMinorVersion = 0
    ..dwPlatformId = 0
    ..szCSDVersion = ''
    ..wServicePackMajor = 0
    ..wServicePackMinor = 0
    ..wSuiteMask = 0
    ..wProductType = 0
    ..wReserved = 0;
  return pointer;
}

/// Update the resizable style of a top-level window with an exact title match.
///
/// Desktop multi-window engines do not register the window_manager plugin, so
/// Windows child-window styles need to be updated through Win32 directly.
bool setWindowResizableByTitle_(String title, bool resizable) {
  final nativeTitle = title.toNativeUtf16();
  final processId = calloc<Uint32>();
  try {
    final currentProcessId = win32.GetCurrentProcessId();
    var window = 0;
    while (true) {
      window = win32.FindWindowEx(0, window, nullptr, nativeTitle);
      if (window == 0) {
        return false;
      }
      processId.value = 0;
      if (win32.GetWindowThreadProcessId(window, processId) != 0 &&
          processId.value == currentProcessId) {
        break;
      }
    }

    win32.SetLastError(win32.ERROR_SUCCESS);
    final currentStyle = win32.GetWindowLongPtr(window, win32.GWL_STYLE);
    if (currentStyle == 0 && win32.GetLastError() != win32.ERROR_SUCCESS) {
      return false;
    }
    final resizeStyles = win32.WS_THICKFRAME | win32.WS_MAXIMIZEBOX;
    final nextStyle =
        resizable ? currentStyle | resizeStyles : currentStyle & ~resizeStyles;
    if (nextStyle == currentStyle) {
      return true;
    }

    win32.SetLastError(win32.ERROR_SUCCESS);
    final previousStyle =
        win32.SetWindowLongPtr(window, win32.GWL_STYLE, nextStyle);
    if (previousStyle == 0 && win32.GetLastError() != win32.ERROR_SUCCESS) {
      return false;
    }
    return win32.SetWindowPos(
          window,
          0,
          0,
          0,
          0,
          0,
          win32.SWP_FRAMECHANGED |
              win32.SWP_NOMOVE |
              win32.SWP_NOSIZE |
              win32.SWP_NOZORDER |
              win32.SWP_NOACTIVATE,
        ) !=
        0;
  } finally {
    calloc.free(processId);
    calloc.free(nativeTitle);
  }
}
