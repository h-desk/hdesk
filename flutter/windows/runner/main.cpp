#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>
#include <tchar.h>
#include <uni_links_desktop/uni_links_desktop_plugin.h>
#include <windows.h>

#include <algorithm>
#include <iostream>

#include "win32_desktop.h"
#include "flutter_window.h"
#include "utils.h"

typedef char** (*FUNC_RUSTDESK_CORE_MAIN)(int*);
typedef void (*FUNC_RUSTDESK_FREE_ARGS)( char**, int);
typedef int (*FUNC_RUSTDESK_GET_APP_NAME)(wchar_t*, int);
/// Note: `--server`, `--service` are already handled in [core_main.rs].
const std::vector<std::string> parameters_white_list = {"--install", "--cm"};
constexpr ULONGLONG kLikelyMissingHwcodecDllThresholdBytes = 29ull * 1024ull * 1024ull;

const wchar_t* getWindowClassName();

namespace {

constexpr const wchar_t kHDeskAppUserModelId[] = L"YunjiChuangzhi.HDesk";

void SetExplicitAppUserModelIdIfAvailable() {
  HMODULE shell32 = LoadLibraryW(L"shell32.dll");
  if (shell32 == nullptr) {
    return;
  }

  using SetAppIdFn = HRESULT(WINAPI*)(PCWSTR);
  auto set_app_id = reinterpret_cast<SetAppIdFn>(
      GetProcAddress(shell32, "SetCurrentProcessExplicitAppUserModelID"));
  if (set_app_id != nullptr) {
    set_app_id(kHDeskAppUserModelId);
  }

  FreeLibrary(shell32);
}

std::wstring GetSiblingLibraryPath(const wchar_t* library_name) {
  wchar_t module_path[MAX_PATH] = {0};
  DWORD path_length = ::GetModuleFileNameW(nullptr, module_path, MAX_PATH);
  if (path_length == 0 || path_length >= MAX_PATH) {
    return std::wstring(library_name);
  }

  std::wstring executable_path(module_path, path_length);
  size_t separator = executable_path.find_last_of(L"\\/");
  if (separator == std::wstring::npos) {
    return std::wstring(library_name);
  }
  return executable_path.substr(0, separator + 1) + library_name;
}

ULONGLONG GetFileSizeBytes(const std::wstring& file_path) {
  WIN32_FILE_ATTRIBUTE_DATA file_info = {0};
  if (!::GetFileAttributesExW(file_path.c_str(), GetFileExInfoStandard,
                              &file_info)) {
    return 0;
  }
  ULARGE_INTEGER file_size = {0};
  file_size.HighPart = file_info.nFileSizeHigh;
  file_size.LowPart = file_info.nFileSizeLow;
  return file_size.QuadPart;
}

bool WarnIfLikelyMissingHwcodec(const std::wstring& dll_path) {
  ULONGLONG dll_size = GetFileSizeBytes(dll_path);
  if (dll_size == 0 || dll_size >= kLikelyMissingHwcodecDllThresholdBytes) {
    return false;
  }

  std::wstring message =
      L"The current librustdesk.dll is too small and is likely missing hwcodec support.\n\n"
      L"This commonly causes a black screen with Harmony clients.\n\n"
      L"Please rebuild with:\n"
      L"python build.py --flutter --skip-portable-pack --hwcodec\n"
      L"or\n"
      L"cargo build --features flutter,hwcodec --lib --release";
  ::MessageBoxW(nullptr, message.c_str(), L"HDesk startup blocked",
                MB_OK | MB_ICONERROR);
  return true;
}

}  // namespace

int APIENTRY wWinMain(_In_ HINSTANCE instance, _In_opt_ HINSTANCE prev,
                      _In_ wchar_t *command_line, _In_ int show_command)
{
  SetExplicitAppUserModelIdIfAvailable();

  std::wstring rustdesk_dll_path = GetSiblingLibraryPath(L"librustdesk.dll");
  if (WarnIfLikelyMissingHwcodec(rustdesk_dll_path)) {
    return EXIT_FAILURE;
  }

  HINSTANCE hInstance = LoadLibraryW(rustdesk_dll_path.c_str());
  if (!hInstance)
  {
    std::cout << "Failed to load librustdesk.dll." << std::endl;
    return EXIT_FAILURE;
  }
  FUNC_RUSTDESK_CORE_MAIN rustdesk_core_main =
      (FUNC_RUSTDESK_CORE_MAIN)GetProcAddress(hInstance, "rustdesk_core_main_args");
  if (!rustdesk_core_main)
  {
    std::cout << "Failed to get rustdesk_core_main." << std::endl;
    return EXIT_FAILURE;
  }
  FUNC_RUSTDESK_FREE_ARGS free_c_args =
      (FUNC_RUSTDESK_FREE_ARGS)GetProcAddress(hInstance, "free_c_args");
  if (!free_c_args)
  {
    std::cout << "Failed to get free_c_args." << std::endl;
    return EXIT_FAILURE;
  }
  std::vector<std::string> command_line_arguments =
      GetCommandLineArguments();
  // Remove possible trailing whitespace from command line arguments
  for (auto& argument : command_line_arguments) {
    argument.erase(argument.find_last_not_of(" \n\r\t"));
  }

  int args_len = 0;
  char** c_args = rustdesk_core_main(&args_len);
  if (!c_args)
  {
    std::string args_str = "";
    for (const auto& argument : command_line_arguments) {
      args_str += (argument + " ");
    }
    // std::cout << "RustDesk [" << args_str << "], core returns false, exiting without launching Flutter app." << std::endl;
    return EXIT_SUCCESS;
  }
  std::vector<std::string> rust_args(c_args, c_args + args_len);
  free_c_args(c_args, args_len);

  std::wstring app_name = L"HDesk";
  FUNC_RUSTDESK_GET_APP_NAME get_rustdesk_app_name = (FUNC_RUSTDESK_GET_APP_NAME)GetProcAddress(hInstance, "get_rustdesk_app_name");
  if (get_rustdesk_app_name) {
    wchar_t app_name_buffer[512] = {0};
    if (get_rustdesk_app_name(app_name_buffer, 512) == 0) {
      app_name = std::wstring(app_name_buffer);
    }
  }

  // Uri links dispatch
  HWND hwnd = ::FindWindowW(getWindowClassName(), app_name.c_str());
  if (hwnd != NULL) {
    // Allow multiple flutter instances when being executed by parameters
    // contained in whitelists.
    bool allow_multiple_instances = false;
    for (auto& whitelist_param : parameters_white_list) {
      allow_multiple_instances =
          allow_multiple_instances ||
          std::find(command_line_arguments.begin(),
                    command_line_arguments.end(),
                    whitelist_param) != command_line_arguments.end();
    }
    if (!allow_multiple_instances) {
      if (!command_line_arguments.empty()) {
        // Dispatch command line arguments
        DispatchToUniLinksDesktop(hwnd);
      } else {
        // Not called with arguments, or just open the app shortcut on desktop.
        // So we just show the main window instead.
        ::ShowWindow(hwnd, SW_NORMAL);
        ::SetForegroundWindow(hwnd);
      }
      return EXIT_FAILURE;
    }
  }

  // Attach to console when present (e.g., 'flutter run') or create a
  // new console when running with a debugger.
  if (!::AttachConsole(ATTACH_PARENT_PROCESS) && ::IsDebuggerPresent())
  {
    CreateAndAttachConsole();
  }

  // Initialize COM, so that it is available for use in the library and/or
  // plugins.
  ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

  flutter::DartProject project(L"data");
  // connection manager hide icon from taskbar
  bool is_cm_page = false;
  auto cmParam = std::string("--cm");
  if (!command_line_arguments.empty() && command_line_arguments.front().compare(0, cmParam.size(), cmParam.c_str()) == 0) {
    is_cm_page = true;
  }
  bool is_install_page = false;
  auto installParam = std::string("--install");
  if (!command_line_arguments.empty() && command_line_arguments.front().compare(0, installParam.size(), installParam.c_str()) == 0) {
    is_install_page = true;
  }

  command_line_arguments.insert(command_line_arguments.end(), rust_args.begin(), rust_args.end());
  project.set_dart_entrypoint_arguments(std::move(command_line_arguments));

  FlutterWindow window(project);

  // Get primary monitor's work area.
  Win32Window::Point workarea_origin(0, 0);
  Win32Window::Size workarea_size(0, 0);

  Win32Desktop::GetWorkArea(workarea_origin, workarea_size);

  // Compute window bounds for default main window position: (10, 10) x(800, 600)
  Win32Window::Point relative_origin(10, 10);

  Win32Window::Point origin(workarea_origin.x + relative_origin.x, workarea_origin.y + relative_origin.y);
  Win32Window::Size size(800u, 600u);

  // Fit the window to the monitor's work area.
  Win32Desktop::FitToWorkArea(origin, size);

  std::wstring window_title;
  if (is_cm_page) {
    window_title = app_name + L" - Connection Manager";
  } else if (is_install_page) {
    window_title = app_name + L" - Install";
  } else {
    window_title = app_name;
  }
  if (!window.CreateAndShow(window_title, origin, size, !is_cm_page)) {
      return EXIT_FAILURE;
  }
  window.SetQuitOnClose(true);

  ::MSG msg;
  while (::GetMessage(&msg, nullptr, 0, 0))
  {
    ::TranslateMessage(&msg);
    ::DispatchMessage(&msg);
  }

  ::CoUninitialize();
  return EXIT_SUCCESS;
}
