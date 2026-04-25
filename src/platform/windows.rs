use super::{CursorData, ResultType};
use crate::{
    common::PORTABLE_APPNAME_RUNTIME_ENV_KEY,
    custom_server::*,
    ipc,
    privacy_mode::win_topmost_window::{self, WIN_TOPMOST_INJECTED_PROCESS_EXE},
};
use hbb_common::{
    allow_err,
    anyhow::anyhow,
    bail,
    config::{self, Config},
    libc::{c_int, wchar_t},
    log,
    message_proto::{DisplayInfo, Resolution, WindowsSession},
    sleep,
    sysinfo::{Pid, System},
    timeout, tokio,
};
use std::{
    collections::HashMap,
    ffi::{CString, OsString},
    fs,
    io::{self, prelude::*},
    mem,
    os::{
        raw::c_ulong,
        windows::{ffi::OsStringExt, process::CommandExt},
    },
    path::*,
    ptr::null_mut,
    sync::{atomic::Ordering, Arc, Mutex},
    time::{Duration, Instant},
};
use wallpaper;
#[cfg(not(debug_assertions))]
use winapi::um::libloaderapi::{LoadLibraryExW, LOAD_LIBRARY_SEARCH_USER_DIRS};
use winapi::{
    ctypes::c_void,
    shared::{minwindef::*, ntdef::NULL, windef::*, winerror::*},
    um::{
        errhandlingapi::GetLastError,
        handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
        libloaderapi::{
            GetProcAddress, LoadLibraryA, LoadLibraryExA, LOAD_LIBRARY_SEARCH_SYSTEM32,
        },
        minwinbase::STILL_ACTIVE,
        processthreadsapi::{
            GetCurrentProcess, GetCurrentProcessId, GetExitCodeProcess, OpenProcess,
            OpenProcessToken, ProcessIdToSessionId, PROCESS_INFORMATION, STARTUPINFOW,
        },
        securitybaseapi::{
            AllocateAndInitializeSid, DuplicateToken, EqualSid, FreeSid, GetTokenInformation,
        },
        shellapi::ShellExecuteW,
        sysinfoapi::{GetNativeSystemInfo, SYSTEM_INFO},
        winbase::*,
        wingdi::*,
        winnt::{
            SecurityImpersonation, TokenElevation, TokenGroups, TokenImpersonation, TokenType,
            DOMAIN_ALIAS_RID_ADMINS, ES_AWAYMODE_REQUIRED, ES_CONTINUOUS, ES_DISPLAY_REQUIRED,
            ES_SYSTEM_REQUIRED, HANDLE, PROCESS_ALL_ACCESS, PROCESS_QUERY_LIMITED_INFORMATION,
            PSID, SECURITY_BUILTIN_DOMAIN_RID, SECURITY_NT_AUTHORITY, SID_IDENTIFIER_AUTHORITY,
            TOKEN_ELEVATION, TOKEN_GROUPS, TOKEN_QUERY, TOKEN_TYPE,
        },
        winreg::HKEY_CURRENT_USER,
        winspool::{
            EnumPrintersW, GetDefaultPrinterW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
            PRINTER_INFO_1W,
        },
        winuser::*,
    },
};
use windows::Win32::{
    Foundation::{CloseHandle as WinCloseHandle, HANDLE as WinHANDLE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    },
};
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
};
use winreg::{enums::*, RegKey};

pub const FLUTTER_RUNNER_WIN32_WINDOW_CLASS: &'static str = "FLUTTER_RUNNER_WIN32_WINDOW"; // main window, install window
pub const EXPLORER_EXE: &'static str = "explorer.exe";
pub const SET_FOREGROUND_WINDOW: &'static str = "SET_FOREGROUND_WINDOW";

const REG_NAME_INSTALL_DESKTOPSHORTCUTS: &str = "DESKTOPSHORTCUTS";
const REG_NAME_INSTALL_STARTMENUSHORTCUTS: &str = "STARTMENUSHORTCUTS";
pub const REG_NAME_INSTALL_PRINTER: &str = "PRINTER";

pub fn get_focused_display(displays: Vec<DisplayInfo>) -> Option<usize> {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut rect: RECT = mem::zeroed();
        if GetWindowRect(hwnd, &mut rect as *mut RECT) == 0 {
            return None;
        }
        displays.iter().position(|display| {
            let center_x = rect.left + (rect.right - rect.left) / 2;
            let center_y = rect.top + (rect.bottom - rect.top) / 2;
            center_x >= display.x
                && center_x < display.x + display.width
                && center_y >= display.y
                && center_y < display.y + display.height
        })
    }
}

/// Editable focus hint data returned by `get_editable_focus_hint`.
/// All rects are in desktop global coordinates (pixels, origin = virtual-desktop top-left).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct EditableFocusHintInfo {
    /// Whether the currently focused UI element accepts keyboard text input.
    pub editable: bool,
    /// Caret rectangle (x, y, w, h). May be (0,0,0,0) if unavailable.
    pub caret: (i32, i32, i32, i32),
    /// Editing area rectangle. Falls back to window rect when UIA unavailable.
    pub editor: (i32, i32, i32, i32),
    /// Foreground window bounding rectangle.
    pub window: (i32, i32, i32, i32),
    /// Pane rectangle (e.g. browser tab content area).
    pub pane: (i32, i32, i32, i32),
    /// Index into the displays slice for the display containing the editor.
    pub display_idx: i32,
    /// 0=unknown, 1=single-line, 2=multi-line, 3=code
    pub content_kind: i32,
    /// Foreground window handle (used to detect app switch when window rects are identical).
    /// This is NOT transmitted over the wire, only used for local change detection.
    pub foreground_hwnd: isize,
}

#[derive(Clone)]
struct EditableFocusStickyState {
    fg_hwnd: isize,
    hint: EditableFocusHintInfo,
    expires_at: Instant,
    transient_expires_at: Instant,
}

const EDITABLE_FOCUS_STICKY_MS: u64 = 700;
const EDITABLE_FOCUS_TRANSIENT_REUSE_MS: u64 = 3000;

lazy_static::lazy_static! {
    static ref EDITABLE_FOCUS_STICKY: Mutex<Option<EditableFocusStickyState>> = Mutex::new(None);
    static ref QT_CARET_CONFIRMED: Mutex<Option<QtCaretConfirmation>> = Mutex::new(None);
}

/// How long a caret-confirmed Qt hint stays valid after the last caret sighting.
/// Qt apps like WeChat expose a system caret for only ~1 second after clicking
/// the input area. This extended TTL keeps reporting editable=true so the mobile
/// keyboard stays open.
const QT_CARET_CONFIRMED_TTL_MS: u64 = 30_000;

#[derive(Clone)]
struct QtCaretConfirmation {
    fg_hwnd: isize,
    hint: EditableFocusHintInfo,
    last_caret_at: Instant,
}

unsafe fn is_qt_class(hwnd: HWND) -> bool {
    let mut buf = [0u16; 256];
    let len = winapi::um::winuser::GetClassNameW(hwnd, buf.as_mut_ptr(), 256) as usize;
    if len == 0 {
        return false;
    }
    let cls = String::from_utf16_lossy(&buf[..len]).to_uppercase();
    cls.starts_with("QT") || cls.contains("QWINDOW")
}

fn record_qt_caret_confirmation(fg_hwnd: HWND, hint: &EditableFocusHintInfo) {
    *QT_CARET_CONFIRMED.lock().unwrap() = Some(QtCaretConfirmation {
        fg_hwnd: fg_hwnd as isize,
        hint: hint.clone(),
        last_caret_at: Instant::now(),
    });
    log::debug!(
        "editable_focus qt caret confirmed: fg_hwnd={:?} editor={:?} pane={:?}",
        fg_hwnd,
        hint.editor,
        hint.pane,
    );
}

fn check_qt_caret_confirmed(fg_hwnd: HWND) -> Option<EditableFocusHintInfo> {
    let guard = QT_CARET_CONFIRMED.lock().unwrap();
    let conf = guard.as_ref()?;
    if conf.fg_hwnd != fg_hwnd as isize {
        return None;
    }
    let age = conf.last_caret_at.elapsed();
    if age > Duration::from_millis(QT_CARET_CONFIRMED_TTL_MS) {
        return None;
    }
    // Cursor proximity check: only reuse if cursor is within the confirmed
    // pane area (± margin).  This prevents false positives when the user
    // clicks a non-input area (e.g. contact list) in the same Qt window.
    let mut cursor_pt = winapi::shared::windef::POINT { x: 0, y: 0 };
    let cursor_ok = unsafe { winapi::um::winuser::GetCursorPos(&mut cursor_pt) } != 0;
    if cursor_ok && rect_has_area(conf.hint.pane) {
        if !point_near_rect(conf.hint.pane, cursor_pt.x, cursor_pt.y, 48) {
            return None;
        }
    }
    log::debug!(
        "editable_focus qt caret confirmed reuse: fg_hwnd={:?} editor={:?} age_ms={}",
        fg_hwnd,
        conf.hint.editor,
        age.as_millis(),
    );
    Some(conf.hint.clone())
}

/// Poll for an editable-focus hint. Returns `None` only on unrecoverable Windows API failure.
/// Returns a hint with `editable=false` when no editable element is focused.
pub fn get_editable_focus_hint(displays: &[DisplayInfo]) -> Option<EditableFocusHintInfo> {
    unsafe { get_editable_focus_hint_impl(displays) }
}

fn looks_like_virtual_proxy_rect(
    rect: (i32, i32, i32, i32),
    window: (i32, i32, i32, i32),
) -> bool {
    if !rect_has_area(rect) || !rect_has_area(window) {
        return false;
    }
    let width_threshold = (window.2 * 25 / 100).max(320);
    let lower_half = rect.1 + rect.3 / 2 >= window.1 + window.3 / 2;
    rect.3 <= 40 && rect.2 >= width_threshold && rect.2 >= rect.3 * 6 && lower_half
}

fn looks_like_visible_bottom_input_rect(
    rect: (i32, i32, i32, i32),
    window: (i32, i32, i32, i32),
    cursor: (i32, i32),
) -> bool {
    if !rect_has_area(rect) || !rect_has_area(window) {
        return false;
    }
    if !point_near_rect(rect, cursor.0, cursor.1, 24) {
        return false;
    }
    let lower_half = rect.1 + rect.3 / 2 >= window.1 + window.3 / 2;
    let left_inset = rect.0 - window.0;
    let right_inset = (window.0 + window.2) - (rect.0 + rect.2);
    let side_anchored = left_inset.abs() <= 80 || right_inset.abs() <= 80;
    let min_width = (window.2 * 25 / 100).max(420);
    let max_width = (window.2 * 70 / 100).max(min_width);
    lower_half
        && rect.3 <= 48
        && rect.2 >= min_width
        && rect.2 <= max_width
        && side_anchored
}

fn looks_like_bottom_input_band(
    rect: (i32, i32, i32, i32),
    window: (i32, i32, i32, i32),
) -> bool {
    if !rect_has_area(rect) || !rect_has_area(window) {
        return false;
    }
    let lower_half = rect.1 + rect.3 / 2 >= window.1 + window.3 / 2;
    rect.3 <= 56 && rect.2 >= (window.2 * 25 / 100).max(320) && lower_half
}

fn cursor_surface_explicitly_noneditable(class_name: &str) -> bool {
    let class_name = class_name.to_lowercase();
    class_name.contains("no-user-select")
}

fn virtual_proxy_still_matches_previous_hint(
    prev: &EditableFocusHintInfo,
    next: &EditableFocusHintInfo,
) -> bool {
    if !rect_has_area(prev.editor) || !rect_has_area(next.editor) {
        return false;
    }

    // Wide bottom proxy strips are only safe to reuse when they still line up with the
    // previously accepted synth editor horizontally. Otherwise split-pane apps like VS Code
    // can carry a stale pane hint across a click into a different bottom region.
    let prev_center_x = prev.editor.0 + prev.editor.2 / 2;
    let next_left = next.editor.0;
    let next_right = next.editor.0 + next.editor.2;
    let margin = (next.editor.2 / 10).clamp(48, 160);
    prev_center_x >= next_left - margin && prev_center_x <= next_right + margin
}

fn should_keep_previous_virtual_cursor_hint(
    prev: &EditableFocusHintInfo,
    next: &EditableFocusHintInfo,
) -> bool {
    next.editable
        && looks_like_virtual_proxy_rect(next.editor, next.window)
        && (!rect_has_area(next.pane) || next.pane == next.editor)
        && rect_has_area(prev.pane)
        && prev.pane != prev.editor
        && virtual_proxy_still_matches_previous_hint(prev, next)
}

fn should_reuse_sticky_for_blocked_surface(
    prev: &EditableFocusHintInfo,
    x: i32,
    y: i32,
) -> bool {
    if point_near_rect(prev.editor, x, y, 96) {
        return true;
    }
    rect_has_area(prev.pane)
        && prev.pane != prev.window
        && looks_like_bottom_input_band(prev.editor, prev.window)
        && point_near_rect(prev.pane, x, y, 24)
}

fn try_reuse_sticky_hint_on_hard_negative(fg_hwnd: HWND) -> Option<EditableFocusHintInfo> {
    let now = Instant::now();
    let hwnd_key = fg_hwnd as isize;
    let mut cursor_pt = winapi::shared::windef::POINT { x: 0, y: 0 };
    let cursor_ok = unsafe { winapi::um::winuser::GetCursorPos(&mut cursor_pt) } != 0;
    if !cursor_ok {
        return None;
    }

    let mut sticky = EDITABLE_FOCUS_STICKY.lock().unwrap();
    let prev = sticky.as_ref()?.clone();
    if prev.fg_hwnd != hwnd_key || prev.transient_expires_at <= now {
        return None;
    }
    if !should_reuse_sticky_for_blocked_surface(&prev.hint, cursor_pt.x, cursor_pt.y) {
        return None;
    }

    let mut reused = prev.hint.clone();
    reused.foreground_hwnd = hwnd_key; // Ensure hwnd is current
    *sticky = Some(EditableFocusStickyState {
        fg_hwnd: hwnd_key,
        hint: reused.clone(),
        expires_at: now + Duration::from_millis(EDITABLE_FOCUS_STICKY_MS),
        transient_expires_at: now + Duration::from_millis(EDITABLE_FOCUS_TRANSIENT_REUSE_MS),
    });
    log::debug!(
        "editable_focus hard negative reuse: fg={} cursor=({}, {}) editor={:?} pane={:?}",
        unsafe { hwnd_debug_summary(fg_hwnd) },
        cursor_pt.x,
        cursor_pt.y,
        reused.editor,
        reused.pane,
    );
    Some(reused)
}

fn finalize_editable_focus_hint(
    fg_hwnd: HWND,
    mut hint: EditableFocusHintInfo,
) -> EditableFocusHintInfo {
    let now = Instant::now();
    let hwnd_key = fg_hwnd as isize;
    // Always set foreground_hwnd for change detection
    hint.foreground_hwnd = hwnd_key;
    let mut sticky = EDITABLE_FOCUS_STICKY.lock().unwrap();

    if hint.editable {
        if let Some(prev) = sticky.as_ref() {
            if prev.fg_hwnd == hwnd_key
                && should_keep_previous_virtual_cursor_hint(&prev.hint, &hint)
            {
                log::debug!(
                    "editable_focus preserve prior virtual-cursor hint: incoming editor={:?}, pane={:?}, prev_editor={:?}, prev_pane={:?}",
                    hint.editor,
                    hint.pane,
                    prev.hint.editor,
                    prev.hint.pane
                );
                let mut reused = prev.hint.clone();
                reused.foreground_hwnd = hwnd_key; // Ensure hwnd is current
                *sticky = Some(EditableFocusStickyState {
                    fg_hwnd: hwnd_key,
                    hint: reused.clone(),
                    expires_at: now + Duration::from_millis(EDITABLE_FOCUS_STICKY_MS),
                    transient_expires_at: now
                        + Duration::from_millis(EDITABLE_FOCUS_TRANSIENT_REUSE_MS),
                });
                return reused;
            }
        }
        *sticky = Some(EditableFocusStickyState {
            fg_hwnd: hwnd_key,
            hint: hint.clone(),
            expires_at: now + Duration::from_millis(EDITABLE_FOCUS_STICKY_MS),
            transient_expires_at: now
                + Duration::from_millis(EDITABLE_FOCUS_TRANSIENT_REUSE_MS),
        });
        return hint;
    }

    if let Some(prev) = sticky.as_ref() {
        if prev.fg_hwnd == hwnd_key && prev.expires_at > now {
            log::debug!(
                "editable_focus sticky reuse: fg_hwnd={:?}, editor={:?}, pane={:?}, until={:?}",
                fg_hwnd,
                prev.hint.editor,
                prev.hint.pane,
                prev.expires_at
            );
            let mut reused = prev.hint.clone();
            reused.foreground_hwnd = hwnd_key;
            return reused;
        }

        if prev.transient_expires_at > now
            && unsafe { should_reuse_sticky_for_transient_fg(fg_hwnd) }
        {
            log::debug!(
                "editable_focus transient reuse: fg={}, prev_fg_hwnd={:?}, editor={:?}, pane={:?}, until={:?}",
                unsafe { hwnd_debug_summary(fg_hwnd) },
                prev.fg_hwnd,
                prev.hint.editor,
                prev.hint.pane,
                prev.transient_expires_at
            );
            let mut reused = prev.hint.clone();
            reused.foreground_hwnd = hwnd_key;
            return reused;
        }

        // Cursor-proximity reuse: the viewport on the mobile client is zoomed,
        // so taps often land just outside the actual editor rect (e.g., a few pixels
        // below the WeChat input box).  The caret won't be visible at that cursor
        // position, but the user clearly intends to re-enter the input.  If the cursor
        // is within 80px of the last-known editor AND we are within the 3-second
        // transient window, keep reporting editable=true so the keyboard reopens on
        // the first tap instead of requiring 2-4 retries.
        if prev.fg_hwnd == hwnd_key && prev.transient_expires_at > now {
            let mut cursor_pt = winapi::shared::windef::POINT { x: 0, y: 0 };
            // SAFETY: GetCursorPos is safe to call from any thread at any time.
            let cursor_ok = unsafe { winapi::um::winuser::GetCursorPos(&mut cursor_pt) } != 0;
            if cursor_ok && point_near_rect(prev.hint.editor, cursor_pt.x, cursor_pt.y, 80) {
                log::debug!(
                    "editable_focus proximity reuse: cursor=({},{}) editor={:?} until={:?}",
                    cursor_pt.x, cursor_pt.y, prev.hint.editor, prev.transient_expires_at
                );
                let mut reused = prev.hint.clone();
                reused.foreground_hwnd = hwnd_key;
                return reused;
            }
        }
    }

    *sticky = None;
    hint
}

unsafe fn rect_to_tuple(r: RECT) -> (i32, i32, i32, i32) {
    (r.left, r.top, (r.right - r.left).max(0), (r.bottom - r.top).max(0))
}

unsafe fn hwnd_screen_rect(hwnd: HWND) -> (i32, i32, i32, i32) {
    let mut r: RECT = mem::zeroed();
    if GetWindowRect(hwnd, &mut r) != 0 {
        rect_to_tuple(r)
    } else {
        (0, 0, 0, 0)
    }
}

unsafe fn hwnd_class_name(hwnd: HWND) -> String {
    if hwnd.is_null() {
        return "null".to_string();
    }
    let mut buf = [0u16; 256];
    let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) as usize;
    if len == 0 {
        return "?".to_string();
    }
    String::from_utf16_lossy(&buf[..len])
}

unsafe fn hwnd_window_text(hwnd: HWND) -> String {
    if hwnd.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 256];
    let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) as usize;
    if len == 0 {
        return String::new();
    }
    let mut text = String::from_utf16_lossy(&buf[..len]);
    text = text.replace('\r', " ").replace('\n', " ");
    if text.len() > 80 {
        text.truncate(80);
    }
    text
}

unsafe fn hwnd_debug_summary(hwnd: HWND) -> String {
    if hwnd.is_null() {
        return "null".to_string();
    }
    format!(
        "hwnd={:?} cls={} title={:?} rect={:?}",
        hwnd,
        hwnd_class_name(hwnd),
        hwnd_window_text(hwnd),
        hwnd_screen_rect(hwnd)
    )
}

unsafe fn startupdui_looks_transient(hwnd: HWND) -> bool {
    let rect = hwnd_screen_rect(hwnd);
    let title = hwnd_window_text(hwnd);
    if title.is_empty() || !rect_has_area(rect) {
        return true;
    }
    let area = i64::from(rect.2) * i64::from(rect.3);
    let max_edge = rect.2.max(rect.3);
    area <= 180_000 && max_edge <= 520
}

unsafe fn should_reuse_sticky_for_transient_fg(hwnd: HWND) -> bool {
    if hwnd.is_null() {
        return false;
    }

    match hwnd_class_name(hwnd).as_str() {
        "ForegroundStaging" | "Shell_TrayWnd" | "ControlCenterWindow"
        | "XamlExplorerHostIslandWindow" => return true,
        "StartupDui" => return startupdui_looks_transient(hwnd),
        _ => {}
    }

    let title = hwnd_window_text(hwnd);
    hwnd_class_name(hwnd) == "Chrome_WidgetWin_1"
        && title.contains("notification: command completed")
}

fn display_idx_for_point(displays: &[DisplayInfo], cx: i32, cy: i32) -> i32 {
    find_display_idx_for_point(displays, cx, cy)
        .map(|i| i as i32)
        .unwrap_or(0)
}

fn find_display_idx_for_point(displays: &[DisplayInfo], cx: i32, cy: i32) -> Option<usize> {
    displays.iter().position(|d| {
        cx >= d.x && cx < d.x + d.width && cy >= d.y && cy < d.y + d.height
    })
}

fn rect_has_area(rect: (i32, i32, i32, i32)) -> bool {
    rect.2 > 0 && rect.3 > 0
}

fn rects_intersect(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    let a_right = a.0 + a.2;
    let a_bottom = a.1 + a.3;
    let b_right = b.0 + b.2;
    let b_bottom = b.1 + b.3;
    a.0 < b_right && a_right > b.0 && a.1 < b_bottom && a_bottom > b.1
}

fn rect_intersection(
    a: (i32, i32, i32, i32),
    b: (i32, i32, i32, i32),
) -> Option<(i32, i32, i32, i32)> {
    let left = a.0.max(b.0);
    let top = a.1.max(b.1);
    let right = (a.0 + a.2).min(b.0 + b.2);
    let bottom = (a.1 + a.3).min(b.1 + b.3);
    let width = right - left;
    let height = bottom - top;
    if width > 0 && height > 0 {
        Some((left, top, width, height))
    } else {
        None
    }
}

fn rect_center(rect: (i32, i32, i32, i32)) -> Option<(i32, i32)> {
    if !rect_has_area(rect) {
        return None;
    }
    Some((rect.0 + rect.2 / 2, rect.1 + rect.3 / 2))
}

fn point_near_rect(rect: (i32, i32, i32, i32), x: i32, y: i32, margin: i32) -> bool {
    if !rect_has_area(rect) {
        return false;
    }
    let left = rect.0 - margin;
    let top = rect.1 - margin;
    let right = rect.0 + rect.2 + margin;
    let bottom = rect.1 + rect.3 + margin;
    x >= left && x <= right && y >= top && y <= bottom
}

fn normalize_rect_to_display(
    rect: (i32, i32, i32, i32),
    display: &DisplayInfo,
) -> (i32, i32, i32, i32) {
    if !rect_has_area(rect) {
        return rect;
    }
    (rect.0 - display.x, rect.1 - display.y, rect.2, rect.3)
}

fn normalize_editable_focus_hint_to_display(
    displays: &[DisplayInfo],
    mut hint: EditableFocusHintInfo,
) -> EditableFocusHintInfo {
    let display_idx = rect_center(hint.editor)
        .and_then(|(cx, cy)| find_display_idx_for_point(displays, cx, cy))
        .or_else(|| rect_center(hint.pane)
            .and_then(|(cx, cy)| find_display_idx_for_point(displays, cx, cy)))
        .or_else(|| rect_center(hint.window)
            .and_then(|(cx, cy)| find_display_idx_for_point(displays, cx, cy)))
        .or_else(|| rect_center(hint.caret)
            .and_then(|(cx, cy)| find_display_idx_for_point(displays, cx, cy)));

    let Some(display_idx) = display_idx else {
        return hint;
    };
    let Some(display) = displays.get(display_idx) else {
        return hint;
    };

    hint.caret = normalize_rect_to_display(hint.caret, display);
    hint.editor = normalize_rect_to_display(hint.editor, display);
    hint.window = normalize_rect_to_display(hint.window, display);
    hint.pane = normalize_rect_to_display(hint.pane, display);
    hint.display_idx = display_idx as i32;
    hint
}

fn caret_fallback_editor_rect(
    window: (i32, i32, i32, i32),
    caret: (i32, i32, i32, i32),
) -> (i32, i32, i32, i32) {
    if !rect_has_area(window) || !rect_has_area(caret) {
        return window;
    }

    let max_width = (window.2 - 16).max(caret.2.max(1));
    let target_width = (caret.3 * 24).clamp(320, max_width.min(900));
    let target_height = (caret.3 * 3).max(48).min((window.3 - 16).max(caret.3));
    let min_left = window.0 + 8;
    let max_left = (window.0 + window.2 - target_width - 8).max(min_left);
    let left = (caret.0 - target_width / 2).clamp(min_left, max_left);
    let min_top = window.1 + 8;
    let max_top = (window.1 + window.3 - target_height - 8).max(min_top);
    let top = (caret.1 - target_height / 2).clamp(min_top, max_top);

    (left, top, target_width, target_height)
}

/// Compute the pane rectangle for split-layout applications (Phase A).
/// If the editor is significantly narrower than the window, assume a split layout
/// (e.g. WeChat chat, Notion doc area) and expand pane from editor.left to window.right.
/// Returns the editor rect as fallback if no meaningful split layout is detected.
fn compute_pane_rect(
    editor: (i32, i32, i32, i32),
    window: (i32, i32, i32, i32),
) -> (i32, i32, i32, i32) {
    if !rect_has_area(editor) || !rect_has_area(window) {
        return editor;
    }

    // Split-view heuristic: editor takes up less than 60% of window width
    let is_split_layout = editor.2 > 0 && editor.2 < (window.2 * 60 / 100);
    if !is_split_layout {
        return editor;
    }

    // Expand pane: from editor.left to window.right, full window height
    let w_right = window.0 + window.2;
    let pane_width = w_right - editor.0;

    // Must expand at least 20% beyond editor width
    if pane_width < (editor.2 * 120 / 100) {
        return editor;
    }
    // Minimum 400 pixels
    if pane_width < 400 {
        return editor;
    }

    (editor.0, window.1, pane_width, window.3)
}

const AWT_CODE_WORK_LEFT_MARGIN_PCT: i32 = 12;
const AWT_CODE_WORK_RIGHT_MARGIN_PCT: i32 = 5;
const AWT_CODE_WORK_TOP_MARGIN_PCT: i32 = 15;
const AWT_CODE_WORK_BOTTOM_MARGIN_PCT: i32 = 16;
const AWT_CODE_WORK_LEFT_MARGIN_MIN: i32 = 120;
const AWT_CODE_WORK_LEFT_MARGIN_MAX: i32 = 260;
const AWT_CODE_WORK_RIGHT_MARGIN_MIN: i32 = 48;
const AWT_CODE_WORK_RIGHT_MARGIN_MAX: i32 = 140;
const AWT_CODE_WORK_TOP_MARGIN_MIN: i32 = 96;
const AWT_CODE_WORK_TOP_MARGIN_MAX: i32 = 220;
const AWT_CODE_WORK_BOTTOM_MARGIN_MIN: i32 = 120;
const AWT_CODE_WORK_BOTTOM_MARGIN_MAX: i32 = 260;
const AWT_CODE_WORK_MIN_WIDTH: i32 = 200;
const AWT_CODE_WORK_MIN_HEIGHT: i32 = 160;
const AWT_SYNTH_EDITOR_WIDTH: i32 = 220;
const AWT_SYNTH_EDITOR_HEIGHT: i32 = 36;

fn title_contains_code_file_suffix(title: &str, ext: &str) -> bool {
    let mut offset = 0usize;
    while let Some(found) = title[offset..].find(ext) {
        let idx = offset + found;
        let next_char = title[idx + ext.len()..].chars().next();
        if next_char
            .map(|ch| ch.is_whitespace() || matches!(ch, ']' | ')' | ':' | '-' | '\u{2013}' | '\u{2014}'))
            .unwrap_or(true)
        {
            return true;
        }
        offset = idx + ext.len();
    }
    false
}

fn window_title_looks_like_code_file(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    [
        ".ets", ".ts", ".tsx", ".js", ".jsx", ".java", ".kt", ".kts", ".rs",
        ".py", ".go", ".cpp", ".hpp", ".json", ".xml", ".yaml", ".yml", ".md",
    ]
    .iter()
    .any(|ext| title_contains_code_file_suffix(&lower, ext))
}

fn awt_code_work_area_rect(window: (i32, i32, i32, i32)) -> Option<(i32, i32, i32, i32)> {
    if !rect_has_area(window) {
        return None;
    }

    let left_margin = (window.2 * AWT_CODE_WORK_LEFT_MARGIN_PCT / 100)
        .clamp(AWT_CODE_WORK_LEFT_MARGIN_MIN, AWT_CODE_WORK_LEFT_MARGIN_MAX);
    let right_margin = (window.2 * AWT_CODE_WORK_RIGHT_MARGIN_PCT / 100)
        .clamp(AWT_CODE_WORK_RIGHT_MARGIN_MIN, AWT_CODE_WORK_RIGHT_MARGIN_MAX);
    let top_margin = (window.3 * AWT_CODE_WORK_TOP_MARGIN_PCT / 100)
        .clamp(AWT_CODE_WORK_TOP_MARGIN_MIN, AWT_CODE_WORK_TOP_MARGIN_MAX);
    let bottom_margin = (window.3 * AWT_CODE_WORK_BOTTOM_MARGIN_PCT / 100)
        .clamp(AWT_CODE_WORK_BOTTOM_MARGIN_MIN, AWT_CODE_WORK_BOTTOM_MARGIN_MAX);
    let width = window.2 - left_margin - right_margin;
    let height = window.3 - top_margin - bottom_margin;

    if width <= AWT_CODE_WORK_MIN_WIDTH || height <= AWT_CODE_WORK_MIN_HEIGHT {
        return None;
    }

    Some((
        window.0 + left_margin,
        window.1 + top_margin,
        width,
        height,
    ))
}

fn point_in_rect(rect: (i32, i32, i32, i32), x: i32, y: i32) -> bool {
    rect_has_area(rect)
        && x >= rect.0
        && x <= rect.0 + rect.2
        && y >= rect.1
        && y <= rect.1 + rect.3
}

fn synth_editor_rect_from_cursor(
    window: (i32, i32, i32, i32),
    cursor: (i32, i32),
) -> Option<(i32, i32, i32, i32)> {
    if !rect_has_area(window) {
        return None;
    }

    let target_width = AWT_SYNTH_EDITOR_WIDTH;
    let target_height = AWT_SYNTH_EDITOR_HEIGHT;
    let min_left = window.0 + 8;
    let max_left = (window.0 + window.2 - target_width - 8).max(min_left);
    let min_top = window.1 + 8;
    let max_top = (window.1 + window.3 - target_height - 8).max(min_top);

    Some((
        (cursor.0 - target_width / 2).clamp(min_left, max_left),
        (cursor.1 - target_height / 2).clamp(min_top, max_top),
        target_width,
        target_height,
    ))
}

fn apply_awt_code_file_override(
    host_class: &str,
    host_title: &str,
    host_rect: (i32, i32, i32, i32),
    cursor_pos: Option<(i32, i32)>,
) -> Option<(
    bool,
    i32,
    Option<(i32, i32, i32, i32)>,
    Option<(i32, i32, i32, i32)>,
)> {
    if !host_class.eq_ignore_ascii_case("SunAwtFrame") || !window_title_looks_like_code_file(host_title) {
        return None;
    }

    let cursor = cursor_pos?;
    let pane = awt_code_work_area_rect(host_rect)?;
    if !point_in_rect(pane, cursor.0, cursor.1) {
        return None;
    }

    let editor = synth_editor_rect_from_cursor(host_rect, cursor)?;
    log::debug!(
        "editable_focus awt code-file override: title={:?} cursor=({},{}) editor={:?} pane={:?}",
        host_title,
        cursor.0,
        cursor.1,
        editor,
        pane
    );
    Some((true, 3, Some(editor), Some(pane)))
}

/// Classify a window class name into (editable, content_kind).
/// Returns `None` if the class is not a recognised editable class and
/// we should fall through to UI Automation.
unsafe fn classify_by_class(hwnd: HWND) -> Option<(bool, i32)> {
    let mut buf = [0u16; 256];
    let len = winapi::um::winuser::GetClassNameW(hwnd, buf.as_mut_ptr(), 256) as usize;
    if len == 0 {
        return None;
    }
    let cls = String::from_utf16_lossy(&buf[..len]).to_uppercase();

    let style = winapi::um::winuser::GetWindowLongW(hwnd, winapi::um::winuser::GWL_STYLE) as u32;
    let es_readonly = style & 0x0800 != 0; // ES_READONLY
    let es_multiline = style & 0x0004 != 0; // ES_MULTILINE
    let es_password = style & 0x0020 != 0; // ES_PASSWORD

    if cls == "EDIT" {
        if es_readonly || es_password {
            return Some((false, 0));
        }
        return Some((true, if es_multiline { 2 } else { 1 }));
    }
    if cls.starts_with("RICHEDIT") {
        if es_readonly {
            return Some((false, 0));
        }
        return Some((true, if es_multiline { 2 } else { 1 }));
    }
    // Scintilla — code editor control used by Notepad++, SciTE, etc.
    if cls.starts_with("SCINTILLA") {
        return Some((true, 3));
    }
    // Chromium/Electron/CEF/WPF hosts need deeper UIA focus probing.
    // Treating the host HWND as the editor loses the real input rectangle.
    if cls.contains("CHROME_WIDGET") || cls.contains("CEFBROWSER")
        || cls == "HWNDSOURCE" || cls.starts_with("WPF") {
        return None;
    }
    // Unknown Win32 class — fall through to UIA
    None
}

/// Attempt to detect editability via UI Automation (COM, apartment-per-call).
/// Returns (editable, content_kind, editor_rect_option, pane_rect_option).
unsafe fn probe_via_uia(
    hwnd: HWND,
) -> (
    bool,
    i32,
    Option<(i32, i32, i32, i32)>,
    Option<(i32, i32, i32, i32)>,
) {
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
        UIA_IsKeyboardFocusablePropertyId, UIA_ControlTypePropertyId,
        UIA_ValueIsReadOnlyPropertyId, UIA_EditControlTypeId, UIA_DocumentControlTypeId,
        UIA_ValuePatternId, UIA_TextPatternId, UIA_WindowControlTypeId,
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, CoCreateInstance, COINIT_APARTMENTTHREADED, CLSCTX_INPROC_SERVER,
    };
    use windows::Win32::System::Variant::{VARIANT, VT_BOOL, VT_I4, VT_INT};

    // Extract bool from a VARIANT value using raw union field access.
    let variant_as_bool = |v: &VARIANT| -> bool {
        let inner = &v.Anonymous.Anonymous;  // &ManuallyDrop<VARIANT_0_0>
        if inner.vt == VT_BOOL {
            inner.Anonymous.boolVal.0 != 0  // VARIANT_BOOL(i16), 0=false, -1=true
        } else {
            false
        }
    };

    // Extract i32 from a VARIANT value.
    let variant_as_i32 = |v: &VARIANT| -> i32 {
        let inner = &v.Anonymous.Anonymous;
        if inner.vt == VT_I4 || inner.vt == VT_INT {
            inner.Anonymous.lVal
        } else {
            0
        }
    };

    // Initialize COM for this call — we immediately uninit after.
    let hr_init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    let com_ok = hr_init.is_ok() || hr_init.0 == 0x00000001_u32 as i32; // S_FALSE = already init

    if !com_ok {
        return (false, 0, None, None);
    }

    let result = (|| -> windows::core::Result<(
        bool,
        i32,
        Option<(i32, i32, i32, i32)>,
        Option<(i32, i32, i32, i32)>,
    )> {
        let host_rect = hwnd_screen_rect(hwnd);
        let host_class = hwnd_class_name(hwnd);
        let host_title = hwnd_window_text(hwnd);
        let is_chromium_host = host_class.contains("CHROME_WIDGET")
            || host_class.contains("Chrome_Widget")
            || host_class.contains("Chrome_RenderWidget")
            || host_class.contains("CEFBROWSER")
            || host_class == "HWNDSOURCE"
            || host_class.starts_with("WPF");
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let control_walker: IUIAutomationTreeWalker = automation.ControlViewWalker()?;
        let raw_walker: IUIAutomationTreeWalker = automation.RawViewWalker()?;
        let mut content_pane_candidate: Option<(i32, i32, i32, i32)> = None;
        let mut cursor_surface_blocks_virtual_override = false;
        let mut cursor_pt: winapi::shared::windef::POINT = mem::zeroed();
        let cursor_pos = if winapi::um::winuser::GetCursorPos(&mut cursor_pt) != 0 {
            Some((cursor_pt.x, cursor_pt.y))
        } else {
            None
        };
        let cursor_matches_candidate = |rect: Option<(i32, i32, i32, i32)>| -> bool {
            match (rect, cursor_pos) {
                (Some(candidate_rect), Some((x, y))) => point_near_rect(candidate_rect, x, y, 96),
                _ => true,
            }
        };
        let element_class_name_lower = |element: &IUIAutomationElement| -> String {
            element
                .CurrentClassName()
                .map(|s| s.to_string().to_lowercase())
                .unwrap_or_default()
        };
        let element_control_type = |element: &IUIAutomationElement| -> i32 {
            element.CurrentControlType().map(|v| v.0).unwrap_or_default()
        };
        let element_visible_rect = |element: &IUIAutomationElement| -> Option<(i32, i32, i32, i32)> {
            element
                .CurrentBoundingRectangle()
                .ok()
                .map(|r| (r.left, r.top, (r.right - r.left).max(0), (r.bottom - r.top).max(0)))
                .and_then(|r| rect_intersection(r, host_rect))
        };
        let find_code_pane_ancestor = |
            element: &IUIAutomationElement,
            source: &str,
        | -> windows::core::Result<Option<(i32, i32, i32, i32)>> {
            let mut current = element.clone();
            for depth in 0..=8 {
                let class_name = element_class_name_lower(&current);
                let control_type = element_control_type(&current);
                let rect = element_visible_rect(&current);
                let looks_like_monaco = class_name.contains("monaco");
                let looks_like_document = control_type == UIA_DocumentControlTypeId.0;
                if let Some(candidate_rect) = rect {
                    let wide_enough = candidate_rect.2 >= (host_rect.2 * 28 / 100).max(420);
                    let tall_enough = candidate_rect.3 >= (host_rect.3 * 35 / 100).max(220);
                    if (looks_like_monaco || looks_like_document) && wide_enough && tall_enough {
                        log::debug!(
                            "editable_focus {} code pane candidate depth={} class={} rect={:?}",
                            source,
                            depth,
                            class_name,
                            candidate_rect
                        );
                        return Ok(Some(candidate_rect));
                    }
                }
                let Ok(parent) = control_walker.GetParentElement(&current) else {
                    break;
                };
                current = parent;
            }
            Ok(None)
        };
        let merge_code_candidate = |
            source: &str,
            candidate: (bool, i32, Option<(i32, i32, i32, i32)>),
            code_pane: Option<(i32, i32, i32, i32)>,
        | -> (bool, i32, Option<(i32, i32, i32, i32)>, Option<(i32, i32, i32, i32)>) {
            let (editable, content_kind, editor_rect) = candidate;
            if !editable {
                return (editable, content_kind, editor_rect, None);
            }
            let Some(code_pane_rect) = code_pane else {
                return (editable, content_kind, editor_rect, None);
            };
            let candidate_rect = editor_rect.unwrap_or(code_pane_rect);
            if looks_like_bottom_input_band(candidate_rect, host_rect) {
                return (editable, content_kind, editor_rect, None);
            }
            log::debug!(
                "editable_focus {} upgrade code-pane: editor={:?} pane={:?}",
                source,
                editor_rect,
                code_pane_rect
            );
            (true, 3, editor_rect.or(Some(code_pane_rect)), Some(code_pane_rect))
        };
        let awt_host_needs_descendant_probe = |element: &IUIAutomationElement| -> bool {
            element_class_name_lower(element) == "sunawtframe"
                && element_control_type(element) == UIA_WindowControlTypeId.0
        };

        let classify_element = |element: &IUIAutomationElement|
         -> windows::core::Result<(bool, i32, Option<(i32, i32, i32, i32)>)> {
            let class_name = element_class_name_lower(element);
            let ctrl_type_var = element.GetCurrentPropertyValue(UIA_ControlTypePropertyId)?;
            let ctrl_type: i32 = variant_as_i32(&ctrl_type_var);
            let has_value = element.GetCurrentPattern(UIA_ValuePatternId).is_ok();
            let has_text = element.GetCurrentPattern(UIA_TextPatternId).is_ok();
            let contenteditable_class = class_name.contains("contenteditable")
                || class_name.contains("content-editable")
                || class_name.contains("whencontenteditable");
            let focusable = element.GetCurrentPropertyValue(UIA_IsKeyboardFocusablePropertyId)?;
            let allow_nonfocusable_edit_like = (
                ctrl_type == UIA_EditControlTypeId.0
                    || ctrl_type == UIA_DocumentControlTypeId.0
                    || contenteditable_class
            ) && (has_value || has_text || ctrl_type == UIA_EditControlTypeId.0);
            if !variant_as_bool(&focusable) && !allow_nonfocusable_edit_like {
                return Ok((false, 0, None));
            }
            if !variant_as_bool(&focusable) && allow_nonfocusable_edit_like {
                log::debug!(
                    "editable_focus accept nonfocusable edit-like class={} ctrl_type={} value={} text={}",
                    class_name,
                    ctrl_type,
                    has_value,
                    has_text
                );
            }

            let readonly_var = element.GetCurrentPropertyValue(UIA_ValueIsReadOnlyPropertyId)?;
            if variant_as_bool(&readonly_var) {
                return Ok((false, 0, None));
            }

            let content_kind = if ctrl_type == UIA_EditControlTypeId.0 {
                if has_text { 2 } else { 1 }
            } else if ctrl_type == UIA_DocumentControlTypeId.0 {
                if !has_value && !has_text {
                    return Ok((false, 0, None));
                }
                3
            } else {
                if !has_value && !has_text {
                    return Ok((false, 0, None));
                }
                2
            };

            let bounds = element.CurrentBoundingRectangle()?;
            let raw_rect = (
                bounds.left,
                bounds.top,
                (bounds.right - bounds.left).max(0),
                (bounds.bottom - bounds.top).max(0),
            );
            let rect = rect_intersection(raw_rect, host_rect);

            Ok((true, content_kind, rect))
        };

        let find_descendant_candidate_near_cursor = |
            root: &IUIAutomationElement,
            source: &str,
            tree_walker: &IUIAutomationTreeWalker,
            walker_name: &str,
        | -> windows::core::Result<Option<(
            bool,
            i32,
            Option<(i32, i32, i32, i32)>,
            Option<(i32, i32, i32, i32)>,
        )>> {
            let Some((cursor_x, cursor_y)) = cursor_pos else {
                return Ok(None);
            };

            let mut stack: Vec<(IUIAutomationElement, usize)> = Vec::new();
            let first_child = match tree_walker.GetFirstChildElement(root) {
                Ok(child) => child,
                Err(err) => {
                    log::debug!(
                        "editable_focus {} descendant probe {} no-child: root_class={} err={:?}",
                        source,
                        walker_name,
                        element_class_name_lower(root),
                        err
                    );
                    return Ok(None);
                }
            };
            stack.push((first_child, 1));

            let mut visited = 0usize;
            let mut near_cursor_nodes = 0usize;
            while let Some((node, depth)) = stack.pop() {
                visited += 1;
                if visited > 192 {
                    break;
                }

                let node_rect = element_visible_rect(&node);
                let rect_near_cursor = node_rect
                    .map(|rect| point_near_rect(rect, cursor_x, cursor_y, 48))
                    .unwrap_or(depth <= 2);

                if rect_near_cursor {
                    near_cursor_nodes += 1;
                    if walker_name == "raw" && near_cursor_nodes <= 6 {
                        let class_name = element_class_name_lower(&node);
                        let ctrl_type = element_control_type(&node);
                        let focusable = node
                            .GetCurrentPropertyValue(UIA_IsKeyboardFocusablePropertyId)
                            .ok()
                            .map(|value| variant_as_bool(&value))
                            .unwrap_or(false);
                        let has_value = node.GetCurrentPattern(UIA_ValuePatternId).is_ok();
                        let has_text = node.GetCurrentPattern(UIA_TextPatternId).is_ok();
                        log::debug!(
                            "editable_focus {} descendant inspect {} depth={} class={} ctrl_type={} focusable={} value={} text={} rect={:?}",
                            source,
                            walker_name,
                            depth,
                            class_name,
                            ctrl_type,
                            focusable,
                            has_value,
                            has_text,
                            node_rect
                        );
                    }
                    let candidate = classify_element(&node)?;
                    if candidate.0 {
                        if let Some(candidate_rect) = candidate.2 {
                            if point_near_rect(candidate_rect, cursor_x, cursor_y, 48) {
                                let class_name = element_class_name_lower(&node);
                                let code_pane = find_code_pane_ancestor(&node, source)?;
                                let upgraded = merge_code_candidate(source, candidate, code_pane);
                                log::debug!(
                                    "editable_focus {} descendant hit depth={} class={} rect={:?} kind={}",
                                    source,
                                    depth,
                                    class_name,
                                    candidate_rect,
                                    upgraded.1
                                );
                                return Ok(Some(upgraded));
                            }
                        }
                    }

                    if depth < 6 {
                        if let Ok(first_child) = tree_walker.GetFirstChildElement(&node) {
                            stack.push((first_child, depth + 1));
                        }
                    }
                }

                if let Ok(next_sibling) = tree_walker.GetNextSiblingElement(&node) {
                    stack.push((next_sibling, depth));
                }
            }

            log::debug!(
                "editable_focus {} descendant probe {} miss: visited={} near_cursor={} root_class={}",
                source,
                walker_name,
                visited,
                near_cursor_nodes,
                element_class_name_lower(root)
            );

            Ok(None)
        };

        let classify_element_or_ancestors = |
            element: &IUIAutomationElement,
            source: &str,
        | -> windows::core::Result<(bool, i32, Option<(i32, i32, i32, i32)>)> {
            let mut current = element.clone();
            for depth in 0..=8 {
                let candidate = classify_element(&current)?;
                if candidate.0 {
                    if depth > 0 {
                        log::debug!(
                            "editable_focus {} ancestor hit depth={}",
                            source,
                            depth
                        );
                    }
                    return Ok(candidate);
                }
                let Ok(parent) = control_walker.GetParentElement(&current) else {
                    break;
                };
                current = parent;
            }
            Ok((false, 0, None))
        };

        let find_awt_descendant_candidate = |
            element: &IUIAutomationElement,
            source: &str,
        | -> windows::core::Result<Option<(
            bool,
            i32,
            Option<(i32, i32, i32, i32)>,
            Option<(i32, i32, i32, i32)>,
        )>> {
            if let Some(candidate) = find_descendant_candidate_near_cursor(
                element,
                source,
                &control_walker,
                "control",
            )? {
                return Ok(Some(candidate));
            }
            match find_descendant_candidate_near_cursor(element, source, &raw_walker, "raw") {
                Ok(candidate) => Ok(candidate),
                Err(err) => {
                    log::debug!(
                        "editable_focus {} descendant probe raw error: root_class={} err={:?}",
                        source,
                        element_class_name_lower(element),
                        err
                    );
                    Ok(None)
                }
            }
        };

        // In remote desktop, cursor position IS the user's click intent.
        // Try ElementFromPoint first — it's the most accurate signal for what
        // the user just interacted with in Electron/Chromium apps where UIA
        // focus is notoriously unreliable.
        log::debug!("editable_focus probe: cursor={:?} host_hwnd={:?}", cursor_pos, hwnd);
        if let Some((cursor_x, cursor_y)) = cursor_pos {
            let pt = windows::Win32::Foundation::POINT { x: cursor_x, y: cursor_y };
            // Use ElementFromPoint as a hint. IMPORTANT: errors here must NOT propagate via ?
            // because Electron/Chromium often causes COM errors in the UIA parent walker.
            // Failing here would prevent GetFocusedElement from ever running.
            'efp: {
                let cursor_element = match automation.ElementFromPoint(pt) {
                    Ok(e) => e,
                    Err(e) => {
                        log::debug!("editable_focus ElementFromPoint error at ({},{}) err={:?}", cursor_x, cursor_y, e);
                        break 'efp;
                    }
                };
                let raw_class = cursor_element.CurrentClassName().map(|s| s.to_string()).unwrap_or_default();
                let raw_role = cursor_element.CurrentControlType().unwrap_or_default();
                let raw_rect = cursor_element.CurrentBoundingRectangle().ok().map(|r| (r.left, r.top, r.right - r.left, r.bottom - r.top));
                if is_chromium_host {
                    if let Some(raw) = raw_rect
                        .and_then(|r| rect_intersection(r, host_rect))
                        .filter(|r| point_near_rect(*r, cursor_x, cursor_y, 24))
                        .filter(|r| r.2 >= (host_rect.2 * 20 / 100).max(320))
                        .filter(|r| r.3 >= (host_rect.3 * 35 / 100).max(220))
                    {
                        content_pane_candidate = Some(raw);
                        log::debug!(
                            "editable_focus ElementFromPoint pane candidate: cursor=({},{}) class={} role={} pane={:?}",
                            cursor_x,
                            cursor_y,
                            raw_class,
                            raw_role.0,
                            raw
                        );
                    }
                }
                let cursor_candidate = match classify_element_or_ancestors(&cursor_element, "ElementFromPoint") {
                    Ok(c) => c,
                    Err(e) => {
                        log::debug!("editable_focus ElementFromPoint classify error at ({},{}) class={} err={:?}", cursor_x, cursor_y, raw_class, e);
                        break 'efp;
                    }
                };
                let cursor_code_pane = match find_code_pane_ancestor(&cursor_element, "ElementFromPoint") {
                    Ok(pane) => pane,
                    Err(e) => {
                        log::debug!(
                            "editable_focus ElementFromPoint code pane error at ({},{}) class={} err={:?}",
                            cursor_x,
                            cursor_y,
                            raw_class,
                            e
                        );
                        None
                    }
                };
                log::debug!(
                    "editable_focus ElementFromPoint at ({},{}) → editable={} class={} role={} raw_rect={:?} result_rect={:?}",
                    cursor_x, cursor_y, cursor_candidate.0, raw_class, raw_role.0, raw_rect, cursor_candidate.2
                );
                if !cursor_candidate.0 && cursor_surface_explicitly_noneditable(&raw_class) {
                    cursor_surface_blocks_virtual_override = true;
                    log::debug!(
                        "editable_focus ElementFromPoint explicit noneditable surface: cursor=({},{}) class={} role={} raw_rect={:?}",
                        cursor_x,
                        cursor_y,
                        raw_class,
                        raw_role.0,
                        raw_rect
                    );
                }
                if !cursor_candidate.0 && awt_host_needs_descendant_probe(&cursor_element) {
                    let descendant_candidate =
                        find_awt_descendant_candidate(&cursor_element, "ElementFromPoint")?;
                    if let Some(descendant_candidate) = descendant_candidate {
                        let pane_override = descendant_candidate.3.or(content_pane_candidate);
                        return Ok((
                            descendant_candidate.0,
                            descendant_candidate.1,
                            descendant_candidate.2,
                            pane_override,
                        ));
                    }
                }
                if cursor_candidate.0 {
                    let proxy_visible_bottom_input = cursor_candidate.2
                        .zip(cursor_pos)
                        .map(|(r, c)| looks_like_visible_bottom_input_rect(r, host_rect, c))
                        .unwrap_or(false);
                    // Extra check: the returned element's rect must cover the cursor (or nearly so).
                    // Electron/Chromium sometimes returns elements whose BoundingRectangle is
                    // far from the query point — this indicates a phantom/misreported element.
                    let bounds_ok = cursor_candidate.2.map(|r| {
                        let margin = 120i32;
                        cursor_x >= r.0 - margin && cursor_x <= r.0 + r.2 + margin &&
                        cursor_y >= r.1 - margin && cursor_y <= r.1 + r.3 + margin
                    }).unwrap_or(true);
                    if bounds_ok && (!looks_like_virtual_proxy_rect(cursor_candidate.2.unwrap_or_default(), host_rect) || proxy_visible_bottom_input) {
                        log::debug!("editable_focus ElementFromPoint accepted at ({},{})", cursor_x, cursor_y);
                        let upgraded = merge_code_candidate("ElementFromPoint", cursor_candidate, cursor_code_pane);
                        let pane_override = upgraded.3.or(content_pane_candidate);
                        return Ok((
                            upgraded.0,
                            upgraded.1,
                            upgraded.2,
                            pane_override,
                        ));
                    } else {
                        log::debug!(
                            "editable_focus ElementFromPoint bounds/proxy mismatch: cursor=({},{}) result_rect={:?}",
                            cursor_x, cursor_y, cursor_candidate.2
                        );
                    }
                }
            }
        }

        // Fallback: GetFocusedElement. In Electron/Chromium apps, this may point
        // to an internal element that differs from where the cursor just clicked.
        let mut fallback_candidate: Option<(bool, i32, Option<(i32, i32, i32, i32)>)> = None;
        if let Ok(focused_element) = automation.GetFocusedElement() {
            let focused = classify_element_or_ancestors(&focused_element, "GetFocusedElement")?;
            let focused_code_pane = find_code_pane_ancestor(&focused_element, "GetFocusedElement")?;
            if focused.0 {
                fallback_candidate = Some(focused.clone());
                if let Some(rect) = focused.2 {
                    let proxy_visible_bottom_input = cursor_pos
                        .map(|cursor| looks_like_visible_bottom_input_rect(rect, host_rect, cursor))
                        .unwrap_or(false);
                    if rect_has_area(rect) && rects_intersect(rect, host_rect) {
                        if cursor_matches_candidate(focused.2)
                            && (!looks_like_virtual_proxy_rect(rect, host_rect) || proxy_visible_bottom_input)
                        {
                            let upgraded = merge_code_candidate("GetFocusedElement", focused, focused_code_pane);
                            let pane_override = if upgraded.3.is_some() {
                                upgraded.3
                            } else if proxy_visible_bottom_input {
                                Some(compute_pane_rect(rect, host_rect))
                            } else {
                                content_pane_candidate
                            };
                            return Ok((upgraded.0, upgraded.1, upgraded.2, pane_override));
                        }
                        log::debug!(
                            "editable_focus GetFocusedElement cursor mismatch/proxy: cursor={:?}, rect={:?}, host_rect={:?}",
                            cursor_pos,
                            focused.2,
                            host_rect
                        );
                    }
                } else {
                    return Ok((focused.0, focused.1, focused.2, content_pane_candidate));
                }
            } else if awt_host_needs_descendant_probe(&focused_element) {
                let descendant_candidate =
                    find_awt_descendant_candidate(&focused_element, "GetFocusedElement")?;
                if let Some(descendant_candidate) = descendant_candidate {
                    return Ok((
                        descendant_candidate.0,
                        descendant_candidate.1,
                        descendant_candidate.2,
                        descendant_candidate.3.or(content_pane_candidate),
                    ));
                }
            }
        }

        let hwnd_win = windows::Win32::Foundation::HWND(hwnd as *mut _);
        let element: IUIAutomationElement = automation.ElementFromHandle(hwnd_win)?;
        let handle_candidate = classify_element_or_ancestors(&element, "ElementFromHandle")?;
        let handle_code_pane = find_code_pane_ancestor(&element, "ElementFromHandle")?;
        if handle_candidate.0 {
            fallback_candidate = Some(handle_candidate.clone());
            if let Some(rect) = handle_candidate.2 {
                let proxy_visible_bottom_input = cursor_pos
                    .map(|cursor| looks_like_visible_bottom_input_rect(rect, host_rect, cursor))
                    .unwrap_or(false);
                if cursor_matches_candidate(handle_candidate.2)
                    && (!looks_like_virtual_proxy_rect(rect, host_rect) || proxy_visible_bottom_input)
                {
                    let upgraded = merge_code_candidate("ElementFromHandle", handle_candidate, handle_code_pane);
                    let pane_override = if upgraded.3.is_some() {
                        upgraded.3
                    } else if proxy_visible_bottom_input {
                        Some(compute_pane_rect(rect, host_rect))
                    } else {
                        content_pane_candidate
                    };
                    return Ok((
                        upgraded.0,
                        upgraded.1,
                        upgraded.2,
                        pane_override,
                    ));
                }
            } else if cursor_matches_candidate(handle_candidate.2) {
                let upgraded = merge_code_candidate("ElementFromHandle", handle_candidate, handle_code_pane);
                return Ok((
                    upgraded.0,
                    upgraded.1,
                    upgraded.2,
                    upgraded.3.or(content_pane_candidate),
                ));
            }
            log::debug!(
                "editable_focus ElementFromHandle cursor mismatch/proxy: cursor={:?}, rect={:?}, host_rect={:?}",
                cursor_pos,
                handle_candidate.2,
                host_rect
            );
        }

        // Final fallback: we have an editable element but its rect doesn't match cursor.
        // This is the "virtual cursor" pattern used by apps like Notion: the UIA-visible
        // text input is a hidden proxy element at a fixed/off-screen position, while the
        // actual visible typing area is where the user clicked.
        // Strategy: if ElementFromPoint exposed a large visible content container, use it
        // as pane and synthesize a narrow editor around the cursor. This avoids the mobile
        // side falling back to editor-bottom-band on Electron proxy textareas.
        let final_result = fallback_candidate.unwrap_or((false, 0, None));
        if cursor_surface_blocks_virtual_override && !final_result.0 {
            log::debug!(
                "editable_focus hard negative: explicit noneditable cursor surface with no editable candidate"
            );
            return Ok((false, -1, None, None));
        }
        if final_result.0 {
            if let (Some(rect), Some((cx, cy))) = (final_result.2, cursor_pos) {
                let rect_is_proxy = looks_like_virtual_proxy_rect(rect, host_rect);
                let proxy_visible_bottom_input = looks_like_visible_bottom_input_rect(rect, host_rect, (cx, cy));
                if cursor_surface_blocks_virtual_override
                    && ((rect_is_proxy && !proxy_visible_bottom_input)
                        || !point_near_rect(rect, cx, cy, 96))
                {
                    log::debug!(
                        "editable_focus suppress virtual-cursor override: cursor=({},{}) fallback_rect={:?} pane={:?}",
                        cx,
                        cy,
                        rect,
                        content_pane_candidate
                    );
                    return Ok((false, -1, None, None));
                }
                if rect_is_proxy && proxy_visible_bottom_input {
                    let pane_override = Some(compute_pane_rect(rect, host_rect));
                    log::debug!(
                        "editable_focus visible-bottom-input accept: cursor=({},{}) rect={:?} pane={:?}",
                        cx,
                        cy,
                        rect,
                        pane_override
                    );
                    return Ok((true, final_result.1, Some(rect), pane_override));
                }
                if rect_is_proxy || !point_near_rect(rect, cx, cy, 96) {
                    // The editable element's rect is far from where the user clicked.
                    // Use cursor position as the synthetic editor location and preserve
                    // any visible content container that ElementFromPoint surfaced.
                    let synth_w = 200i32;
                    let synth_h = 32i32;
                    let synth_rect = Some((cx - synth_w / 2, cy - synth_h / 2, synth_w, synth_h));
                    log::debug!(
                        "editable_focus virtual-cursor override: cursor=({},{}) fallback_rect={:?} proxy={} pane={:?} → synth={:?}",
                        cx,
                        cy,
                        rect,
                        rect_is_proxy,
                        content_pane_candidate,
                        synth_rect
                    );
                    return Ok((true, final_result.1, synth_rect, content_pane_candidate));
                }
            }
        }
        Ok((
            final_result.0,
            final_result.1,
            final_result.2,
            if let Some(override_result) = apply_awt_code_file_override(
                &host_class,
                &host_title,
                host_rect,
                cursor_pos,
            ) {
                return Ok(override_result);
            } else {
                content_pane_candidate
            },
        ))
    })();

    CoUninitialize();
    result.unwrap_or((false, 0, None, None))
}

unsafe fn get_editable_focus_hint_impl(displays: &[DisplayInfo]) -> Option<EditableFocusHintInfo> {
    let fg_hwnd = GetForegroundWindow();
    let fg_hwnd_isize = fg_hwnd as isize;
    if fg_hwnd.is_null() {
        *EDITABLE_FOCUS_STICKY.lock().unwrap() = None;
        *QT_CARET_CONFIRMED.lock().unwrap() = None;
        return Some(EditableFocusHintInfo {
            editable: false,
            foreground_hwnd: 0,
            ..Default::default()
        });
    }

    let window = hwnd_screen_rect(fg_hwnd);
    let window_display_idx = rect_center(window)
        .map(|(cx, cy)| display_idx_for_point(displays, cx, cy))
        .unwrap_or(0);
    let not_editable = normalize_editable_focus_hint_to_display(displays, EditableFocusHintInfo {
        editable: false,
        window,
        pane: window,
        display_idx: window_display_idx,
        foreground_hwnd: fg_hwnd_isize,
        ..Default::default()
    });

    // GetGUIThreadInfo to find the focused child hwnd and caret rect
    let thread_id = winapi::um::winuser::GetWindowThreadProcessId(fg_hwnd, std::ptr::null_mut());
    let mut gti: winapi::um::winuser::GUITHREADINFO = mem::zeroed();
    gti.cbSize = std::mem::size_of::<winapi::um::winuser::GUITHREADINFO>() as DWORD;
    if winapi::um::winuser::GetGUIThreadInfo(thread_id, &mut gti) == 0 {
        if EDITABLE_FOCUS_STICKY.lock().unwrap().is_some() {
            log::debug!(
                "editable_focus miss: reason=gti-fail fg={} thread_id={} error={}",
                hwnd_debug_summary(fg_hwnd),
                thread_id,
                GetLastError()
            );
        }
        return Some(finalize_editable_focus_hint(fg_hwnd, not_editable.clone()));
    }

    let focus_hwnd = gti.hwndFocus;
    if focus_hwnd.is_null() {
        if EDITABLE_FOCUS_STICKY.lock().unwrap().is_some() {
            log::debug!(
                "editable_focus miss: reason=focus-null fg={} caret_hwnd={} thread_id={}",
                hwnd_debug_summary(fg_hwnd),
                hwnd_debug_summary(gti.hwndCaret),
                thread_id
            );
        }
        return Some(finalize_editable_focus_hint(fg_hwnd, not_editable.clone()));
    }

    // Get caret rect in screen coordinates early so we can use it as a
    // fallback signal for custom-rendered inputs that do not expose UIA edit
    // descendants but still publish a live caret through GUITHREADINFO.
    let caret = if !gti.hwndCaret.is_null() {
        let rc = gti.rcCaret;
        let mut pt = winapi::shared::windef::POINT { x: rc.left, y: rc.top };
        winapi::um::winuser::ClientToScreen(gti.hwndCaret, &mut pt);
        let w = (rc.right - rc.left).max(2);
        let h = (rc.bottom - rc.top).max(4);
        (pt.x, pt.y, w, h)
    } else {
        (0, 0, 0, 0)
    };
    let caret_visible = rect_has_area(caret);

    // Try Win32 class-based fast path
    let class_result = classify_by_class(focus_hwnd);
    let detect_source = if class_result.is_some() { "class" } else { "uia" };
    let (editable, content_kind, editor_rect, pane_override) =
        if let Some((editable, kind)) = class_result {
            (editable, kind, Some(hwnd_screen_rect(focus_hwnd)), None)
        } else {
            // Fall through to UIA for modern/Electron/WPF apps.
            // Do NOT degrade editable=true with no actionable rect into the
            // focused HWND bounds: Chromium/VS Code can then become a false
            // whole-window editable hint that the proxy-reuse path keeps alive.
            let (ed, kind, maybe_rect, maybe_pane) = probe_via_uia(focus_hwnd);
            (ed, kind, maybe_rect, maybe_pane)
        };
    let editor_rect_has_area = editor_rect.map(rect_has_area).unwrap_or(false);
    let actionable_editable = editable && editor_rect_has_area;

    if editable && detect_source == "uia" && !actionable_editable {
        log::debug!(
            "editable_focus reject uia editable without actionable rect: fg={} focus={} caret_hwnd={} caret={:?} kind={} pane={:?}",
            hwnd_debug_summary(fg_hwnd),
            hwnd_debug_summary(focus_hwnd),
            hwnd_debug_summary(gti.hwndCaret),
            caret,
            content_kind,
            pane_override
        );
    }

    if !actionable_editable {
        if content_kind == -1 {
            if let Some(reused) = try_reuse_sticky_hint_on_hard_negative(fg_hwnd) {
                return Some(reused);
            }
            *EDITABLE_FOCUS_STICKY.lock().unwrap() = None;
            log::debug!(
                "editable_focus hard negative: fg={} focus={} caret_hwnd={} caret={:?}",
                hwnd_debug_summary(fg_hwnd),
                hwnd_debug_summary(focus_hwnd),
                hwnd_debug_summary(gti.hwndCaret),
                caret
            );
            return Some(finalize_editable_focus_hint(fg_hwnd, not_editable.clone()));
        }
        // WeCom and similar custom-drawn apps may expose only the top-level
        // host window to UIA while still publishing a valid caret via
        // GetGUIThreadInfo. Treat that as editable and build a narrow editor
        // band around the caret so mobile can still open the IME and pan.
        //
        // Guard: reject phantom carets that Electron/Chrome apps report at the
        // hwndCaret window's own origin (0,0 in client space → window top-left
        // in screen space).  Such carets produce an editor band near (8,8) on
        // the desktop, causing the phone viewport to zoom into the wrong area.
        let caret_at_hwnd_origin = if caret_visible && !gti.hwndCaret.is_null() {
            let r = hwnd_screen_rect(gti.hwndCaret);
            // Caret is "degenerate" if it lands within 8px of the hwnd top-left corner
            (caret.0 - r.0).abs() <= 8 && (caret.1 - r.1).abs() <= 8
        } else {
            false
        };
        if caret_visible && !caret_at_hwnd_origin
            && (gti.hwndCaret == focus_hwnd || gti.hwndCaret == fg_hwnd)
        {
            let fallback_editor = caret_fallback_editor_rect(window, caret);
            let cx = fallback_editor.0 + fallback_editor.2 / 2;
            let cy = fallback_editor.1 + fallback_editor.3 / 2;
            let display_idx = display_idx_for_point(displays, cx, cy);
            let pane = compute_pane_rect(fallback_editor, window);
            log::debug!(
                "editable_focus caret fallback: focus_hwnd={:?}, caret={:?}, editor={:?}, pane={:?}",
                focus_hwnd,
                caret,
                fallback_editor,
                pane
            );
            let caret_hint = normalize_editable_focus_hint_to_display(displays, EditableFocusHintInfo {
                editable: true,
                caret,
                editor: fallback_editor,
                pane,
                window,
                display_idx,
                content_kind: 2,
                foreground_hwnd: fg_hwnd_isize,
            });
            if is_qt_class(fg_hwnd) {
                record_qt_caret_confirmation(fg_hwnd, &caret_hint);
            }
            return Some(finalize_editable_focus_hint(fg_hwnd, caret_hint));
        }
        // Qt caret confirmation: reuse last caret-confirmed hint for Qt windows
        // whose caret has since become invisible (WeChat shows caret for ~1s only).
        if let Some(qt_hint) = check_qt_caret_confirmed(fg_hwnd) {
            return Some(finalize_editable_focus_hint(fg_hwnd, qt_hint));
        }
        if detect_source == "uia" || EDITABLE_FOCUS_STICKY.lock().unwrap().is_some() {
            let miss_reason = if editable {
                "editable-no-rect"
            } else {
                "not-editable"
            };
            log::debug!(
                "editable_focus miss: reason={} source={} fg={} focus={} caret_hwnd={} caret={:?} editor={:?} pane={:?} class_result={:?}",
                miss_reason,
                detect_source,
                hwnd_debug_summary(fg_hwnd),
                hwnd_debug_summary(focus_hwnd),
                hwnd_debug_summary(gti.hwndCaret),
                caret,
                editor_rect,
                pane_override,
                class_result
            );
        }
        return Some(finalize_editable_focus_hint(fg_hwnd, not_editable.clone()));
    }

    let editor_rect = editor_rect.unwrap_or_default();
    let pane = pane_override.unwrap_or_else(|| compute_pane_rect(editor_rect, window));
    let cx = editor_rect.0 + editor_rect.2 / 2;
    let cy = editor_rect.1 + editor_rect.3 / 2;
    let display_idx = display_idx_for_point(displays, cx, cy);

    Some(finalize_editable_focus_hint(fg_hwnd, normalize_editable_focus_hint_to_display(displays, EditableFocusHintInfo {
        editable: true,
        caret,
        editor: editor_rect,
        pane,
        window,
        display_idx,
        content_kind,
        foreground_hwnd: fg_hwnd_isize,
    })))
}

pub fn get_cursor_pos() -> Option<(i32, i32)> {
    unsafe {
        let mut out = mem::MaybeUninit::<POINT>::uninit();
        if GetCursorPos(out.as_mut_ptr()) == FALSE {
            return None;
        }
        let out = out.assume_init();
        Some((out.x, out.y))
    }
}

pub fn get_foreground_window_rect() -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let rect = hwnd_screen_rect(hwnd);
        if rect_has_area(rect) {
            Some(rect)
        } else {
            None
        }
    }
}

pub fn set_cursor_pos(x: i32, y: i32) -> bool {
    unsafe {
        if SetCursorPos(x, y) == FALSE {
            let err = GetLastError();
            log::warn!("SetCursorPos failed: x={}, y={}, error_code={}", x, y, err);
            return false;
        }
        true
    }
}

/// Clip cursor to a rectangle. Pass None to unclip.
pub fn clip_cursor(rect: Option<(i32, i32, i32, i32)>) -> bool {
    unsafe {
        let result = match rect {
            Some((left, top, right, bottom)) => {
                let r = RECT {
                    left,
                    top,
                    right,
                    bottom,
                };
                ClipCursor(&r)
            }
            None => ClipCursor(std::ptr::null()),
        };
        if result == FALSE {
            let err = GetLastError();
            log::warn!("ClipCursor failed: rect={:?}, error_code={}", rect, err);
            return false;
        }
        true
    }
}

pub fn reset_input_cache() {}

pub fn get_cursor() -> ResultType<Option<u64>> {
    unsafe {
        #[allow(invalid_value)]
        let mut ci: CURSORINFO = mem::MaybeUninit::uninit().assume_init();
        ci.cbSize = std::mem::size_of::<CURSORINFO>() as _;
        if crate::portable_service::client::get_cursor_info(&mut ci) == FALSE {
            return Err(io::Error::last_os_error().into());
        }
        if ci.flags & CURSOR_SHOWING == 0 {
            Ok(None)
        } else {
            Ok(Some(ci.hCursor as _))
        }
    }
}

struct IconInfo(ICONINFO);

impl IconInfo {
    fn new(icon: HICON) -> ResultType<Self> {
        unsafe {
            #[allow(invalid_value)]
            let mut ii = mem::MaybeUninit::uninit().assume_init();
            if GetIconInfo(icon, &mut ii) == FALSE {
                Err(io::Error::last_os_error().into())
            } else {
                let ii = Self(ii);
                if ii.0.hbmMask.is_null() {
                    bail!("Cursor bitmap handle is NULL");
                }
                return Ok(ii);
            }
        }
    }

    fn is_color(&self) -> bool {
        !self.0.hbmColor.is_null()
    }
}

impl Drop for IconInfo {
    fn drop(&mut self) {
        unsafe {
            if !self.0.hbmColor.is_null() {
                DeleteObject(self.0.hbmColor as _);
            }
            if !self.0.hbmMask.is_null() {
                DeleteObject(self.0.hbmMask as _);
            }
        }
    }
}

// https://github.com/TurboVNC/tightvnc/blob/a235bae328c12fd1c3aed6f3f034a37a6ffbbd22/vnc_winsrc/winvnc/vncEncoder.cpp
// https://github.com/TigerVNC/tigervnc/blob/master/win/rfb_win32/DeviceFrameBuffer.cxx
pub fn get_cursor_data(hcursor: u64) -> ResultType<CursorData> {
    unsafe {
        let mut ii = IconInfo::new(hcursor as _)?;
        let bm_mask = get_bitmap(ii.0.hbmMask)?;
        let mut width = bm_mask.bmWidth;
        let mut height = if ii.is_color() {
            bm_mask.bmHeight
        } else {
            bm_mask.bmHeight / 2
        };
        let cbits_size = width * height * 4;
        if cbits_size < 16 {
            bail!("Invalid icon: too small"); // solve some crash
        }
        let mut cbits: Vec<u8> = Vec::new();
        cbits.resize(cbits_size as _, 0);
        let mut mbits: Vec<u8> = Vec::new();
        mbits.resize((bm_mask.bmWidthBytes * bm_mask.bmHeight) as _, 0);
        let r = GetBitmapBits(ii.0.hbmMask, mbits.len() as _, mbits.as_mut_ptr() as _);
        if r == 0 {
            bail!("Failed to copy bitmap data");
        }
        if r != (mbits.len() as i32) {
            bail!(
                "Invalid mask cursor buffer size, got {} bytes, expected {}",
                r,
                mbits.len()
            );
        }
        let do_outline;
        if ii.is_color() {
            get_rich_cursor_data(ii.0.hbmColor, width, height, &mut cbits)?;
            do_outline = fix_cursor_mask(
                &mut mbits,
                &mut cbits,
                width as _,
                height as _,
                bm_mask.bmWidthBytes as _,
            );
        } else {
            do_outline = handleMask(
                cbits.as_mut_ptr(),
                mbits.as_ptr(),
                width,
                height,
                bm_mask.bmWidthBytes,
                bm_mask.bmHeight,
            ) > 0;
        }
        if do_outline {
            let mut outline = Vec::new();
            outline.resize(((width + 2) * (height + 2) * 4) as _, 0);
            drawOutline(
                outline.as_mut_ptr(),
                cbits.as_ptr(),
                width,
                height,
                outline.len() as _,
            );
            cbits = outline;
            width += 2;
            height += 2;
            ii.0.xHotspot += 1;
            ii.0.yHotspot += 1;
        }

        Ok(CursorData {
            id: hcursor,
            colors: cbits.into(),
            hotx: ii.0.xHotspot as _,
            hoty: ii.0.yHotspot as _,
            width: width as _,
            height: height as _,
            ..Default::default()
        })
    }
}

#[inline]
fn get_bitmap(handle: HBITMAP) -> ResultType<BITMAP> {
    unsafe {
        let mut bm: BITMAP = mem::zeroed();
        if GetObjectA(
            handle as _,
            std::mem::size_of::<BITMAP>() as _,
            &mut bm as *mut BITMAP as *mut _,
        ) == FALSE
        {
            return Err(io::Error::last_os_error().into());
        }
        if bm.bmPlanes != 1 {
            bail!("Unsupported multi-plane cursor");
        }
        if bm.bmBitsPixel != 1 {
            bail!("Unsupported cursor mask format");
        }
        Ok(bm)
    }
}

struct DC(HDC);

impl DC {
    fn new() -> ResultType<Self> {
        unsafe {
            let dc = GetDC(0 as _);
            if dc.is_null() {
                bail!("Failed to get a drawing context");
            }
            Ok(Self(dc))
        }
    }
}

impl Drop for DC {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                ReleaseDC(0 as _, self.0);
            }
        }
    }
}

struct CompatibleDC(HDC);

impl CompatibleDC {
    fn new(existing: HDC) -> ResultType<Self> {
        unsafe {
            let dc = CreateCompatibleDC(existing);
            if dc.is_null() {
                bail!("Failed to get a compatible drawing context");
            }
            Ok(Self(dc))
        }
    }
}

impl Drop for CompatibleDC {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                DeleteDC(self.0);
            }
        }
    }
}

struct BitmapDC(CompatibleDC, HBITMAP);

impl BitmapDC {
    fn new(hdc: HDC, hbitmap: HBITMAP) -> ResultType<Self> {
        unsafe {
            let dc = CompatibleDC::new(hdc)?;
            let oldbitmap = SelectObject(dc.0, hbitmap as _) as HBITMAP;
            if oldbitmap.is_null() {
                bail!("Failed to select CompatibleDC");
            }
            Ok(Self(dc, oldbitmap))
        }
    }

    fn dc(&self) -> HDC {
        (self.0).0
    }
}

impl Drop for BitmapDC {
    fn drop(&mut self) {
        unsafe {
            if !self.1.is_null() {
                SelectObject((self.0).0, self.1 as _);
            }
        }
    }
}

#[inline]
fn get_rich_cursor_data(
    hbm_color: HBITMAP,
    width: i32,
    height: i32,
    out: &mut Vec<u8>,
) -> ResultType<()> {
    unsafe {
        let dc = DC::new()?;
        let bitmap_dc = BitmapDC::new(dc.0, hbm_color)?;
        if get_di_bits(out.as_mut_ptr(), bitmap_dc.dc(), hbm_color, width, height) > 0 {
            bail!("Failed to get di bits: {}", io::Error::last_os_error());
        }
    }
    Ok(())
}

fn fix_cursor_mask(
    mbits: &mut Vec<u8>,
    cbits: &mut Vec<u8>,
    width: usize,
    height: usize,
    bm_width_bytes: usize,
) -> bool {
    let mut pix_idx = 0;
    for _ in 0..height {
        for _ in 0..width {
            if cbits[pix_idx + 3] != 0 {
                return false;
            }
            pix_idx += 4;
        }
    }

    let packed_width_bytes = (width + 7) >> 3;
    let bm_size = mbits.len();
    let c_size = cbits.len();

    // Pack and invert bitmap data (mbits)
    // borrow from tigervnc
    for y in 0..height {
        for x in 0..packed_width_bytes {
            let a = y * packed_width_bytes + x;
            let b = y * bm_width_bytes + x;
            if a < bm_size && b < bm_size {
                mbits[a] = !mbits[b];
            }
        }
    }

    // Replace "inverted background" bits with black color to ensure
    // cross-platform interoperability. Not beautiful but necessary code.
    // borrow from tigervnc
    let bytes_row = width << 2;
    for y in 0..height {
        let mut bitmask: u8 = 0x80;
        for x in 0..width {
            let mask_idx = y * packed_width_bytes + (x >> 3);
            if mask_idx < bm_size {
                let pix_idx = y * bytes_row + (x << 2);
                if (mbits[mask_idx] & bitmask) == 0 {
                    for b1 in 0..4 {
                        let a = pix_idx + b1;
                        if a < c_size {
                            if cbits[a] != 0 {
                                mbits[mask_idx] ^= bitmask;
                                for b2 in b1..4 {
                                    let b = pix_idx + b2;
                                    if b < c_size {
                                        cbits[b] = 0x00;
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            }
            bitmask >>= 1;
            if bitmask == 0 {
                bitmask = 0x80;
            }
        }
    }

    // borrow from noVNC
    let mut pix_idx = 0;
    for y in 0..height {
        for x in 0..width {
            let mask_idx = y * packed_width_bytes + (x >> 3);
            let mut alpha = 255;
            if mask_idx < bm_size {
                if (mbits[mask_idx] << (x & 0x7)) & 0x80 == 0 {
                    alpha = 0;
                }
            }
            let a = cbits[pix_idx + 2];
            let b = cbits[pix_idx + 1];
            let c = cbits[pix_idx];
            cbits[pix_idx] = a;
            cbits[pix_idx + 1] = b;
            cbits[pix_idx + 2] = c;
            cbits[pix_idx + 3] = alpha;
            pix_idx += 4;
        }
    }
    return true;
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(arguments: Vec<OsString>) {
    if let Err(e) = run_service(arguments) {
        log::error!("run_service failed: {}", e);
    }
}

pub fn start_os_service() {
    if let Err(e) =
        windows_service::service_dispatcher::start(crate::get_app_name(), ffi_service_main)
    {
        log::error!("start_service failed: {}", e);
    }
}

const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

extern "C" {
    fn get_current_session(rdp: BOOL) -> DWORD;
    fn is_session_locked(session_id: DWORD) -> BOOL;
    fn LaunchProcessWin(
        cmd: *const u16,
        session_id: DWORD,
        as_user: BOOL,
        show: BOOL,
        token_pid: &mut DWORD,
    ) -> HANDLE;
    fn GetSessionUserTokenWin(
        lphUserToken: LPHANDLE,
        dwSessionId: DWORD,
        as_user: BOOL,
        token_pid: &mut DWORD,
    ) -> BOOL;
    fn selectInputDesktop() -> BOOL;
    fn inputDesktopSelected() -> BOOL;
    fn is_windows_server() -> BOOL;
    fn is_windows_10_or_greater() -> BOOL;
    fn handleMask(
        out: *mut u8,
        mask: *const u8,
        width: i32,
        height: i32,
        bmWidthBytes: i32,
        bmHeight: i32,
    ) -> i32;
    fn drawOutline(out: *mut u8, in_: *const u8, width: i32, height: i32, out_size: i32);
    fn get_di_bits(out: *mut u8, dc: HDC, hbmColor: HBITMAP, width: i32, height: i32) -> i32;
    fn blank_screen(v: BOOL);
    fn win32_enable_lowlevel_keyboard(hwnd: HWND) -> i32;
    fn win32_disable_lowlevel_keyboard(hwnd: HWND);
    fn win_stop_system_key_propagate(v: BOOL);
    fn is_win_down() -> BOOL;
    fn is_local_system() -> BOOL;
    fn alloc_console_and_redirect();
    fn is_service_running_w(svc_name: *const u16) -> bool;
}

pub fn get_current_session_id(share_rdp: bool) -> DWORD {
    unsafe { get_current_session(if share_rdp { TRUE } else { FALSE }) }
}

extern "system" {
    fn BlockInput(v: BOOL) -> BOOL;
}

#[tokio::main(flavor = "current_thread")]
async fn run_service(_arguments: Vec<OsString>) -> ResultType<()> {
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        log::info!("Got service control event: {:?}", control_event);
        match control_event {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop | ServiceControl::Preshutdown | ServiceControl::Shutdown => {
                send_close(crate::POSTFIX_SERVICE).ok();
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    // Register system service event handler
    let status_handle = service_control_handler::register(crate::get_app_name(), event_handler)?;

    let next_status = ServiceStatus {
        // Should match the one from system service registry
        service_type: SERVICE_TYPE,
        // The new state
        current_state: ServiceState::Running,
        // Accept stop events when running
        controls_accepted: ServiceControlAccept::STOP,
        // Used to report an error when starting or stopping only, otherwise must be zero
        exit_code: ServiceExitCode::Win32(0),
        // Only used for pending states, otherwise must be zero
        checkpoint: 0,
        // Only used for pending states, otherwise must be zero
        wait_hint: Duration::default(),
        process_id: None,
    };

    // Tell the system that the service is running now
    status_handle.set_service_status(next_status)?;

    let mut session_id = unsafe { get_current_session(share_rdp()) };
    log::info!("session id {}", session_id);
    let mut h_process = launch_server(session_id, true).await.unwrap_or(NULL);
    let mut incoming = ipc::new_listener(crate::POSTFIX_SERVICE).await?;
    let mut stored_usid = None;
    loop {
        let sids: Vec<_> = get_available_sessions(false)
            .iter()
            .map(|e| e.sid)
            .collect();
        if !sids.contains(&session_id) || !is_share_rdp() {
            let current_active_session = unsafe { get_current_session(share_rdp()) };
            if session_id != current_active_session {
                session_id = current_active_session;
                // https://github.com/rustdesk/rustdesk/discussions/10039
                let count = ipc::get_port_forward_session_count(1000).await.unwrap_or(0);
                if count == 0 {
                    h_process = launch_server(session_id, true).await.unwrap_or(NULL);
                }
            }
        }
        let res = timeout(super::SERVICE_INTERVAL, incoming.next()).await;
        match res {
            Ok(res) => match res {
                Some(Ok(stream)) => {
                    let mut stream = ipc::Connection::new(stream);
                    if let Ok(Some(data)) = stream.next_timeout(1000).await {
                        match data {
                            ipc::Data::Close => {
                                log::info!("close received");
                                break;
                            }
                            ipc::Data::SAS => {
                                send_sas();
                            }
                            ipc::Data::UserSid(usid) => {
                                if let Some(usid) = usid {
                                    if session_id != usid {
                                        log::info!(
                                            "session changed from {} to {}",
                                            session_id,
                                            usid
                                        );
                                        session_id = usid;
                                        stored_usid = Some(session_id);
                                        h_process =
                                            launch_server(session_id, true).await.unwrap_or(NULL);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            Err(_) => {
                // timeout
                unsafe {
                    let tmp = get_current_session(share_rdp());
                    if tmp == 0xFFFFFFFF {
                        continue;
                    }
                    let mut close_sent = false;
                    if tmp != session_id && stored_usid != Some(session_id) {
                        log::info!("session changed from {} to {}", session_id, tmp);
                        session_id = tmp;
                        let count = ipc::get_port_forward_session_count(1000).await.unwrap_or(0);
                        if count == 0 {
                            send_close_async("").await.ok();
                            close_sent = true;
                        }
                    }
                    let mut exit_code: DWORD = 0;
                    if h_process.is_null()
                        || (GetExitCodeProcess(h_process, &mut exit_code) == TRUE
                            && exit_code != STILL_ACTIVE
                            && CloseHandle(h_process) == TRUE)
                    {
                        match launch_server(session_id, !close_sent).await {
                            Ok(ptr) => {
                                h_process = ptr;
                            }
                            Err(err) => {
                                log::error!("Failed to launch server: {}", err);
                            }
                        }
                    }
                }
            }
        }
    }

    if !h_process.is_null() {
        send_close_async("").await.ok();
        unsafe { CloseHandle(h_process) };
    }

    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

async fn launch_server(session_id: DWORD, close_first: bool) -> ResultType<HANDLE> {
    if close_first {
        // in case started some elsewhere
        send_close_async("").await.ok();
    }
    let cmd = format!(
        "\"{}\" --server",
        std::env::current_exe()?.to_str().unwrap_or("")
    );
    launch_privileged_process(session_id, &cmd)
}

pub fn launch_privileged_process(session_id: DWORD, cmd: &str) -> ResultType<HANDLE> {
    use std::os::windows::ffi::OsStrExt;
    let wstr: Vec<u16> = std::ffi::OsStr::new(&cmd)
        .encode_wide()
        .chain(Some(0).into_iter())
        .collect();
    let wstr = wstr.as_ptr();
    let mut token_pid = 0;
    let h = unsafe { LaunchProcessWin(wstr, session_id, FALSE, FALSE, &mut token_pid) };
    if h.is_null() {
        log::error!(
            "Failed to launch privileged process: {}",
            io::Error::last_os_error()
        );
        if token_pid == 0 {
            log::error!("No process winlogon.exe");
        }
    }
    Ok(h)
}

pub fn run_as_user(arg: Vec<&str>) -> ResultType<Option<std::process::Child>> {
    run_exe_in_cur_session(std::env::current_exe()?.to_str().unwrap_or(""), arg, false)
}

pub fn run_exe_direct(
    exe: &str,
    arg: Vec<&str>,
    show: bool,
) -> ResultType<Option<std::process::Child>> {
    let mut cmd = std::process::Command::new(exe);
    for a in arg {
        cmd.arg(a);
    }
    if !show {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.spawn() {
        Ok(child) => Ok(Some(child)),
        Err(e) => bail!("Failed to start process: {}", e),
    }
}

pub fn run_exe_in_cur_session(
    exe: &str,
    arg: Vec<&str>,
    show: bool,
) -> ResultType<Option<std::process::Child>> {
    if is_root() {
        let Some(session_id) = get_current_process_session_id() else {
            bail!("Failed to get current process session id");
        };
        run_exe_in_session(exe, arg, session_id, show)
    } else {
        run_exe_direct(exe, arg, show)
    }
}

pub fn run_exe_in_session(
    exe: &str,
    arg: Vec<&str>,
    session_id: DWORD,
    show: bool,
) -> ResultType<Option<std::process::Child>> {
    use std::os::windows::ffi::OsStrExt;
    let cmd = format!("\"{}\" {}", exe, arg.join(" "),);
    let wstr: Vec<u16> = std::ffi::OsStr::new(&cmd)
        .encode_wide()
        .chain(Some(0).into_iter())
        .collect();
    let wstr = wstr.as_ptr();
    let mut token_pid = 0;
    let h = unsafe {
        LaunchProcessWin(
            wstr,
            session_id,
            TRUE,
            if show { TRUE } else { FALSE },
            &mut token_pid,
        )
    };
    if h.is_null() {
        if token_pid == 0 {
            bail!(
                "Failed to launch {:?} with session id {}: no process {}",
                arg,
                session_id,
                EXPLORER_EXE
            );
        }
        bail!(
            "Failed to launch {:?} with session id {}: {}",
            arg,
            session_id,
            io::Error::last_os_error()
        );
    }
    Ok(None)
}

#[tokio::main(flavor = "current_thread")]
async fn send_close(postfix: &str) -> ResultType<()> {
    send_close_async(postfix).await
}

async fn send_close_async(postfix: &str) -> ResultType<()> {
    ipc::connect(1000, postfix)
        .await?
        .send(&ipc::Data::Close)
        .await?;
    // sleep a while to wait for closing and exit
    sleep(0.1).await;
    Ok(())
}

// https://docs.microsoft.com/en-us/windows/win32/api/sas/nf-sas-sendsas
// https://www.cnblogs.com/doutu/p/4892726.html
pub fn send_sas() {
    #[link(name = "sas")]
    extern "system" {
        pub fn SendSAS(AsUser: BOOL);
    }
    unsafe {
        log::info!("SAS received");

        // Check and temporarily set SoftwareSASGeneration if needed
        let mut original_value: Option<u32> = None;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

        if let Ok(policy_key) = hklm.open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\System",
            KEY_READ | KEY_WRITE,
        ) {
            // Read current value
            match policy_key.get_value::<u32, _>("SoftwareSASGeneration") {
                Ok(value) => {
                    /*
                    - 0 = None (disabled)
                    - 1 = Services
                    - 2 = Ease of Access applications
                    - 3 = Services and Ease of Access applications (Both)
                                      */
                    if value != 1 && value != 3 {
                        original_value = Some(value);
                        log::info!("SoftwareSASGeneration is {}, setting to 1", value);
                        // Set to 1 for SendSAS to work
                        if let Err(e) = policy_key.set_value("SoftwareSASGeneration", &1u32) {
                            log::error!("Failed to set SoftwareSASGeneration: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::info!(
                        "SoftwareSASGeneration not found or error reading: {}, setting to 1",
                        e
                    );
                    original_value = Some(0); // Mark that we need to restore (delete) it
                                              // Create and set to 1
                    if let Err(e) = policy_key.set_value("SoftwareSASGeneration", &1u32) {
                        log::error!("Failed to set SoftwareSASGeneration: {}", e);
                    }
                }
            }
        } else {
            log::error!("Failed to open registry key for SoftwareSASGeneration");
        }

        // Send SAS
        SendSAS(FALSE);

        // Restore original value if we changed it
        if let Some(original) = original_value {
            if let Ok(policy_key) = hklm.open_subkey_with_flags(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\System",
                KEY_WRITE,
            ) {
                if original == 0 {
                    // It didn't exist before, delete it
                    if let Err(e) = policy_key.delete_value("SoftwareSASGeneration") {
                        log::error!("Failed to delete SoftwareSASGeneration: {}", e);
                    } else {
                        log::info!("Deleted SoftwareSASGeneration (restored to original state)");
                    }
                } else {
                    // Restore the original value
                    if let Err(e) = policy_key.set_value("SoftwareSASGeneration", &original) {
                        log::error!(
                            "Failed to restore SoftwareSASGeneration to {}: {}",
                            original,
                            e
                        );
                    } else {
                        log::info!("Restored SoftwareSASGeneration to {}", original);
                    }
                }
            }
        }
    }
}

lazy_static::lazy_static! {
    static ref SUPPRESS: Arc<Mutex<Instant>> = Arc::new(Mutex::new(Instant::now()));
}

pub fn desktop_changed() -> bool {
    unsafe { inputDesktopSelected() == FALSE }
}

pub fn try_change_desktop() -> bool {
    unsafe {
        if inputDesktopSelected() == FALSE {
            let res = selectInputDesktop() == TRUE;
            if !res {
                let mut s = SUPPRESS.lock().unwrap();
                if s.elapsed() > std::time::Duration::from_secs(3) {
                    log::error!("Failed to switch desktop: {}", io::Error::last_os_error());
                    *s = Instant::now();
                }
            } else {
                log::info!("Desktop switched");
            }
            return res;
        }
    }
    return false;
}

fn share_rdp() -> BOOL {
    if get_reg("share_rdp") != "false" {
        TRUE
    } else {
        FALSE
    }
}

pub fn is_share_rdp() -> bool {
    share_rdp() == TRUE
}

pub fn set_share_rdp(enable: bool) {
    let (subkey, _, _, _) = get_install_info();
    let cmd = format!(
        "reg add {} /f /v share_rdp /t REG_SZ /d \"{}\"",
        subkey,
        if enable { "true" } else { "false" }
    );
    run_cmds(cmd, false, "share_rdp").ok();
}

pub fn get_current_process_session_id() -> Option<u32> {
    get_session_id_of_process(unsafe { GetCurrentProcessId() })
}

pub fn get_session_id_of_process(pid: DWORD) -> Option<u32> {
    let mut sid = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut sid) == TRUE } {
        Some(sid)
    } else {
        None
    }
}

pub fn is_physical_console_session() -> Option<bool> {
    if let Some(sid) = get_current_process_session_id() {
        let physical_console_session_id = unsafe { get_current_session(FALSE) };
        if physical_console_session_id == u32::MAX {
            return None;
        }
        return Some(physical_console_session_id == sid);
    }
    None
}

pub fn get_active_username() -> String {
    // get_active_user will give console username higher priority
    if let Some(name) = get_current_session_username() {
        return name;
    }
    if !is_root() {
        return crate::username();
    }

    extern "C" {
        fn get_active_user(path: *mut u16, n: u32, rdp: BOOL) -> u32;
    }
    let buff_size = 256;
    let mut buff: Vec<u16> = Vec::with_capacity(buff_size);
    buff.resize(buff_size, 0);
    let n = unsafe { get_active_user(buff.as_mut_ptr(), buff_size as _, share_rdp()) };
    if n == 0 {
        return "".to_owned();
    }
    let sl = unsafe { std::slice::from_raw_parts(buff.as_ptr(), n as _) };
    String::from_utf16(sl)
        .unwrap_or("??".to_owned())
        .trim_end_matches('\0')
        .to_owned()
}

fn get_current_session_username() -> Option<String> {
    let Some(sid) = get_current_process_session_id() else {
        log::error!("get_current_process_session_id failed");
        return None;
    };
    Some(get_session_username(sid))
}

fn get_session_username(session_id: u32) -> String {
    extern "C" {
        fn get_session_user_info(path: *mut u16, n: u32, session_id: u32) -> u32;
    }
    let buff_size = 256;
    let mut buff: Vec<u16> = Vec::with_capacity(buff_size);
    buff.resize(buff_size, 0);
    let n = unsafe { get_session_user_info(buff.as_mut_ptr(), buff_size as _, session_id) };
    if n == 0 {
        return "".to_owned();
    }
    let sl = unsafe { std::slice::from_raw_parts(buff.as_ptr(), n as _) };
    String::from_utf16(sl)
        .unwrap_or("".to_owned())
        .trim_end_matches('\0')
        .to_owned()
}

pub fn get_available_sessions(name: bool) -> Vec<WindowsSession> {
    extern "C" {
        fn get_available_session_ids(buf: *mut wchar_t, buf_size: c_int, include_rdp: bool);
    }
    const BUF_SIZE: c_int = 1024;
    let mut buf: Vec<wchar_t> = vec![0; BUF_SIZE as usize];

    let station_session_id_array = unsafe {
        get_available_session_ids(buf.as_mut_ptr(), BUF_SIZE, true);
        let session_ids = String::from_utf16_lossy(&buf);
        session_ids.trim_matches(char::from(0)).trim().to_string()
    };
    let mut v: Vec<WindowsSession> = vec![];
    // https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-wtsgetactiveconsolesessionid
    let physical_console_sid = unsafe { get_current_session(FALSE) };
    if physical_console_sid != u32::MAX {
        let physical_console_name = if name {
            let physical_console_username = get_session_username(physical_console_sid);
            if physical_console_username.is_empty() {
                "Console".to_owned()
            } else {
                format!("Console: {physical_console_username}")
            }
        } else {
            "".to_owned()
        };
        v.push(WindowsSession {
            sid: physical_console_sid,
            name: physical_console_name,
            ..Default::default()
        });
    }
    // https://learn.microsoft.com/en-us/previous-versions//cc722458(v=technet.10)?redirectedfrom=MSDN
    for type_session_id in station_session_id_array.split(",") {
        let split: Vec<_> = type_session_id.split(":").collect();
        if split.len() == 2 {
            if let Ok(sid) = split[1].parse::<u32>() {
                if !v.iter().any(|e| (*e).sid == sid) {
                    let name = if name {
                        let name = get_session_username(sid);
                        if name.is_empty() {
                            split[0].to_string()
                        } else {
                            format!("{}: {}", split[0], name)
                        }
                    } else {
                        "".to_owned()
                    };
                    v.push(WindowsSession {
                        sid,
                        name,
                        ..Default::default()
                    });
                }
            }
        }
    }
    if name {
        let mut name_count: HashMap<String, usize> = HashMap::new();
        for session in &v {
            *name_count.entry(session.name.clone()).or_insert(0) += 1;
        }
        let current_sid = get_current_process_session_id().unwrap_or_default();
        for e in v.iter_mut() {
            let running = e.sid == current_sid && current_sid != 0;
            if name_count.get(&e.name).map(|v| *v).unwrap_or_default() > 1 {
                e.name = format!("{} (sid = {})", e.name, e.sid);
            }
            if running {
                e.name = format!("{} (running)", e.name);
            }
        }
    }
    v
}

pub fn get_active_user_home() -> Option<PathBuf> {
    let username = get_active_username();
    if !username.is_empty() {
        let drive = std::env::var("SystemDrive").unwrap_or("C:".to_owned());
        let home = PathBuf::from(format!("{}\\Users\\{}", drive, username));
        if home.exists() {
            return Some(home);
        }
    }
    None
}

pub fn is_prelogin() -> bool {
    let Some(username) = get_current_session_username() else {
        return false;
    };
    username.is_empty() || username == "SYSTEM"
}

pub fn is_locked() -> bool {
    let Some(session_id) = get_current_process_session_id() else {
        return false;
    };
    unsafe { is_session_locked(session_id) == TRUE }
}

#[inline]
pub fn is_logon_ui() -> ResultType<bool> {
    let Some(current_sid) = get_current_process_session_id() else {
        return Ok(false);
    };
    let pids = get_pids("LogonUI.exe")?;
    Ok(pids
        .into_iter()
        .any(|pid| get_session_id_of_process(pid) == Some(current_sid)))
}

pub fn is_root() -> bool {
    // https://stackoverflow.com/questions/4023586/correct-way-to-find-out-if-a-service-is-running-as-the-system-user
    unsafe { is_local_system() == TRUE }
}

pub fn lock_screen() {
    extern "system" {
        pub fn LockWorkStation() -> BOOL;
    }
    unsafe {
        LockWorkStation();
    }
}

const IS1: &str = "{54E86BC2-6C85-41F3-A9EB-1A94AC9B1F93}_is1";

fn get_subkey(name: &str, wow: bool) -> String {
    let tmp = format!(
        "HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
        name
    );
    if wow {
        tmp.replace("Microsoft", "Wow6432Node\\Microsoft")
    } else {
        tmp
    }
}

fn get_valid_subkey() -> String {
    let subkey = get_subkey(IS1, false);
    if !get_reg_of(&subkey, "InstallLocation").is_empty() {
        return subkey;
    }
    let subkey = get_subkey(IS1, true);
    if !get_reg_of(&subkey, "InstallLocation").is_empty() {
        return subkey;
    }
    let app_name = crate::get_app_name();
    let subkey = get_subkey(&app_name, true);
    if !get_reg_of(&subkey, "InstallLocation").is_empty() {
        return subkey;
    }
    return get_subkey(&app_name, false);
}

// Return install options other than InstallLocation.
pub fn get_install_options() -> String {
    let app_name = crate::get_app_name();
    let subkey = format!(".{}", app_name.to_lowercase());
    let mut opts = HashMap::new();

    let desktop_shortcuts = get_reg_of_hkcr(&subkey, REG_NAME_INSTALL_DESKTOPSHORTCUTS);
    if let Some(desktop_shortcuts) = desktop_shortcuts {
        opts.insert(REG_NAME_INSTALL_DESKTOPSHORTCUTS, desktop_shortcuts);
    }
    let start_menu_shortcuts = get_reg_of_hkcr(&subkey, REG_NAME_INSTALL_STARTMENUSHORTCUTS);
    if let Some(start_menu_shortcuts) = start_menu_shortcuts {
        opts.insert(REG_NAME_INSTALL_STARTMENUSHORTCUTS, start_menu_shortcuts);
    }
    let printer = get_reg_of_hkcr(&subkey, REG_NAME_INSTALL_PRINTER);
    if let Some(printer) = printer {
        opts.insert(REG_NAME_INSTALL_PRINTER, printer);
    }
    serde_json::to_string(&opts).unwrap_or("{}".to_owned())
}

// This function return Option<String>, because some registry value may be empty.
fn get_reg_of_hkcr(subkey: &str, name: &str) -> Option<String> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    if let Ok(tmp) = hkcr.open_subkey(subkey.replace("HKEY_CLASSES_ROOT\\", "")) {
        return tmp.get_value(name).ok();
    }
    None
}

pub fn get_install_info() -> (String, String, String, String) {
    get_install_info_with_subkey(get_valid_subkey())
}

fn get_default_install_info() -> (String, String, String, String) {
    get_install_info_with_subkey(get_subkey(&crate::get_app_name(), false))
}

fn get_default_install_path() -> String {
    let mut pf = "C:\\Program Files".to_owned();
    if let Ok(x) = std::env::var("ProgramFiles") {
        if std::path::Path::new(&x).exists() {
            pf = x;
        }
    }
    #[cfg(target_pointer_width = "32")]
    {
        let tmp = pf.replace("Program Files", "Program Files (x86)");
        if std::path::Path::new(&tmp).exists() {
            pf = tmp;
        }
    }
    format!("{}\\{}", pf, crate::get_app_name())
}

pub fn check_update_broker_process() -> ResultType<()> {
    let process_exe = win_topmost_window::INJECTED_PROCESS_EXE;
    let origin_process_exe = win_topmost_window::ORIGIN_PROCESS_EXE;

    let exe_file = std::env::current_exe()?;
    let Some(cur_dir) = exe_file.parent() else {
        bail!("Cannot get parent of current exe file");
    };
    let cur_exe = cur_dir.join(process_exe);

    // Force update broker exe if failed to check modified time.
    let cmds = format!(
        "
        chcp 65001
        taskkill /F /IM {process_exe}
        copy /Y \"{origin_process_exe}\" \"{cur_exe}\"
    ",
        cur_exe = cur_exe.to_string_lossy(),
    );

    if !std::path::Path::new(&cur_exe).exists() {
        run_cmds(cmds, false, "update_broker")?;
        return Ok(());
    }

    let ori_modified = fs::metadata(origin_process_exe)?.modified()?;
    if let Ok(metadata) = fs::metadata(&cur_exe) {
        if let Ok(cur_modified) = metadata.modified() {
            if cur_modified == ori_modified {
                return Ok(());
            } else {
                log::info!(
                    "broker process updated, modify time from {:?} to {:?}",
                    cur_modified,
                    ori_modified
                );
            }
        }
    }

    run_cmds(cmds, false, "update_broker")?;

    Ok(())
}

fn get_install_info_with_subkey(subkey: String) -> (String, String, String, String) {
    let mut path = get_reg_of(&subkey, "InstallLocation");
    if path.is_empty() {
        path = get_default_install_path();
    }
    path = path.trim_end_matches('\\').to_owned();
    let start_menu = format!(
        "%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs\\{}",
        crate::get_app_name()
    );
    let exe = format!("{}\\{}.exe", path, crate::get_app_name());
    (subkey, path, start_menu, exe)
}

pub fn copy_raw_cmd(src_raw: &str, _raw: &str, _path: &str) -> ResultType<String> {
    let main_raw = format!(
        "XCOPY \"{}\" \"{}\" /Y /E /H /C /I /K /R /Z",
        PathBuf::from(src_raw)
            .parent()
            .ok_or(anyhow!("Can't get parent directory of {src_raw}"))?
            .to_string_lossy()
            .to_string(),
        _path
    );
    return Ok(main_raw);
}

pub fn copy_exe_cmd(src_exe: &str, exe: &str, path: &str) -> ResultType<String> {
    let main_exe = copy_raw_cmd(src_exe, exe, path)?;
    Ok(format!(
        "
        {main_exe}
        copy /Y \"{ORIGIN_PROCESS_EXE}\" \"{path}\\{broker_exe}\"
        ",
        ORIGIN_PROCESS_EXE = win_topmost_window::ORIGIN_PROCESS_EXE,
        broker_exe = win_topmost_window::INJECTED_PROCESS_EXE,
    ))
}

#[inline]
pub fn rename_exe_cmd(src_exe: &str, path: &str) -> ResultType<String> {
    let src_exe_filename = PathBuf::from(src_exe)
        .file_name()
        .ok_or(anyhow!("Can't get file name of {src_exe}"))?
        .to_string_lossy()
        .to_string();
    let app_name = crate::get_app_name().to_lowercase();
    if src_exe_filename.to_lowercase() == format!("{app_name}.exe") {
        Ok("".to_owned())
    } else {
        Ok(format!(
            "
        move /Y \"{path}\\{src_exe_filename}\" \"{path}\\{app_name}.exe\"
        ",
        ))
    }
}

#[inline]
pub fn remove_meta_toml_cmd(is_msi: bool, path: &str) -> String {
    if is_msi && crate::is_custom_client() {
        format!(
            "
        del /F /Q \"{path}\\meta.toml\"
        ",
        )
    } else {
        "".to_owned()
    }
}

fn get_after_install(
    exe: &str,
    reg_value_start_menu_shortcuts: Option<String>,
    reg_value_desktop_shortcuts: Option<String>,
    reg_value_printer: Option<String>,
) -> String {
    let app_name = crate::get_app_name();
    let ext = app_name.to_lowercase();

    // reg delete HKEY_CURRENT_USER\Software\Classes for
    // https://github.com/rustdesk/rustdesk/commit/f4bdfb6936ae4804fc8ab1cf560db192622ad01a
    // and https://github.com/leanflutter/uni_links_desktop/blob/1b72b0226cec9943ca8a84e244c149773f384e46/lib/src/protocol_registrar_impl_windows.dart#L30
    let hcu = RegKey::predef(HKEY_CURRENT_USER);
    hcu.delete_subkey_all(format!("Software\\Classes\\{}", exe))
        .ok();

    let desktop_shortcuts = reg_value_desktop_shortcuts
        .map(|v| {
            format!("reg add HKEY_CLASSES_ROOT\\.{ext} /f /v {REG_NAME_INSTALL_DESKTOPSHORTCUTS} /t REG_SZ /d \"{v}\"")
        })
        .unwrap_or_default();
    let start_menu_shortcuts = reg_value_start_menu_shortcuts
        .map(|v| {
            format!(
                "reg add HKEY_CLASSES_ROOT\\.{ext} /f /v {REG_NAME_INSTALL_STARTMENUSHORTCUTS} /t REG_SZ /d \"{v}\""
            )
        })
        .unwrap_or_default();
    let reg_printer = reg_value_printer
        .map(|v| {
            format!(
                "reg add HKEY_CLASSES_ROOT\\.{ext} /f /v {REG_NAME_INSTALL_PRINTER} /t REG_SZ /d \"{v}\""
            )
        })
        .unwrap_or_default();

    format!("
    chcp 65001
    reg add HKEY_CLASSES_ROOT\\.{ext} /f
    {desktop_shortcuts}
    {start_menu_shortcuts}
    {reg_printer}
    reg add HKEY_CLASSES_ROOT\\.{ext}\\DefaultIcon /f
    reg add HKEY_CLASSES_ROOT\\.{ext}\\DefaultIcon /f /ve /t REG_SZ  /d \"\\\"{exe}\\\",0\"
    reg add HKEY_CLASSES_ROOT\\.{ext}\\shell /f
    reg add HKEY_CLASSES_ROOT\\.{ext}\\shell\\open /f
    reg add HKEY_CLASSES_ROOT\\.{ext}\\shell\\open\\command /f
    reg add HKEY_CLASSES_ROOT\\.{ext}\\shell\\open\\command /f /ve /t REG_SZ /d \"\\\"{exe}\\\" --play \\\"%%1\\\"\"
    reg add HKEY_CLASSES_ROOT\\{ext} /f
    reg add HKEY_CLASSES_ROOT\\{ext} /f /v \"URL Protocol\" /t REG_SZ /d \"\"
    reg add HKEY_CLASSES_ROOT\\{ext}\\shell /f
    reg add HKEY_CLASSES_ROOT\\{ext}\\shell\\open /f
    reg add HKEY_CLASSES_ROOT\\{ext}\\shell\\open\\command /f
    reg add HKEY_CLASSES_ROOT\\{ext}\\shell\\open\\command /f /ve /t REG_SZ /d \"\\\"{exe}\\\" \\\"%%1\\\"\"
    netsh advfirewall firewall add rule name=\"{app_name} Service\" dir=out action=allow program=\"{exe}\" enable=yes
    netsh advfirewall firewall add rule name=\"{app_name} Service\" dir=in action=allow program=\"{exe}\" enable=yes
    {create_service}
    reg add HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\System /f /v SoftwareSASGeneration /t REG_DWORD /d 1
    ", create_service=get_create_service(&exe))
}

pub fn install_me(options: &str, path: String, silent: bool, debug: bool) -> ResultType<()> {
    let uninstall_str = get_uninstall(false, false);
    let mut path = path.trim_end_matches('\\').to_owned();
    let (subkey, _path, start_menu, exe) = get_default_install_info();
    let mut exe = exe;
    if path.is_empty() {
        path = _path;
    } else {
        exe = exe.replace(&_path, &path);
    }
    let mut version_major = "0";
    let mut version_minor = "0";
    let mut version_build = "0";
    let versions: Vec<&str> = crate::VERSION.split(".").collect();
    if versions.len() > 0 {
        version_major = versions[0];
    }
    if versions.len() > 1 {
        version_minor = versions[1];
    }
    if versions.len() > 2 {
        version_build = versions[2];
    }
    let app_name = crate::get_app_name();

    let current_exe = std::env::current_exe()?;

    let tmp_path = std::env::temp_dir().to_string_lossy().to_string();
    let cur_exe = current_exe.to_str().unwrap_or("").to_owned();
    let shortcut_icon_location = get_shortcut_icon_location(&path, &cur_exe);
    let mk_shortcut = write_cmds(
        format!(
            "
Set oWS = WScript.CreateObject(\"WScript.Shell\")
sLinkFile = \"{tmp_path}\\{app_name}.lnk\"

Set oLink = oWS.CreateShortcut(sLinkFile)
    oLink.TargetPath = \"{exe}\"
    {shortcut_icon_location}
oLink.Save
        "
        ),
        "vbs",
        "mk_shortcut",
    )?
    .to_str()
    .unwrap_or("")
    .to_owned();
    // https://superuser.com/questions/392061/how-to-make-a-shortcut-from-cmd
    let uninstall_shortcut = write_cmds(
        format!(
            "
Set oWS = WScript.CreateObject(\"WScript.Shell\")
sLinkFile = \"{tmp_path}\\Uninstall {app_name}.lnk\"
Set oLink = oWS.CreateShortcut(sLinkFile)
    oLink.TargetPath = \"{exe}\"
    oLink.Arguments = \"--uninstall\"
    oLink.IconLocation = \"msiexec.exe\"
oLink.Save
        "
        ),
        "vbs",
        "uninstall_shortcut",
    )?
    .to_str()
    .unwrap_or("")
    .to_owned();
    let tray_shortcut = get_tray_shortcut(&path, &exe, &cur_exe, &tmp_path)?;
    let mut reg_value_desktop_shortcuts = "0".to_owned();
    let mut reg_value_start_menu_shortcuts = "0".to_owned();
    let mut reg_value_printer = "0".to_owned();
    let mut shortcuts = Default::default();
    if options.contains("desktopicon") {
        shortcuts = format!(
            "copy /Y \"{}\\{}.lnk\" \"%PUBLIC%\\Desktop\\\"",
            tmp_path,
            crate::get_app_name()
        );
        reg_value_desktop_shortcuts = "1".to_owned();
    }
    if options.contains("startmenu") {
        shortcuts = format!(
            "{shortcuts}
md \"{start_menu}\"
copy /Y \"{tmp_path}\\{app_name}.lnk\" \"{start_menu}\\\"
copy /Y \"{tmp_path}\\Uninstall {app_name}.lnk\" \"{start_menu}\\\"
     "
        );
        reg_value_start_menu_shortcuts = "1".to_owned();
    }
    let install_printer = options.contains("printer") && is_win_10_or_greater();
    if install_printer {
        reg_value_printer = "1".to_owned();
    }

    let meta = std::fs::symlink_metadata(&current_exe)?;
    let mut size = meta.len() / 1024;
    if let Some(parent_dir) = current_exe.parent() {
        if let Some(d) = parent_dir.to_str() {
            size = get_directory_size_kb(d);
        }
    }
    // https://docs.microsoft.com/zh-cn/windows/win32/msi/uninstall-registry-key?redirectedfrom=MSDNa
    // https://www.windowscentral.com/how-edit-registry-using-command-prompt-windows-10
    // https://www.tenforums.com/tutorials/70903-add-remove-allowed-apps-through-windows-firewall-windows-10-a.html
    // Note: without if exist, the bat may exit in advance on some Windows7 https://github.com/rustdesk/rustdesk/issues/895
    let dels = format!(
        "
if exist \"{mk_shortcut}\" del /f /q \"{mk_shortcut}\"
if exist \"{uninstall_shortcut}\" del /f /q \"{uninstall_shortcut}\"
if exist \"{tray_shortcut}\" del /f /q \"{tray_shortcut}\"
if exist \"{tmp_path}\\{app_name}.lnk\" del /f /q \"{tmp_path}\\{app_name}.lnk\"
if exist \"{tmp_path}\\Uninstall {app_name}.lnk\" del /f /q \"{tmp_path}\\Uninstall {app_name}.lnk\"
if exist \"{tmp_path}\\{app_name} Tray.lnk\" del /f /q \"{tmp_path}\\{app_name} Tray.lnk\"
        "
    );
    let src_exe = std::env::current_exe()?.to_str().unwrap_or("").to_string();

    // potential bug here: if run_cmd cancelled, but config file is changed.
    if let Some(lic) = get_license() {
        Config::set_option("key".into(), lic.key);
        Config::set_option("custom-rendezvous-server".into(), lic.host);
        Config::set_option("api-server".into(), lic.api);
    }

    let tray_shortcuts = if config::is_outgoing_only() {
        "".to_owned()
    } else {
        format!("
cscript \"{tray_shortcut}\"
copy /Y \"{tmp_path}\\{app_name} Tray.lnk\" \"%PROGRAMDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\\"
")
    };

    let install_remote_printer = if install_printer {
        // No need to use `|| true` here.
        // The script will not exit even if `--install-remote-printer` panics.
        format!("\"{}\" --install-remote-printer", &src_exe)
    } else if is_win_10_or_greater() {
        format!("\"{}\" --uninstall-remote-printer", &src_exe)
    } else {
        "".to_owned()
    };

    // Remember to check if `update_me` need to be changed if changing the `cmds`.
    // No need to merge the existing dup code, because the code in these two functions are too critical.
    // New code should be written in a common function.
    let cmds = format!(
        "
{uninstall_str}
chcp 65001
md \"{path}\"
{copy_exe}
reg add {subkey} /f
reg add {subkey} /f /v DisplayIcon /t REG_SZ /d \"{display_icon}\"
reg add {subkey} /f /v DisplayName /t REG_SZ /d \"{app_name}\"
reg add {subkey} /f /v DisplayVersion /t REG_SZ /d \"{version}\"
reg add {subkey} /f /v Version /t REG_SZ /d \"{version}\"
reg add {subkey} /f /v BuildDate /t REG_SZ /d \"{build_date}\"
reg add {subkey} /f /v InstallLocation /t REG_SZ /d \"{path}\"
reg add {subkey} /f /v Publisher /t REG_SZ /d \"{app_name}\"
reg add {subkey} /f /v VersionMajor /t REG_DWORD /d {version_major}
reg add {subkey} /f /v VersionMinor /t REG_DWORD /d {version_minor}
reg add {subkey} /f /v VersionBuild /t REG_DWORD /d {version_build}
reg add {subkey} /f /v UninstallString /t REG_SZ /d \"\\\"{exe}\\\" --uninstall\"
reg add {subkey} /f /v EstimatedSize /t REG_DWORD /d {size}
reg add {subkey} /f /v WindowsInstaller /t REG_DWORD /d 0
cscript \"{mk_shortcut}\"
cscript \"{uninstall_shortcut}\"
{tray_shortcuts}
{shortcuts}
copy /Y \"{tmp_path}\\Uninstall {app_name}.lnk\" \"{path}\\\"
{dels}
{import_config}
{after_install}
{install_remote_printer}
{sleep}
    ",
        display_icon = get_custom_icon(&path, &cur_exe).unwrap_or(exe.to_string()),
        version = crate::VERSION.replace("-", "."),
        build_date = crate::BUILD_DATE,
        after_install = get_after_install(
            &exe,
            Some(reg_value_start_menu_shortcuts),
            Some(reg_value_desktop_shortcuts),
            Some(reg_value_printer)
        ),
        sleep = if debug { "timeout 300" } else { "" },
        dels = if debug { "" } else { &dels },
        copy_exe = copy_exe_cmd(&src_exe, &exe, &path)?,
        import_config = get_import_config(&exe),
    );
    run_cmds(cmds, debug, "install")?;
    run_after_run_cmds(silent);
    Ok(())
}

pub fn run_after_install() -> ResultType<()> {
    let (_, _, _, exe) = get_install_info();
    run_cmds(
        get_after_install(&exe, None, None, None),
        true,
        "after_install",
    )
}

pub fn run_before_uninstall() -> ResultType<()> {
    run_cmds(get_before_uninstall(true), true, "before_install")
}

fn get_before_uninstall(kill_self: bool) -> String {
    let app_name = crate::get_app_name();
    let ext = app_name.to_lowercase();
    let filter = if kill_self {
        "".to_string()
    } else {
        format!(" /FI \"PID ne {}\"", get_current_pid())
    };
    format!(
        "
    chcp 65001
    sc stop {app_name}
    sc delete {app_name}
    taskkill /F /IM {broker_exe}
    taskkill /F /IM {app_name}.exe{filter}
    reg delete HKEY_CLASSES_ROOT\\.{ext} /f
    reg delete HKEY_CLASSES_ROOT\\{ext} /f
    netsh advfirewall firewall delete rule name=\"{app_name} Service\"
    ",
        broker_exe = WIN_TOPMOST_INJECTED_PROCESS_EXE,
    )
}

/// Constructs the uninstall command string for the application.
///
/// # Parameters
/// - `kill_self`: The command will kill the process of current app name. If `true`, it will kill
///   the current process as well. If `false`, it will exclude the current process from the kill
///   command.
/// - `uninstall_printer`: If `true`, includes commands to uninstall the remote printer.
///
/// # Details
/// The `uninstall_printer` parameter determines whether the command to uninstall the remote printer
/// is included in the generated uninstall script. If `uninstall_printer` is `false`, the printer
/// related command is omitted from the script.
fn get_uninstall(kill_self: bool, uninstall_printer: bool) -> String {
    let reg_uninstall_string = get_reg("UninstallString");
    if reg_uninstall_string.to_lowercase().contains("msiexec.exe") {
        return reg_uninstall_string;
    }

    let mut uninstall_cert_cmd = "".to_string();
    let mut uninstall_printer_cmd = "".to_string();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_path) = exe.to_str() {
            uninstall_cert_cmd = format!("\"{}\" --uninstall-cert", exe_path);
            if uninstall_printer {
                uninstall_printer_cmd = format!("\"{}\" --uninstall-remote-printer", &exe_path);
            }
        }
    }
    let (subkey, path, start_menu, _) = get_install_info();
    format!(
        "
    {before_uninstall}
    {uninstall_printer_cmd}
    {uninstall_cert_cmd}
    reg delete {subkey} /f
    {uninstall_amyuni_idd}
    if exist \"{path}\" rd /s /q \"{path}\"
    if exist \"{start_menu}\" rd /s /q \"{start_menu}\"
    if exist \"%PUBLIC%\\Desktop\\{app_name}.lnk\" del /f /q \"%PUBLIC%\\Desktop\\{app_name}.lnk\"
    if exist \"%PROGRAMDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\{app_name} Tray.lnk\" del /f /q \"%PROGRAMDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\{app_name} Tray.lnk\"
    ",
        before_uninstall=get_before_uninstall(kill_self),
        uninstall_amyuni_idd=get_uninstall_amyuni_idd(),
        app_name = crate::get_app_name(),
    )
}

pub fn uninstall_me(kill_self: bool) -> ResultType<()> {
    run_cmds(get_uninstall(kill_self, true), true, "uninstall")
}

fn write_cmds(cmds: String, ext: &str, tip: &str) -> ResultType<std::path::PathBuf> {
    let mut cmds = cmds;
    let mut tmp = std::env::temp_dir();
    // When dir contains these characters, the bat file will not execute in elevated mode.
    if vec!["&", "@", "^"]
        .drain(..)
        .any(|s| tmp.to_string_lossy().to_string().contains(s))
    {
        if let Ok(dir) = user_accessible_folder() {
            tmp = dir;
        }
    }
    tmp.push(format!("{}_{}.{}", crate::get_app_name(), tip, ext));
    let mut file = std::fs::File::create(&tmp)?;
    if ext == "bat" {
        let tmp2 = get_undone_file(&tmp)?;
        std::fs::File::create(&tmp2).ok();
        cmds = format!(
            "
{cmds}
if exist \"{path}\" del /f /q \"{path}\"
",
            path = tmp2.to_string_lossy()
        );
    }
    // in case cmds mixed with \r\n and \n, make sure all ending with \r\n
    // in some windows, \r\n required for cmd file to run
    cmds = cmds.replace("\r\n", "\n").replace("\n", "\r\n");
    if ext == "vbs" {
        let mut v: Vec<u16> = cmds.encode_utf16().collect();
        // utf8 -> utf16le which vbs support it only
        file.write_all(to_le(&mut v))?;
    } else {
        file.write_all(cmds.as_bytes())?;
    }
    file.sync_all()?;
    return Ok(tmp);
}

fn to_le(v: &mut [u16]) -> &[u8] {
    for b in v.iter_mut() {
        *b = b.to_le()
    }
    unsafe { v.align_to().1 }
}

fn get_undone_file(tmp: &Path) -> ResultType<PathBuf> {
    Ok(tmp.with_file_name(format!(
        "{}.undone",
        tmp.file_name()
            .ok_or(anyhow!("Failed to get filename of {:?}", tmp))?
            .to_string_lossy()
    )))
}

fn run_cmds(cmds: String, show: bool, tip: &str) -> ResultType<()> {
    let tmp = write_cmds(cmds, "bat", tip)?;
    let tmp2 = get_undone_file(&tmp)?;
    let tmp_fn = tmp.to_str().unwrap_or("");
    // https://github.com/rustdesk/rustdesk/issues/6786#issuecomment-1879655410
    // Specify cmd.exe explicitly to avoid the replacement of cmd commands.
    let res = runas::Command::new("cmd.exe")
        .args(&["/C", &tmp_fn])
        .show(show)
        .force_prompt(true)
        .status();
    if !show {
        allow_err!(std::fs::remove_file(tmp));
    }
    let _ = res?;
    if tmp2.exists() {
        allow_err!(std::fs::remove_file(tmp2));
        bail!("{} failed", tip);
    }
    Ok(())
}

pub fn toggle_blank_screen(v: bool) {
    let v = if v { TRUE } else { FALSE };
    unsafe {
        blank_screen(v);
    }
}

pub fn block_input(v: bool) -> (bool, String) {
    let v = if v { TRUE } else { FALSE };
    unsafe {
        if BlockInput(v) == TRUE {
            (true, "".to_owned())
        } else {
            (false, format!("Error: {}", io::Error::last_os_error()))
        }
    }
}

pub fn add_recent_document(path: &str) {
    extern "C" {
        fn AddRecentDocument(path: *const u16);
    }
    use std::os::windows::ffi::OsStrExt;
    let wstr: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(Some(0).into_iter())
        .collect();
    let wstr = wstr.as_ptr();
    unsafe {
        AddRecentDocument(wstr);
    }
}

pub fn is_installed() -> bool {
    let (_, _, _, exe) = get_install_info();
    std::fs::metadata(exe).is_ok()
}

pub fn get_reg(name: &str) -> String {
    let (subkey, _, _, _) = get_install_info();
    get_reg_of(&subkey, name)
}

fn get_reg_of(subkey: &str, name: &str) -> String {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(tmp) = hklm.open_subkey(subkey.replace("HKEY_LOCAL_MACHINE\\", "")) {
        if let Ok(v) = tmp.get_value(name) {
            return v;
        }
    }
    "".to_owned()
}

fn get_public_base_dir() -> PathBuf {
    if let Ok(allusersprofile) = std::env::var("ALLUSERSPROFILE") {
        let path = PathBuf::from(&allusersprofile);
        if path.exists() {
            return path;
        }
    }
    if let Ok(public) = std::env::var("PUBLIC") {
        let path = PathBuf::from(public).join("Documents");
        if path.exists() {
            return path;
        }
    }
    let program_data_dir = PathBuf::from("C:\\ProgramData");
    if program_data_dir.exists() {
        return program_data_dir;
    }
    std::env::temp_dir()
}

#[inline]
pub fn get_custom_client_staging_dir() -> PathBuf {
    get_public_base_dir()
        .join("RustDesk")
        .join("RustDeskCustomClientStaging")
}

/// Removes the custom client staging directory.
///
/// Current behavior: intentionally a no-op (does not delete).
///
/// Rationale
/// - The staging directory only contains a small `custom.txt`, leaving it is harmless.
/// - Deleting directories under a public location (e.g., C:\\ProgramData\\RustDesk) is
///   susceptible to TOCTOU attacks if an unprivileged user can replace the path with a
///   symlink/junction between checks and deletion.
///
/// Future work:
/// - Use the files (if needed) in the installation directory instead of a public location.
///   This directory only contains a small `custom.txt` file.
/// - Pass the custom client name directly via command line
///   or environment variable during update installation. Then no staging directory is needed.
#[inline]
pub fn remove_custom_client_staging_dir(staging_dir: &Path) -> ResultType<bool> {
    if !staging_dir.exists() {
        return Ok(false);
    }

    // First explicitly removes `custom.txt` to ensure stale config is never replayed,
    // even if the subsequent directory removal fails.
    //
    // `std::fs::remove_file` on a symlink removes the symlink itself, not the target,
    // so this is safe even in a TOCTOU race.
    let custom_txt_path = staging_dir.join("custom.txt");
    if custom_txt_path.exists() {
        allow_err!(std::fs::remove_file(&custom_txt_path));
    }

    // Intentionally not deleting. See the function docs for rationale.
    log::debug!(
        "Skip deleting staging directory {:?} (intentional to avoid TOCTOU)",
        staging_dir
    );
    Ok(false)
}

// Prepare custom client update by copying staged custom.txt to current directory and loading it.
// Returns:
// 1. Ok(true) if preparation was successful or no staging directory exists.
// 2. Ok(false) if custom.txt file exists but has invalid contents or fails security checks
//    (e.g., is a symlink or has invalid contents).
// 3. Err if any unexpected error occurs during file operations.
pub fn prepare_custom_client_update() -> ResultType<bool> {
    let custom_client_staging_dir = get_custom_client_staging_dir();
    let current_exe = std::env::current_exe()?;
    let current_exe_dir = current_exe
        .parent()
        .ok_or(anyhow!("Cannot get parent directory of current exe"))?;

    let staging_dir = custom_client_staging_dir.clone();
    let clear_staging_on_exit = crate::SimpleCallOnReturn {
        b: true,
        f: Box::new(
            move || match remove_custom_client_staging_dir(&staging_dir) {
                Ok(existed) => {
                    if existed {
                        log::info!("Custom client staging directory removed successfully.");
                    }
                }
                Err(e) => {
                    log::error!(
                        "Failed to remove custom client staging directory {:?}: {}",
                        staging_dir,
                        e
                    );
                }
            },
        ),
    };

    if custom_client_staging_dir.exists() {
        let custom_txt_path = custom_client_staging_dir.join("custom.txt");
        if !custom_txt_path.exists() {
            return Ok(true);
        }

        let metadata = std::fs::symlink_metadata(&custom_txt_path)?;
        if metadata.is_symlink() {
            log::error!(
                "custom.txt is a symlink. Refusing to load custom client for security reasons."
            );
            drop(clear_staging_on_exit);
            return Ok(false);
        }
        if metadata.is_file() {
            // Copy custom.txt to current directory
            let local_custom_file_path = current_exe_dir.join("custom.txt");
            log::debug!(
                "Copying staged custom file from {:?} to {:?}",
                custom_txt_path,
                local_custom_file_path
            );

            // No need to check symlink before copying.
            // `load_custom_client()` will fail if the file is not valid.
            fs::copy(&custom_txt_path, &local_custom_file_path)?;
            log::info!("Staged custom client file copied to current directory.");

            // Load custom client
            let is_custom_file_exists =
                local_custom_file_path.exists() && local_custom_file_path.is_file();
            crate::load_custom_client();

            // Remove the copied custom.txt file
            allow_err!(fs::remove_file(&local_custom_file_path));

            // Check if loaded successfully
            if is_custom_file_exists && !crate::common::is_custom_client() {
                // The custom.txt file existed, but its contents are invalid.
                log::error!("Failed to load custom client from custom.txt.");
                drop(clear_staging_on_exit);
                // ERROR_INVALID_DATA
                return Ok(false);
            }
        } else {
            log::info!("No custom client files found in staging directory.");
        }
    } else {
        log::info!(
            "Custom client staging directory {:?} does not exist.",
            custom_client_staging_dir
        );
    }

    Ok(true)
}

pub fn get_license_from_exe_name() -> ResultType<CustomServer> {
    let mut exe = std::env::current_exe()?.to_str().unwrap_or("").to_owned();
    // if defined portable appname entry, replace original executable name with it.
    if let Ok(portable_exe) = std::env::var(PORTABLE_APPNAME_RUNTIME_ENV_KEY) {
        exe = portable_exe;
    }
    get_custom_server_from_string(&exe)
}

// We can't directly use `RegKey::set_value` to update the registry value, because it will fail with `ERROR_ACCESS_DENIED`
// So we have to use `run_cmds` to update the registry value.
pub fn update_install_option(k: &str, v: &str) -> ResultType<()> {
    // Don't update registry if not installed or not server process.
    if !is_installed() || !crate::is_server() {
        return Ok(());
    }
    if ![REG_NAME_INSTALL_PRINTER].contains(&k) || !["0", "1"].contains(&v) {
        return Ok(());
    }
    let app_name = crate::get_app_name();
    let ext = app_name.to_lowercase();
    let cmds =
        format!("chcp 65001 && reg add HKEY_CLASSES_ROOT\\.{ext} /f /v {k} /t REG_SZ /d \"{v}\"");
    run_cmds(cmds, false, "update_install_option")?;
    Ok(())
}

#[inline]
pub fn is_win_server() -> bool {
    unsafe { is_windows_server() > 0 }
}

#[inline]
pub fn is_win_10_or_greater() -> bool {
    unsafe { is_windows_10_or_greater() > 0 }
}

pub fn bootstrap() -> bool {
    if let Ok(lic) = get_license_from_exe_name() {
        *config::EXE_RENDEZVOUS_SERVER.write().unwrap() = lic.host.clone();
    }

    #[cfg(debug_assertions)]
    {
        true
    }
    #[cfg(not(debug_assertions))]
    {
        // This function will cause `'sciter.dll' was not found neither in PATH nor near the current executable.` when debugging RustDesk.
        // Only call set_safe_load_dll() on Windows 10 or greater
        if is_win_10_or_greater() {
            set_safe_load_dll()
        } else {
            true
        }
    }
}

#[cfg(not(debug_assertions))]
fn set_safe_load_dll() -> bool {
    if !unsafe { set_default_dll_directories() } {
        return false;
    }

    // `SetDllDirectoryW` should never fail.
    // https://docs.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setdlldirectoryw
    if unsafe { SetDllDirectoryW(wide_string("").as_ptr()) == FALSE } {
        eprintln!("SetDllDirectoryW failed: {}", io::Error::last_os_error());
        return false;
    }

    true
}

// https://docs.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-setdefaultdlldirectories
#[cfg(not(debug_assertions))]
unsafe fn set_default_dll_directories() -> bool {
    let module = LoadLibraryExW(
        wide_string("Kernel32.dll").as_ptr(),
        0 as _,
        LOAD_LIBRARY_SEARCH_SYSTEM32,
    );
    if module.is_null() {
        return false;
    }

    match CString::new("SetDefaultDllDirectories") {
        Err(e) => {
            eprintln!("CString::new failed: {}", e);
            return false;
        }
        Ok(func_name) => {
            let func = GetProcAddress(module, func_name.as_ptr());
            if func.is_null() {
                eprintln!("GetProcAddress failed: {}", io::Error::last_os_error());
                return false;
            }
            type SetDefaultDllDirectories = unsafe extern "system" fn(DWORD) -> BOOL;
            let func: SetDefaultDllDirectories = std::mem::transmute(func);
            if func(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS) == FALSE {
                eprintln!(
                    "SetDefaultDllDirectories failed: {}",
                    io::Error::last_os_error()
                );
                return false;
            }
        }
    }
    true
}

fn get_custom_icon(install_dir: &str, exe: &str) -> Option<String> {
    const RELATIVE_ICON_PATH: &str = "data\\flutter_assets\\assets\\icon.ico";
    if crate::is_custom_client() {
        if let Some(p) = PathBuf::from(exe).parent() {
            let alter_icon_path = p.join(RELATIVE_ICON_PATH);
            if alter_icon_path.exists() {
                // During installation, files under `install_dir` may not exist yet.
                // So we validate the icon from the current executable directory first.
                // But for shortcut/registry icon location, we should point to the final
                // installed path so the icon works across different Windows users.
                if let Ok(metadata) = std::fs::symlink_metadata(&alter_icon_path) {
                    if metadata.is_symlink() {
                        log::warn!(
                            "Custom icon at {:?} is a symlink, refusing to use it.",
                            alter_icon_path
                        );
                        return None;
                    }
                    if metadata.is_file() {
                        return if install_dir.is_empty() {
                            Some(alter_icon_path.to_string_lossy().to_string())
                        } else {
                            Some(format!("{}\\{}", install_dir, RELATIVE_ICON_PATH))
                        };
                    }
                }
            }
        }
    }
    None
}

#[inline]
fn get_shortcut_icon_location(install_dir: &str, exe: &str) -> String {
    if exe.is_empty() {
        return "".to_owned();
    }

    get_custom_icon(install_dir, exe)
        .map(|p| format!("oLink.IconLocation = \"{}\"", p))
        .unwrap_or_default()
}

pub fn create_shortcut(id: &str) -> ResultType<()> {
    let exe = std::env::current_exe()?.to_str().unwrap_or("").to_owned();
    // https://github.com/rustdesk/rustdesk/issues/13735
    // Replace ':' with '_' for filename since ':' is not allowed in Windows filenames
    // https://github.com/rustdesk/hbb_common/blob/8b0e25867375ba9e6bff548acf44fe6d6ffa7c0e/src/config.rs#L1384
    let filename = id.replace(':', "_");
    let shortcut_icon_location = get_shortcut_icon_location("", &exe);
    let shortcut = write_cmds(
        format!(
            "
Set oWS = WScript.CreateObject(\"WScript.Shell\")
strDesktop = oWS.SpecialFolders(\"Desktop\")
Set objFSO = CreateObject(\"Scripting.FileSystemObject\")
sLinkFile = objFSO.BuildPath(strDesktop, \"{filename}.lnk\")
Set oLink = oWS.CreateShortcut(sLinkFile)
    oLink.TargetPath = \"{exe}\"
    oLink.Arguments = \"--connect {id}\"
    {shortcut_icon_location}
oLink.Save
        "
        ),
        "vbs",
        "connect_shortcut",
    )?
    .to_str()
    .unwrap_or("")
    .to_owned();
    std::process::Command::new("cscript")
        .arg(&shortcut)
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    allow_err!(std::fs::remove_file(shortcut));
    Ok(())
}

pub fn enable_lowlevel_keyboard(hwnd: HWND) {
    let ret = unsafe { win32_enable_lowlevel_keyboard(hwnd) };
    if ret != 0 {
        log::error!("Failure grabbing keyboard");
        return;
    }
}

pub fn disable_lowlevel_keyboard(hwnd: HWND) {
    unsafe { win32_disable_lowlevel_keyboard(hwnd) };
}

pub fn stop_system_key_propagate(v: bool) {
    unsafe { win_stop_system_key_propagate(if v { TRUE } else { FALSE }) };
}

pub fn get_win_key_state() -> bool {
    unsafe { is_win_down() == TRUE }
}

pub fn quit_gui() {
    std::process::exit(0);
    // unsafe { PostQuitMessage(0) }; // some how not work
}

pub fn get_user_token(session_id: u32, as_user: bool) -> HANDLE {
    let mut token = NULL as HANDLE;
    unsafe {
        let mut _token_pid = 0;
        if FALSE
            == GetSessionUserTokenWin(
                &mut token as _,
                session_id,
                if as_user { TRUE } else { FALSE },
                &mut _token_pid,
            )
        {
            NULL as _
        } else {
            token
        }
    }
}

pub fn run_background(exe: &str, arg: &str) -> ResultType<bool> {
    let wexe = wide_string(exe);
    let warg;
    unsafe {
        let ret = ShellExecuteW(
            NULL as _,
            NULL as _,
            wexe.as_ptr() as _,
            if arg.is_empty() {
                NULL as _
            } else {
                warg = wide_string(arg);
                warg.as_ptr() as _
            },
            NULL as _,
            SW_HIDE,
        );
        return Ok(ret as i32 > 32);
    }
}

pub fn run_uac(exe: &str, arg: &str) -> ResultType<bool> {
    let wop = wide_string("runas");
    let wexe = wide_string(exe);
    let warg;
    unsafe {
        let ret = ShellExecuteW(
            NULL as _,
            wop.as_ptr() as _,
            wexe.as_ptr() as _,
            if arg.is_empty() {
                NULL as _
            } else {
                warg = wide_string(arg);
                warg.as_ptr() as _
            },
            NULL as _,
            SW_SHOWNORMAL,
        );
        return Ok(ret as i32 > 32);
    }
}

pub fn check_super_user_permission() -> ResultType<bool> {
    run_uac(
        std::env::current_exe()?
            .to_string_lossy()
            .to_string()
            .as_str(),
        "--version",
    )
}

pub fn elevate(arg: &str) -> ResultType<bool> {
    run_uac(
        std::env::current_exe()?
            .to_string_lossy()
            .to_string()
            .as_str(),
        arg,
    )
}

pub fn run_as_system(arg: &str) -> ResultType<()> {
    let exe = std::env::current_exe()?.to_string_lossy().to_string();
    if impersonate_system::run_as_system(&exe, arg).is_err() {
        bail!(format!("Failed to run {} as system", exe));
    }
    Ok(())
}

pub fn elevate_or_run_as_system(is_setup: bool, is_elevate: bool, is_run_as_system: bool) {
    // avoid possible run recursively due to failed run.
    log::info!(
        "elevate: {} -> {:?}, run_as_system: {} -> {}",
        is_elevate,
        is_elevated(None),
        is_run_as_system,
        crate::username(),
    );
    let arg_elevate = if is_setup {
        "--noinstall --elevate"
    } else {
        "--elevate"
    };
    let arg_run_as_system = if is_setup {
        "--noinstall --run-as-system"
    } else {
        "--run-as-system"
    };
    if is_root() {
        if is_run_as_system {
            log::info!("run portable service");
            crate::portable_service::server::run_portable_service();
        }
    } else {
        match is_elevated(None) {
            Ok(elevated) => {
                if elevated {
                    if !is_run_as_system {
                        if run_as_system(arg_run_as_system).is_ok() {
                            std::process::exit(0);
                        } else {
                            log::error!(
                                "Failed to run as system, error {}",
                                io::Error::last_os_error()
                            );
                        }
                    }
                } else {
                    if !is_elevate {
                        if let Ok(true) = elevate(arg_elevate) {
                            std::process::exit(0);
                        } else {
                            log::error!("Failed to elevate, error {}", io::Error::last_os_error());
                        }
                    }
                }
            }
            Err(_) => log::error!(
                "Failed to get elevation status, error {}",
                io::Error::last_os_error()
            ),
        }
    }
}

pub fn is_elevated(process_id: Option<DWORD>) -> ResultType<bool> {
    use hbb_common::platform::windows::RAIIHandle;
    unsafe {
        let handle: HANDLE = match process_id {
            Some(process_id) => OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, process_id),
            None => GetCurrentProcess(),
        };
        if handle == NULL {
            bail!(
                "Failed to open process, error {}",
                io::Error::last_os_error()
            )
        }
        let _handle = RAIIHandle(handle);
        let mut token: HANDLE = mem::zeroed();
        if OpenProcessToken(handle, TOKEN_QUERY, &mut token) == FALSE {
            bail!(
                "Failed to open process token, error {}",
                io::Error::last_os_error()
            )
        }
        let _token = RAIIHandle(token);
        let mut token_elevation: TOKEN_ELEVATION = mem::zeroed();
        let mut size: DWORD = 0;
        if GetTokenInformation(
            token,
            TokenElevation,
            (&mut token_elevation) as *mut _ as *mut c_void,
            mem::size_of::<TOKEN_ELEVATION>() as _,
            &mut size,
        ) == FALSE
        {
            bail!(
                "Failed to get token information, error {}",
                io::Error::last_os_error()
            )
        }

        Ok(token_elevation.TokenIsElevated != 0)
    }
}

pub fn is_foreground_window_elevated() -> ResultType<bool> {
    unsafe {
        let mut process_id: DWORD = 0;
        GetWindowThreadProcessId(GetForegroundWindow(), &mut process_id);
        if process_id == 0 {
            bail!(
                "Failed to get processId, error {}",
                io::Error::last_os_error()
            )
        }
        is_elevated(Some(process_id))
    }
}

fn get_current_pid() -> u32 {
    unsafe { GetCurrentProcessId() }
}

pub fn get_double_click_time() -> u32 {
    unsafe { GetDoubleClickTime() }
}

pub fn wide_string(s: &str) -> Vec<u16> {
    use std::os::windows::prelude::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(Some(0).into_iter())
        .collect()
}

/// send message to currently shown window
pub fn send_message_to_hnwd(
    class_name: &str,
    window_name: &str,
    dw_data: usize,
    data: &str,
    show_window: bool,
) -> bool {
    unsafe {
        let class_name_utf16 = wide_string(class_name);
        let window_name_utf16 = wide_string(window_name);
        let window = FindWindowW(class_name_utf16.as_ptr(), window_name_utf16.as_ptr());
        if window.is_null() {
            log::warn!("no such window {}:{}", class_name, window_name);
            return false;
        }
        let mut data_struct = COPYDATASTRUCT::default();
        data_struct.dwData = dw_data;
        let mut data_zero: String = data.chars().chain(Some('\0').into_iter()).collect();
        println!("send {:?}", data_zero);
        data_struct.cbData = data_zero.len() as _;
        data_struct.lpData = data_zero.as_mut_ptr() as _;
        SendMessageW(
            window,
            WM_COPYDATA,
            0,
            &data_struct as *const COPYDATASTRUCT as _,
        );
        if show_window {
            ShowWindow(window, SW_NORMAL);
            SetForegroundWindow(window);
        }
    }
    return true;
}

pub fn get_logon_user_token(user: &str, pwd: &str) -> ResultType<HANDLE> {
    let user_split = user.split("\\").collect::<Vec<&str>>();
    let wuser = wide_string(user_split.get(1).unwrap_or(&user));
    let wpc = wide_string(user_split.get(0).unwrap_or(&""));
    let wpwd = wide_string(pwd);
    let mut ph_token: HANDLE = std::ptr::null_mut();
    let res = unsafe {
        LogonUserW(
            wuser.as_ptr(),
            wpc.as_ptr(),
            wpwd.as_ptr(),
            LOGON32_LOGON_INTERACTIVE,
            LOGON32_PROVIDER_DEFAULT,
            &mut ph_token as _,
        )
    };
    if res == FALSE {
        bail!(
            "Failed to log on user {}: {}",
            user,
            std::io::Error::last_os_error()
        );
    } else {
        if ph_token.is_null() {
            bail!(
                "Failed to log on user {}: {}",
                user,
                std::io::Error::last_os_error()
            );
        }
        Ok(ph_token)
    }
}

// Ensure the token returned is a primary token.
// If the provided token is an impersonation token, it duplicates it to a primary token.
// If the provided token is already a primary token, it returns it as is.
// The caller is responsible for closing the returned token handle.
pub fn ensure_primary_token(user_token: HANDLE) -> ResultType<HANDLE> {
    if user_token.is_null() || user_token == INVALID_HANDLE_VALUE {
        bail!("Invalid user token provided");
    }

    unsafe {
        let mut token_type: TOKEN_TYPE = 0;
        let mut return_length: DWORD = 0;

        if GetTokenInformation(
            user_token,
            TokenType,
            &mut token_type as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_TYPE>() as DWORD,
            &mut return_length,
        ) == FALSE
        {
            bail!(
                "Failed to get token type, error {}",
                io::Error::last_os_error()
            );
        }

        if token_type == TokenImpersonation {
            let mut duplicate_token: HANDLE = std::ptr::null_mut();
            let dup_res = DuplicateToken(user_token, SecurityImpersonation, &mut duplicate_token);
            CloseHandle(user_token);
            if dup_res == FALSE {
                bail!(
                    "Failed to duplicate token, error {}",
                    io::Error::last_os_error()
                );
            }
            Ok(duplicate_token)
        } else {
            Ok(user_token)
        }
    }
}

pub fn is_user_token_admin(user_token: HANDLE) -> ResultType<bool> {
    if user_token.is_null() || user_token == INVALID_HANDLE_VALUE {
        bail!("Invalid user token provided");
    }

    unsafe {
        let mut dw_size: DWORD = 0;
        GetTokenInformation(
            user_token,
            TokenGroups,
            std::ptr::null_mut(),
            0,
            &mut dw_size,
        );

        let last_error = GetLastError();
        if last_error != ERROR_INSUFFICIENT_BUFFER {
            bail!(
                "Failed to get token groups buffer size, error: {}",
                last_error
            );
        }
        if dw_size == 0 {
            bail!("Token groups buffer size is zero");
        }

        let mut buffer = vec![0u8; dw_size as usize];
        if GetTokenInformation(
            user_token,
            TokenGroups,
            buffer.as_mut_ptr() as *mut _,
            dw_size,
            &mut dw_size,
        ) == FALSE
        {
            bail!(
                "Failed to get token groups information, error: {}",
                io::Error::last_os_error()
            );
        }

        let p_token_groups = buffer.as_ptr() as *const TOKEN_GROUPS;
        let group_count = (*p_token_groups).GroupCount;

        if group_count == 0 {
            return Ok(false);
        }

        let mut nt_authority: SID_IDENTIFIER_AUTHORITY = SID_IDENTIFIER_AUTHORITY {
            Value: SECURITY_NT_AUTHORITY,
        };
        let mut administrators_group: PSID = std::ptr::null_mut();
        if AllocateAndInitializeSid(
            &mut nt_authority,
            2,
            SECURITY_BUILTIN_DOMAIN_RID,
            DOMAIN_ALIAS_RID_ADMINS,
            0,
            0,
            0,
            0,
            0,
            0,
            &mut administrators_group,
        ) == FALSE
        {
            bail!(
                "Failed to allocate administrators group SID, error: {}",
                io::Error::last_os_error()
            );
        }
        if administrators_group.is_null() {
            bail!("Failed to create administrators group SID");
        }

        let mut is_admin = false;
        let groups =
            std::slice::from_raw_parts((*p_token_groups).Groups.as_ptr(), group_count as usize);
        for group in groups {
            if EqualSid(administrators_group, group.Sid) == TRUE {
                is_admin = true;
                break;
            }
        }

        if !administrators_group.is_null() {
            FreeSid(administrators_group);
        }

        Ok(is_admin)
    }
}

pub fn create_process_with_logon(user: &str, pwd: &str, exe: &str, arg: &str) -> ResultType<()> {
    let last_error_table = HashMap::from([
        (
            ERROR_LOGON_FAILURE,
            "The user name or password is incorrect.",
        ),
        (ERROR_ACCESS_DENIED, "Access is denied."),
    ]);

    unsafe {
        let user_split = user.split("\\").collect::<Vec<&str>>();
        let wuser = wide_string(user_split.get(1).unwrap_or(&user));
        let wpc = wide_string(user_split.get(0).unwrap_or(&""));
        let wpwd = wide_string(pwd);
        let cmd = if arg.is_empty() {
            format!("\"{}\"", exe)
        } else {
            format!("\"{}\" {}", exe, arg)
        };
        let mut wcmd = wide_string(&cmd);
        let mut si: STARTUPINFOW = mem::zeroed();
        si.wShowWindow = SW_HIDE as _;
        si.lpDesktop = NULL as _;
        si.cb = std::mem::size_of::<STARTUPINFOW>() as _;
        si.dwFlags = STARTF_USESHOWWINDOW;
        let mut pi: PROCESS_INFORMATION = mem::zeroed();
        let wexe = wide_string(exe);
        if FALSE
            == CreateProcessWithLogonW(
                wuser.as_ptr(),
                wpc.as_ptr(),
                wpwd.as_ptr(),
                LOGON_WITH_PROFILE,
                wexe.as_ptr(),
                wcmd.as_mut_ptr(),
                CREATE_UNICODE_ENVIRONMENT,
                NULL,
                NULL as _,
                &mut si as *mut STARTUPINFOW,
                &mut pi as *mut PROCESS_INFORMATION,
            )
        {
            let last_error = GetLastError();
            bail!(
                "CreateProcessWithLogonW failed : \"{}\", error {}",
                last_error_table
                    .get(&last_error)
                    .unwrap_or(&"Unknown error"),
                io::Error::from_raw_os_error(last_error as _)
            );
        }
    }
    return Ok(());
}

pub fn set_path_permission(dir: &Path, permission: &str) -> ResultType<()> {
    std::process::Command::new("icacls")
        .arg(dir.as_os_str())
        .arg("/grant")
        .arg(format!("*S-1-1-0:(OI)(CI){}", permission))
        .arg("/T")
        .spawn()?;
    Ok(())
}

#[inline]
fn str_to_device_name(name: &str) -> [u16; 32] {
    let mut device_name: Vec<u16> = wide_string(name);
    if device_name.len() < 32 {
        device_name.resize(32, 0);
    }
    let mut result = [0; 32];
    result.copy_from_slice(&device_name[..32]);
    result
}

pub fn resolutions(name: &str) -> Vec<Resolution> {
    unsafe {
        let mut dm: DEVMODEW = std::mem::zeroed();
        let mut v = vec![];
        let mut num = 0;
        let device_name = str_to_device_name(name);
        loop {
            if EnumDisplaySettingsW(device_name.as_ptr(), num, &mut dm) == 0 {
                break;
            }
            let r = Resolution {
                width: dm.dmPelsWidth as _,
                height: dm.dmPelsHeight as _,
                ..Default::default()
            };
            if !v.contains(&r) {
                v.push(r);
            }
            num += 1;
        }
        v
    }
}

pub fn current_resolution(name: &str) -> ResultType<Resolution> {
    let device_name = str_to_device_name(name);
    unsafe {
        let mut dm: DEVMODEW = std::mem::zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as _;
        if EnumDisplaySettingsW(device_name.as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm) == 0 {
            bail!(
                "failed to get current resolution, error {}",
                io::Error::last_os_error()
            );
        }
        let r = Resolution {
            width: dm.dmPelsWidth as _,
            height: dm.dmPelsHeight as _,
            ..Default::default()
        };
        Ok(r)
    }
}

pub(super) fn change_resolution_directly(
    name: &str,
    width: usize,
    height: usize,
) -> ResultType<()> {
    let device_name = str_to_device_name(name);
    unsafe {
        let mut dm: DEVMODEW = std::mem::zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as _;
        dm.dmPelsWidth = width as _;
        dm.dmPelsHeight = height as _;
        dm.dmFields = DM_PELSHEIGHT | DM_PELSWIDTH;
        let res = ChangeDisplaySettingsExW(
            device_name.as_ptr(),
            &mut dm,
            NULL as _,
            CDS_UPDATEREGISTRY | CDS_GLOBAL | CDS_RESET,
            NULL,
        );
        if res != DISP_CHANGE_SUCCESSFUL {
            bail!(
                "ChangeDisplaySettingsExW failed, res={}, error {}",
                res,
                io::Error::last_os_error()
            );
        }
        Ok(())
    }
}

pub fn user_accessible_folder() -> ResultType<PathBuf> {
    let disk = std::env::var("SystemDrive").unwrap_or("C:".to_string());
    let dir1 = PathBuf::from(format!("{}\\ProgramData", disk));
    // NOTICE: "C:\Windows\Temp" requires permanent authorization.
    let dir2 = PathBuf::from(format!("{}\\Windows\\Temp", disk));
    let dir;
    if dir1.exists() {
        dir = dir1;
    } else if dir2.exists() {
        dir = dir2;
    } else {
        bail!("no valid user accessible folder");
    }
    Ok(dir)
}

#[inline]
pub fn uninstall_cert() -> ResultType<()> {
    cert::uninstall_cert()
}

mod cert {
    use hbb_common::ResultType;

    extern "C" {
        fn DeleteRustDeskTestCertsW();
    }
    pub fn uninstall_cert() -> ResultType<()> {
        unsafe {
            DeleteRustDeskTestCertsW();
        }
        Ok(())
    }
}

#[inline]
pub fn get_char_from_vk(vk: u32) -> Option<char> {
    get_char_from_unicode(get_unicode_from_vk(vk)?)
}

pub fn get_char_from_unicode(unicode: u16) -> Option<char> {
    let buff = [unicode];
    if let Some(chr) = String::from_utf16(&buff[..1]).ok()?.chars().next() {
        if chr.is_control() {
            return None;
        } else {
            Some(chr)
        }
    } else {
        None
    }
}

pub fn get_unicode_from_vk(vk: u32) -> Option<u16> {
    const BUF_LEN: i32 = 32;
    let mut buff = [0_u16; BUF_LEN as usize];
    let buff_ptr = buff.as_mut_ptr();
    let len = unsafe {
        let current_window_thread_id = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());
        let layout = GetKeyboardLayout(current_window_thread_id);

        // refs: https://github.com/rustdesk-org/rdev/blob/25a99ce71ab42843ad253dd51e6a35e83e87a8a4/src/windows/keyboard.rs#L115
        let press_state = 129;
        let mut state: [BYTE; 256] = [0; 256];
        let shift_left = rdev::get_modifier(rdev::Key::ShiftLeft);
        let shift_right = rdev::get_modifier(rdev::Key::ShiftRight);
        if shift_left {
            state[VK_LSHIFT as usize] = press_state;
        }
        if shift_right {
            state[VK_RSHIFT as usize] = press_state;
        }
        if shift_left || shift_right {
            state[VK_SHIFT as usize] = press_state;
        }
        ToUnicodeEx(vk, 0x00, &state as _, buff_ptr, BUF_LEN, 0, layout)
    };
    if len == 1 {
        Some(buff[0])
    } else {
        None
    }
}

pub fn is_process_consent_running() -> ResultType<bool> {
    let output = std::process::Command::new("cmd")
        .args(&["/C", "tasklist | findstr consent.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    Ok(output.status.success() && !output.stdout.is_empty())
}

pub struct WakeLock(u32);
// Failed to compile keepawake-rs on i686
impl WakeLock {
    pub fn new(display: bool, idle: bool, sleep: bool) -> Self {
        let mut flag = ES_CONTINUOUS;
        if display {
            flag |= ES_DISPLAY_REQUIRED;
        }
        if idle {
            flag |= ES_SYSTEM_REQUIRED;
        }
        if sleep {
            flag |= ES_AWAYMODE_REQUIRED;
        }
        unsafe { SetThreadExecutionState(flag) };
        WakeLock(flag)
    }

    pub fn set_display(&mut self, display: bool) -> ResultType<()> {
        let flag = if display {
            self.0 | ES_DISPLAY_REQUIRED
        } else {
            self.0 & !ES_DISPLAY_REQUIRED
        };
        if flag != self.0 {
            unsafe { SetThreadExecutionState(flag) };
            self.0 = flag;
        }
        Ok(())
    }
}

impl Drop for WakeLock {
    fn drop(&mut self) {
        unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    }
}

pub fn uninstall_service(show_new_window: bool, _: bool) -> bool {
    log::info!("Uninstalling service...");
    let filter = format!(" /FI \"PID ne {}\"", get_current_pid());
    Config::set_option("stop-service".into(), "Y".into());
    let cmds = format!(
        "
    chcp 65001
    sc stop {app_name}
    sc delete {app_name}
    if exist \"%PROGRAMDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\{app_name} Tray.lnk\" del /f /q \"%PROGRAMDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\{app_name} Tray.lnk\"
    taskkill /F /IM {broker_exe}
    taskkill /F /IM {app_name}.exe{filter}
    ",
        app_name = crate::get_app_name(),
        broker_exe = WIN_TOPMOST_INJECTED_PROCESS_EXE,
    );
    if let Err(err) = run_cmds(cmds, false, "uninstall") {
        Config::set_option("stop-service".into(), "".into());
        log::debug!("{err}");
        return true;
    }
    run_after_run_cmds(!show_new_window);
    std::process::exit(0);
}

pub fn install_service() -> bool {
    log::info!("Installing service...");
    let _installing = crate::platform::InstallingService::new();
    let (_, path, _, exe) = get_install_info();
    let tmp_path = std::env::temp_dir().to_string_lossy().to_string();
    let tray_shortcut = get_tray_shortcut(&path, &exe, &exe, &tmp_path).unwrap_or_default();
    let filter = format!(" /FI \"PID ne {}\"", get_current_pid());
    Config::set_option("stop-service".into(), "".into());
    crate::ipc::EXIT_RECV_CLOSE.store(false, Ordering::Relaxed);
    let cmds = format!(
        "
chcp 65001
taskkill /F /IM {app_name}.exe{filter}
cscript \"{tray_shortcut}\"
copy /Y \"{tmp_path}\\{app_name} Tray.lnk\" \"%PROGRAMDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\\"
{import_config}
{create_service}
if exist \"{tray_shortcut}\" del /f /q \"{tray_shortcut}\"
    ",
        app_name = crate::get_app_name(),
        import_config = get_import_config(&exe),
        create_service = get_create_service(&exe),
    );
    if let Err(err) = run_cmds(cmds, false, "install") {
        Config::set_option("stop-service".into(), "Y".into());
        crate::ipc::EXIT_RECV_CLOSE.store(true, Ordering::Relaxed);
        log::debug!("{err}");
        return true;
    }
    run_after_run_cmds(false);
    std::process::exit(0);
}

/// Calculate the total size of a directory in KB
/// Does not follow symlinks to prevent directory traversal attacks.
fn get_directory_size_kb(path: &str) -> u64 {
    let mut total_size = 0u64;
    let mut stack = vec![PathBuf::from(path)];

    while let Some(current_path) = stack.pop() {
        let entries = match std::fs::read_dir(&current_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            let metadata = match std::fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };

            if metadata.is_symlink() {
                continue;
            }

            if metadata.is_dir() {
                stack.push(entry.path());
            } else {
                total_size = total_size.saturating_add(metadata.len());
            }
        }
    }

    total_size / 1024
}

pub fn update_me(debug: bool) -> ResultType<()> {
    let app_name = crate::get_app_name();
    let src_exe = std::env::current_exe()?.to_string_lossy().to_string();
    let (subkey, path, _, exe) = get_install_info();
    let is_installed = std::fs::metadata(&exe).is_ok();
    if !is_installed {
        bail!("{} is not installed.", &app_name);
    }

    let app_exe_name = &format!("{}.exe", &app_name);
    let main_window_pids =
        crate::platform::get_pids_of_process_with_args::<_, &str>(&app_exe_name, &[]);
    let main_window_sessions = main_window_pids
        .iter()
        .map(|pid| get_session_id_of_process(pid.as_u32()))
        .flatten()
        .collect::<Vec<_>>();
    kill_process_by_pids(&app_exe_name, main_window_pids)?;
    let tray_pids = crate::platform::get_pids_of_process_with_args(&app_exe_name, &["--tray"]);
    let tray_sessions = tray_pids
        .iter()
        .map(|pid| get_session_id_of_process(pid.as_u32()))
        .flatten()
        .collect::<Vec<_>>();
    kill_process_by_pids(&app_exe_name, tray_pids)?;
    let is_service_running = is_self_service_running();

    let mut version_major = "0";
    let mut version_minor = "0";
    let mut version_build = "0";
    let versions: Vec<&str> = crate::VERSION.split(".").collect();
    if versions.len() > 0 {
        version_major = versions[0];
    }
    if versions.len() > 1 {
        version_minor = versions[1];
    }
    if versions.len() > 2 {
        version_build = versions[2];
    }
    let version = crate::VERSION.replace("-", ".");
    let size = get_directory_size_kb(&path);
    let build_date = crate::BUILD_DATE;
    // Use the icon in the previous installation directory if possible.
    let display_icon = get_custom_icon("", &exe).unwrap_or(exe.to_string());

    let is_msi = is_msi_installed().ok();

    fn get_reg_cmd(
        subkey: &str,
        is_msi: Option<bool>,
        display_icon: &str,
        version: &str,
        build_date: &str,
        version_major: &str,
        version_minor: &str,
        version_build: &str,
        size: u64,
    ) -> String {
        let reg_display_icon = if is_msi.unwrap_or(false) {
            "".to_string()
        } else {
            format!(
                "reg add {} /f /v DisplayIcon /t REG_SZ /d \"{}\"",
                subkey, display_icon
            )
        };
        format!(
            "
{reg_display_icon}
reg add {subkey} /f /v DisplayVersion /t REG_SZ /d \"{version}\"
reg add {subkey} /f /v Version /t REG_SZ /d \"{version}\"
reg add {subkey} /f /v BuildDate /t REG_SZ /d \"{build_date}\"
reg add {subkey} /f /v VersionMajor /t REG_DWORD /d {version_major}
reg add {subkey} /f /v VersionMinor /t REG_DWORD /d {version_minor}
reg add {subkey} /f /v VersionBuild /t REG_DWORD /d {version_build}
reg add {subkey} /f /v EstimatedSize /t REG_DWORD /d {size}
        "
        )
    }

    let reg_cmd = {
        let reg_cmd_main = get_reg_cmd(
            &subkey,
            is_msi,
            &display_icon,
            &version,
            &build_date,
            &version_major,
            &version_minor,
            &version_build,
            size,
        );
        let reg_cmd_msi = if let Some(reg_msi_key) = get_reg_msi_key(&subkey, is_msi) {
            get_reg_cmd(
                &reg_msi_key,
                is_msi,
                &display_icon,
                &version,
                &build_date,
                &version_major,
                &version_minor,
                &version_build,
                size,
            )
        } else {
            "".to_owned()
        };
        format!("{}{}", reg_cmd_main, reg_cmd_msi)
    };

    let filter = format!(" /FI \"PID ne {}\"", get_current_pid());
    let restore_service_cmd = if is_service_running {
        format!("sc start {}", &app_name)
    } else {
        "".to_owned()
    };

    // No need to check the install option here, `is_rd_printer_installed` rarely fails.
    let is_printer_installed = remote_printer::is_rd_printer_installed(&app_name).unwrap_or(false);
    // Do nothing if the printer is not installed or failed to query if the printer is installed.
    let (uninstall_printer_cmd, install_printer_cmd) = if is_printer_installed {
        (
            format!("\"{}\" --uninstall-remote-printer", &src_exe),
            format!("\"{}\" --install-remote-printer", &src_exe),
        )
    } else {
        ("".to_owned(), "".to_owned())
    };

    // We do not try to remove all files in the old version.
    // Because I don't know whether additional files will be installed here after installation, such as drivers.
    // Just copy files to the installation directory works fine.
    //if exist \"{path}\" rd /s /q \"{path}\"
    // md \"{path}\"
    //
    // We need `taskkill` because:
    // 1. There may be some other processes like `rustdesk --connect` are running.
    // 2. Sometimes, the main window and the tray icon are showing
    // while I cannot find them by `tasklist` or the methods above.
    // There's should be 4 processes running: service, server, tray and main window.
    // But only 2 processes are shown in the tasklist.
    let cmds = format!(
        "
chcp 65001
sc stop {app_name}
taskkill /F /IM {app_name}.exe{filter}
{reg_cmd}
{copy_exe}
{rename_exe}
{remove_meta_toml}
{restore_service_cmd}
{uninstall_printer_cmd}
{install_printer_cmd}
{sleep}
    ",
        app_name = app_name,
        copy_exe = copy_exe_cmd(&src_exe, &exe, &path)?,
        rename_exe = rename_exe_cmd(&src_exe, &path)?,
        remove_meta_toml = remove_meta_toml_cmd(is_msi.unwrap_or(true), &path),
        sleep = if debug { "timeout 300" } else { "" },
    );

    let _restore_session_guard = crate::common::SimpleCallOnReturn {
        b: true,
        f: Box::new(move || {
            let is_root = is_root();
            if tray_sessions.is_empty() {
                log::info!("No tray process found.");
            } else {
                log::info!(
                    "Try to restore the tray process..., sessions: {:?}",
                    &tray_sessions
                );
                // When not running as root, only spawn once since run_exe_direct
                // doesn't target specific sessions.
                let mut spawned_non_root_tray = false;
                for s in tray_sessions.clone().into_iter() {
                    if s != 0 {
                        // We need to check if is_root here because if `update_me()` is called from
                        // the main window running with administrator permission,
                        // `run_exe_in_session()` will fail with error 1314 ("A required privilege is
                        // not held by the client").
                        //
                        // This issue primarily affects the MSI-installed version running in Administrator
                        // session during testing, but we check permissions here to be safe.
                        if is_root {
                            allow_err!(run_exe_in_session(&exe, vec!["--tray"], s, true));
                        } else if !spawned_non_root_tray {
                            // Only spawn once for non-root since run_exe_direct doesn't take session parameter
                            allow_err!(run_exe_direct(&exe, vec!["--tray"], false));
                            spawned_non_root_tray = true;
                        }
                    }
                }
            }
            if main_window_sessions.is_empty() {
                log::info!("No main window process found.");
            } else {
                log::info!("Try to restore the main window process...");
                std::thread::sleep(std::time::Duration::from_millis(2000));
                // When not running as root, only spawn once since run_exe_direct
                // doesn't target specific sessions.
                let mut spawned_non_root_main = false;
                for s in main_window_sessions.clone().into_iter() {
                    if s != 0 {
                        if is_root {
                            allow_err!(run_exe_in_session(&exe, vec![], s, true));
                        } else if !spawned_non_root_main {
                            // Only spawn once for non-root since run_exe_direct doesn't take session parameter
                            allow_err!(run_exe_direct(&exe, vec![], false));
                            spawned_non_root_main = true;
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }),
    };

    run_cmds(cmds, debug, "update")?;

    std::thread::sleep(std::time::Duration::from_millis(2000));
    log::info!("Update completed.");

    Ok(())
}

fn get_reg_msi_key(subkey: &str, is_msi: Option<bool>) -> Option<String> {
    // Only proceed if it's a custom client and MSI is installed.
    // `is_msi.unwrap_or(true)` is intentional: subsequent code validates the registry,
    // hence no early return is required upon MSI detection failure.
    if !(crate::common::is_custom_client() && is_msi.unwrap_or(true)) {
        return None;
    }

    // Get the uninstall string from registry
    let uninstall_string = get_reg_of(subkey, "UninstallString");
    if uninstall_string.is_empty() {
        return None;
    }

    // Find the product code (GUID) in the uninstall string
    // Handle both quoted and unquoted GUIDs: /X {GUID} or /X "{GUID}"
    let start = uninstall_string.rfind('{')?;
    let end = uninstall_string.rfind('}')?;
    if start >= end {
        return None;
    }
    let product_code = &uninstall_string[start..=end];

    // Build the MSI registry key path
    let pos = subkey.rfind('\\')?;
    let reg_msi_key = format!("{}{}", &subkey[..=pos], product_code);

    Some(reg_msi_key)
}

// Double confirm the process name
fn kill_process_by_pids(name: &str, pids: Vec<Pid>) -> ResultType<()> {
    let name = name.to_lowercase();
    let s = System::new_all();
    // No need to check all names of `pids` first, and kill them then.
    // It's rare case that they're not matched.
    for pid in pids {
        if let Some(process) = s.process(pid) {
            if process.name().to_lowercase() != name {
                bail!("Failed to kill the process, the names are mismatched.");
            }
            if !process.kill() {
                bail!("Failed to kill the process");
            }
        } else {
            bail!("Failed to kill the process, the pid is not found");
        }
    }
    Ok(())
}

pub fn handle_custom_client_staging_dir_before_update(
    custom_client_staging_dir: &PathBuf,
) -> ResultType<()> {
    let Some(current_exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    else {
        bail!("Failed to get current exe directory");
    };

    // Clean up existing staging directory
    if custom_client_staging_dir.exists() {
        log::debug!(
            "Removing existing custom client staging directory: {:?}",
            custom_client_staging_dir
        );
        if let Err(e) = remove_custom_client_staging_dir(custom_client_staging_dir) {
            bail!(
                "Failed to remove existing custom client staging directory {:?}: {}",
                custom_client_staging_dir,
                e
            );
        }
    }

    let src_path = current_exe_dir.join("custom.txt");
    if src_path.exists() {
        // Verify that custom.txt is not a symlink before copying
        let metadata = match std::fs::symlink_metadata(&src_path) {
            Ok(m) => m,
            Err(e) => {
                bail!(
                    "Failed to read metadata for custom.txt at {:?}: {}",
                    src_path,
                    e
                );
            }
        };

        if metadata.is_symlink() {
            allow_err!(remove_custom_client_staging_dir(&custom_client_staging_dir));
            bail!(
                "custom.txt at {:?} is a symlink, refusing to stage for security reasons.",
                src_path
            );
        }

        if metadata.is_file() {
            if !custom_client_staging_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(custom_client_staging_dir) {
                    bail!("Failed to create parent directory {:?} when staging custom client files: {}", custom_client_staging_dir, e);
                }
            }
            let dst_path = custom_client_staging_dir.join("custom.txt");
            if let Err(e) = std::fs::copy(&src_path, &dst_path) {
                allow_err!(remove_custom_client_staging_dir(&custom_client_staging_dir));
                bail!(
                    "Failed to copy custom txt from {:?} to {:?}: {}",
                    src_path,
                    dst_path,
                    e
                );
            }
        } else {
            log::warn!(
                "custom.txt at {:?} is not a regular file, skipping.",
                src_path
            );
        }
    } else {
        log::info!("No custom txt found to stage for update.");
    }

    Ok(())
}

// Used for auto update and manual update in the main window.
pub fn update_to(file: &str) -> ResultType<()> {
    if file.ends_with(".exe") {
        let custom_client_staging_dir = get_custom_client_staging_dir();
        if crate::is_custom_client() {
            handle_custom_client_staging_dir_before_update(&custom_client_staging_dir)?;
        } else {
            // Clean up any residual staging directory from previous custom client
            allow_err!(remove_custom_client_staging_dir(&custom_client_staging_dir));
        }
        if !run_uac(file, "--update")? {
            bail!(
                "Failed to run the update exe with UAC, error: {:?}",
                std::io::Error::last_os_error()
            );
        }
    } else if file.ends_with(".msi") {
        if let Err(e) = update_me_msi(file, false) {
            bail!("Failed to run the update msi: {}", e);
        }
    } else {
        // unreachable!()
        bail!("Unsupported update file format: {}", file);
    }
    Ok(())
}

// Don't launch tray app when running with `\qn`.
// 1. Because `/qn` requires administrator permission and the tray app should be launched with user permission.
//   Or launching the main window from the tray app will cause the main window to be launched with administrator permission.
// 2. We are not able to launch the tray app if the UI is in the login screen.
// `fn update_me()` can handle the above cases, but for msi update, we need to do more work to handle the above cases.
//    1. Record the tray app session ids.
//    2. Do the update.
//    3. Restore the tray app sessions.
//    `1` and `3` must be done in custom actions.
//    We need also to handle the command line parsing to find the tray processes.
pub fn update_me_msi(msi: &str, quiet: bool) -> ResultType<()> {
    let cmds = format!(
        "chcp 65001 && msiexec /i {msi} {}",
        if quiet { "/qn LAUNCH_TRAY_APP=N" } else { "" }
    );
    run_cmds(cmds, false, "update-msi")?;
    Ok(())
}

pub fn get_tray_shortcut(
    install_dir: &str,
    exe: &str,
    icon_source_exe: &str,
    tmp_path: &str,
) -> ResultType<String> {
    let shortcut_icon_location = get_shortcut_icon_location(install_dir, icon_source_exe);
    Ok(write_cmds(
        format!(
            "
Set oWS = WScript.CreateObject(\"WScript.Shell\")
sLinkFile = \"{tmp_path}\\{app_name} Tray.lnk\"

Set oLink = oWS.CreateShortcut(sLinkFile)
    oLink.TargetPath = \"{exe}\"
    oLink.Arguments = \"--tray\"
    {shortcut_icon_location}
oLink.Save
        ",
            app_name = crate::get_app_name(),
        ),
        "vbs",
        "tray_shortcut",
    )?
    .to_str()
    .unwrap_or("")
    .to_owned())
}

fn get_import_config(exe: &str) -> String {
    if config::is_outgoing_only() {
        return "".to_string();
    }
    format!("
sc stop {app_name}
sc delete {app_name}
sc create {app_name} binpath= \"\\\"{exe}\\\" --import-config \\\"{config_path}\\\"\" start= auto DisplayName= \"{app_name} Service\"
sc start {app_name}
sc stop {app_name}
sc delete {app_name}
",
    app_name = crate::get_app_name(),
    config_path=Config::file().to_str().unwrap_or(""),
)
}

fn get_create_service(exe: &str) -> String {
    if config::is_outgoing_only() {
        return "".to_string();
    }
    let stop = Config::get_option("stop-service") == "Y";
    if stop {
        format!("
if exist \"%PROGRAMDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\{app_name} Tray.lnk\" del /f /q \"%PROGRAMDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\{app_name} Tray.lnk\"
", app_name = crate::get_app_name())
    } else {
        format!("
sc create {app_name} binpath= \"\\\"{exe}\\\" --service\" start= auto DisplayName= \"{app_name} Service\"
sc start {app_name}
",
    app_name = crate::get_app_name())
    }
}

fn run_after_run_cmds(silent: bool) {
    let (_, _, _, exe) = get_install_info();
    if !silent {
        log::debug!("Spawn new window");
        allow_err!(std::process::Command::new("cmd")
            .args(&["/c", "timeout", "/t", "2", "&", &format!("{exe}")])
            .creation_flags(winapi::um::winbase::CREATE_NO_WINDOW)
            .spawn());
    }
    if Config::get_option("stop-service") != "Y" {
        allow_err!(std::process::Command::new(&exe).arg("--tray").spawn());
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
}

#[inline]
pub fn try_remove_temp_update_files() {
    let temp_dir = std::env::temp_dir();
    let Ok(entries) = std::fs::read_dir(&temp_dir) else {
        log::debug!("Failed to read temp directory: {:?}", temp_dir);
        return;
    };

    let one_hour = std::time::Duration::from_secs(60 * 60);
    let official_prefix = format!("{}-", crate::common::OFFICIAL_RELEASE_ASSET_PREFIX);
    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                // Match current HDesk update packages and clean legacy rustdesk-* leftovers too.
                if (file_name.starts_with(&official_prefix) || file_name.starts_with("rustdesk-"))
                    && (file_name.ends_with(".msi") || file_name.ends_with(".exe"))
                {
                    // Skip files modified within the last hour to avoid deleting files being downloaded
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        if let Ok(modified) = metadata.modified() {
                            if let Ok(elapsed) = modified.elapsed() {
                                if elapsed < one_hour {
                                    continue;
                                }
                            }
                        }
                    }
                    if let Err(e) = std::fs::remove_file(&path) {
                        log::debug!("Failed to remove temp update file {:?}: {}", path, e);
                    } else {
                        log::info!("Removed temp update file: {:?}", path);
                    }
                }
            }
        }
    }
}

#[inline]
pub fn try_kill_broker() {
    allow_err!(std::process::Command::new("cmd")
        .arg("/c")
        .arg(&format!(
            "taskkill /F /IM {}",
            WIN_TOPMOST_INJECTED_PROCESS_EXE
        ))
        .creation_flags(winapi::um::winbase::CREATE_NO_WINDOW)
        .spawn());
}

pub fn message_box(text: &str) {
    let mut text = text.to_owned();
    let nodialog = std::env::var("NO_DIALOG").unwrap_or_default() == "Y";
    if !text.ends_with("!") || nodialog {
        use arboard::Clipboard as ClipboardContext;
        match ClipboardContext::new() {
            Ok(mut ctx) => {
                ctx.set_text(&text).ok();
                if !nodialog {
                    text = format!("{}\n\nAbove text has been copied to clipboard", &text);
                }
            }
            _ => {}
        }
    }
    if nodialog {
        if std::env::var("PRINT_OUT").unwrap_or_default() == "Y" {
            println!("{text}");
        }
        if let Ok(x) = std::env::var("WRITE_TO_FILE") {
            if !x.is_empty() {
                allow_err!(std::fs::write(x, text));
            }
        }
        return;
    }
    let text = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    let caption = "HDesk Output"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK) };
}

pub fn alloc_console() {
    unsafe {
        alloc_console_and_redirect();
    }
}

fn get_license() -> Option<CustomServer> {
    let mut lic: CustomServer = Default::default();
    if let Ok(tmp) = get_license_from_exe_name() {
        lic = tmp;
    } else {
        // for back compatibility from migrating from <= 1.2.1 to 1.2.2
        lic.key = get_reg("Key");
        lic.host = get_reg("Host");
        lic.api = get_reg("Api");
    }
    if lic.key.is_empty() || lic.host.is_empty() {
        return None;
    }
    Some(lic)
}

pub struct WallPaperRemover {
    old_path: String,
}

impl WallPaperRemover {
    pub fn new() -> ResultType<Self> {
        let start = std::time::Instant::now();
        if !Self::need_remove() {
            bail!("already solid color");
        }
        let old_path = match Self::get_recent_wallpaper() {
            Ok(old_path) => old_path,
            Err(e) => {
                log::info!("Failed to get recent wallpaper: {:?}, use fallback", e);
                wallpaper::get().map_err(|e| anyhow!(e.to_string()))?
            }
        };
        Self::set_wallpaper(None)?;
        log::info!(
            "created wallpaper remover,  old_path: {:?},  elapsed: {:?}",
            old_path,
            start.elapsed(),
        );
        Ok(Self { old_path })
    }

    pub fn support() -> bool {
        wallpaper::get().is_ok() || !Self::get_recent_wallpaper().unwrap_or_default().is_empty()
    }

    fn get_recent_wallpaper() -> ResultType<String> {
        // SystemParametersInfoW may return %appdata%\Microsoft\Windows\Themes\TranscodedWallpaper, not real path and may not real cache
        // https://www.makeuseof.com/find-desktop-wallpapers-file-location-windows-11/
        // https://superuser.com/questions/1218413/write-to-current-users-registry-through-a-different-admin-account
        let (hkcu, sid) = if is_root() {
            let sid = get_current_process_session_id().ok_or(anyhow!("failed to get sid"))?;
            (RegKey::predef(HKEY_USERS), format!("{}\\", sid))
        } else {
            (RegKey::predef(HKEY_CURRENT_USER), "".to_string())
        };
        let explorer_key = hkcu.open_subkey_with_flags(
            &format!(
                "{}Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Wallpapers",
                sid
            ),
            KEY_READ,
        )?;
        Ok(explorer_key.get_value("BackgroundHistoryPath0")?)
    }

    fn need_remove() -> bool {
        if let Ok(wallpaper) = wallpaper::get() {
            return !wallpaper.is_empty();
        }
        false
    }

    fn set_wallpaper(path: Option<String>) -> ResultType<()> {
        wallpaper::set_from_path(&path.unwrap_or_default()).map_err(|e| anyhow!(e.to_string()))
    }
}

impl Drop for WallPaperRemover {
    fn drop(&mut self) {
        // If the old background is a slideshow, it will be converted into an image. AnyDesk does the same.
        allow_err!(Self::set_wallpaper(Some(self.old_path.clone())));
    }
}

fn get_uninstall_amyuni_idd() -> String {
    match std::env::current_exe() {
        Ok(path) => format!("\"{}\" --uninstall-amyuni-idd", path.to_str().unwrap_or("")),
        Err(e) => {
            log::warn!("Failed to get current exe path, cannot get command of uninstalling idd, Zzerror: {:?}", e);
            "".to_string()
        }
    }
}

#[inline]
pub fn is_self_service_running() -> bool {
    is_service_running(&crate::get_app_name())
}

pub fn is_service_running(service_name: &str) -> bool {
    unsafe {
        let service_name = wide_string(service_name);
        is_service_running_w(service_name.as_ptr() as _)
    }
}

pub fn is_x64() -> bool {
    const PROCESSOR_ARCHITECTURE_AMD64: u16 = 9;

    let mut sys_info = SYSTEM_INFO::default();
    unsafe {
        GetNativeSystemInfo(&mut sys_info as _);
    }
    unsafe { sys_info.u.s().wProcessorArchitecture == PROCESSOR_ARCHITECTURE_AMD64 }
}

pub fn try_kill_rustdesk_main_window_process() -> ResultType<()> {
    // Kill rustdesk.exe without extra arg, should only be called by --server
    // We can find the exact process which occupies the ipc, see more from https://github.com/winsiderss/systeminformer
    let app_name = crate::get_app_name().to_lowercase();
    log::info!("try kill main window process");
    use hbb_common::sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes();
    let my_uid = sys
        .process((std::process::id() as usize).into())
        .map(|x| x.user_id())
        .unwrap_or_default();
    let my_pid = std::process::id();
    if app_name.is_empty() {
        bail!("app name is empty");
    }
    for (_, p) in sys.processes().iter() {
        let p_name = p.name().to_lowercase();
        // name equal
        if !(p_name == app_name || p_name == app_name.clone() + ".exe") {
            continue;
        }
        // arg more than 1
        if p.cmd().len() < 1 {
            continue;
        }
        // first arg contain app name
        if !p.cmd()[0].to_lowercase().contains(&p_name) {
            continue;
        }
        // only one arg or the second arg is empty uni link
        let is_empty_uni = p.cmd().len() == 2 && crate::common::is_empty_uni_link(&p.cmd()[1]);
        if !(p.cmd().len() == 1 || is_empty_uni) {
            continue;
        }
        // skip self
        if p.pid().as_u32() == my_pid {
            continue;
        }
        // because we call it with --server, so we can check user_id, remove this if call it with user process
        if p.user_id() == my_uid {
            log::info!("user id equal, continue");
            continue;
        }
        log::info!("try kill process: {:?}, pid = {:?}", p.cmd(), p.pid());
        nt_terminate_process(p.pid().as_u32())?;
        log::info!("kill process success: {:?}, pid = {:?}", p.cmd(), p.pid());
        return Ok(());
    }
    bail!("failed to find rustdesk main window process");
}

fn nt_terminate_process(process_id: DWORD) -> ResultType<()> {
    type NtTerminateProcess = unsafe extern "system" fn(HANDLE, DWORD) -> DWORD;
    unsafe {
        let h_module = if is_win_10_or_greater() {
            LoadLibraryExA(
                CString::new("ntdll.dll")?.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        } else {
            LoadLibraryA(CString::new("ntdll.dll")?.as_ptr())
        };
        if !h_module.is_null() {
            let f_nt_terminate_process: NtTerminateProcess = std::mem::transmute(GetProcAddress(
                h_module,
                CString::new("NtTerminateProcess")?.as_ptr(),
            ));
            let h_token = OpenProcess(PROCESS_ALL_ACCESS, 0, process_id);
            if !h_token.is_null() {
                if f_nt_terminate_process(h_token, 1) == 0 {
                    log::info!("terminate process {} success", process_id);
                    CloseHandle(h_token);
                    return Ok(());
                } else {
                    CloseHandle(h_token);
                    bail!("NtTerminateProcess {} failed", process_id);
                }
            } else {
                bail!("OpenProcess {} failed", process_id);
            }
        } else {
            bail!("Failed to load ntdll.dll");
        }
    }
}

pub fn try_set_window_foreground(window: HWND) {
    let env_key = SET_FOREGROUND_WINDOW;
    if let Ok(value) = std::env::var(env_key) {
        if value == "1" {
            unsafe {
                SetForegroundWindow(window);
            }
            std::env::remove_var(env_key);
        }
    }
}

pub mod reg_display_settings {
    use hbb_common::ResultType;
    use serde_derive::{Deserialize, Serialize};
    use std::collections::HashMap;
    use winreg::{enums::*, RegValue};
    const REG_GRAPHICS_DRIVERS_PATH: &str = "SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers";
    const REG_CONNECTIVITY_PATH: &str = "Connectivity";

    #[derive(Serialize, Deserialize, Debug)]
    pub struct RegRecovery {
        path: String,
        key: String,
        old: (Vec<u8>, isize),
        new: (Vec<u8>, isize),
    }

    pub fn read_reg_connectivity() -> ResultType<HashMap<String, HashMap<String, RegValue>>> {
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        let reg_connectivity = hklm.open_subkey_with_flags(
            format!("{}\\{}", REG_GRAPHICS_DRIVERS_PATH, REG_CONNECTIVITY_PATH),
            KEY_READ,
        )?;

        let mut map_connectivity = HashMap::new();
        for key in reg_connectivity.enum_keys() {
            let key = key?;
            let mut map_item = HashMap::new();
            let reg_item = reg_connectivity.open_subkey_with_flags(&key, KEY_READ)?;
            for value in reg_item.enum_values() {
                let (name, value) = value?;
                map_item.insert(name, value);
            }
            map_connectivity.insert(key, map_item);
        }
        Ok(map_connectivity)
    }

    pub fn diff_recent_connectivity(
        map1: HashMap<String, HashMap<String, RegValue>>,
        map2: HashMap<String, HashMap<String, RegValue>>,
    ) -> Option<RegRecovery> {
        for (subkey, map_item2) in map2 {
            if let Some(map_item1) = map1.get(&subkey) {
                let key = "Recent";
                if let Some(value1) = map_item1.get(key) {
                    if let Some(value2) = map_item2.get(key) {
                        if value1 != value2 {
                            return Some(RegRecovery {
                                path: format!(
                                    "{}\\{}\\{}",
                                    REG_GRAPHICS_DRIVERS_PATH, REG_CONNECTIVITY_PATH, subkey
                                ),
                                key: key.to_owned(),
                                old: (value1.bytes.clone(), value1.vtype.clone() as isize),
                                new: (value2.bytes.clone(), value2.vtype.clone() as isize),
                            });
                        }
                    }
                }
            }
        }
        None
    }

    pub fn restore_reg_connectivity(reg_recovery: RegRecovery, force: bool) -> ResultType<()> {
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        let reg_item = hklm.open_subkey_with_flags(&reg_recovery.path, KEY_READ | KEY_WRITE)?;
        if !force {
            let cur_reg_value = reg_item.get_raw_value(&reg_recovery.key)?;
            let new_reg_value = RegValue {
                bytes: reg_recovery.new.0,
                vtype: isize_to_reg_type(reg_recovery.new.1),
            };
            // Compare if the current value is the same as the new value.
            // If they are not the same, the registry value has been changed by other processes.
            // So we do not restore the registry value.
            if cur_reg_value != new_reg_value {
                return Ok(());
            }
        }
        let reg_value = RegValue {
            bytes: reg_recovery.old.0,
            vtype: isize_to_reg_type(reg_recovery.old.1),
        };
        reg_item.set_raw_value(&reg_recovery.key, &reg_value)?;
        Ok(())
    }

    #[inline]
    fn isize_to_reg_type(i: isize) -> RegType {
        match i {
            0 => RegType::REG_NONE,
            1 => RegType::REG_SZ,
            2 => RegType::REG_EXPAND_SZ,
            3 => RegType::REG_BINARY,
            4 => RegType::REG_DWORD,
            5 => RegType::REG_DWORD_BIG_ENDIAN,
            6 => RegType::REG_LINK,
            7 => RegType::REG_MULTI_SZ,
            8 => RegType::REG_RESOURCE_LIST,
            9 => RegType::REG_FULL_RESOURCE_DESCRIPTOR,
            10 => RegType::REG_RESOURCE_REQUIREMENTS_LIST,
            11 => RegType::REG_QWORD,
            _ => RegType::REG_NONE,
        }
    }
}

pub fn get_printer_names() -> ResultType<Vec<String>> {
    let mut needed_bytes = 0;
    let mut returned_count = 0;

    unsafe {
        // First call to get required buffer size
        EnumPrintersW(
            PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
            std::ptr::null_mut(),
            1,
            std::ptr::null_mut(),
            0,
            &mut needed_bytes,
            &mut returned_count,
        );

        let mut buffer = vec![0u8; needed_bytes as usize];

        if EnumPrintersW(
            PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
            std::ptr::null_mut(),
            1,
            buffer.as_mut_ptr() as *mut _,
            needed_bytes,
            &mut needed_bytes,
            &mut returned_count,
        ) == 0
        {
            return Err(anyhow!("Failed to enumerate printers"));
        }

        let ptr = buffer.as_ptr() as *const PRINTER_INFO_1W;
        let printers = std::slice::from_raw_parts(ptr, returned_count as usize);

        Ok(printers
            .iter()
            .filter_map(|p| {
                let name = p.pName;
                if !name.is_null() {
                    let mut len = 0;
                    while len < 500 {
                        if name.add(len).is_null() || *name.add(len) == 0 {
                            break;
                        }
                        len += 1;
                    }
                    if len > 0 && len < 500 {
                        Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                            name, len,
                        )))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect())
    }
}

extern "C" {
    fn PrintXPSRawData(printer_name: *const u16, raw_data: *const u8, data_size: c_ulong) -> DWORD;
}

pub fn send_raw_data_to_printer(printer_name: Option<String>, data: Vec<u8>) -> ResultType<()> {
    let mut printer_name = printer_name.unwrap_or_default();
    if printer_name.is_empty() {
        // use GetDefaultPrinter to get the default printer name
        let mut needed_bytes = 0;
        unsafe {
            GetDefaultPrinterW(std::ptr::null_mut(), &mut needed_bytes);
        }
        if needed_bytes > 0 {
            let mut default_printer_name = vec![0u16; needed_bytes as usize];
            unsafe {
                GetDefaultPrinterW(
                    default_printer_name.as_mut_ptr() as *mut _,
                    &mut needed_bytes,
                );
            }
            printer_name = String::from_utf16_lossy(&default_printer_name[..needed_bytes as usize]);
        }
    } else {
        if let Ok(names) = crate::platform::windows::get_printer_names() {
            if !names.contains(&printer_name) {
                // Don't set the first printer as current printer.
                // It may not be the desired printer.
                bail!("Printer name \"{}\" not found", &printer_name);
            }
        }
    }
    if printer_name.is_empty() {
        return Err(anyhow!("Failed to get printer name"));
    }

    log::info!("Sending data to printer: {}", &printer_name);
    let printer_name = wide_string(&printer_name);
    unsafe {
        let res = PrintXPSRawData(
            printer_name.as_ptr(),
            data.as_ptr() as *const u8,
            data.len() as c_ulong,
        );
        if res != 0 {
            bail!("Failed to send data to the printer, see logs in C:\\Windows\\temp\\test_rustdesk.log for more details.");
        } else {
            log::info!("Successfully sent data to the printer");
        }
    }

    Ok(())
}

fn get_pids<S: AsRef<str>>(name: S) -> ResultType<Vec<u32>> {
    let name = name.as_ref().to_lowercase();
    let mut pids = Vec::new();

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        if snapshot == WinHANDLE::default() {
            return Ok(pids);
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let proc_name = OsString::from_wide(&entry.szExeFile)
                    .to_string_lossy()
                    .to_lowercase();

                if proc_name.contains(&name) {
                    pids.push(entry.th32ProcessID);
                }

                if !Process32NextW(snapshot, &mut entry).is_ok() {
                    break;
                }
            }
        }

        let _ = WinCloseHandle(snapshot);
    }

    Ok(pids)
}

pub fn is_msi_installed() -> std::io::Result<bool> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let uninstall_key = hklm.open_subkey(format!(
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
        crate::get_app_name()
    ))?;
    Ok(1 == uninstall_key.get_value::<u32, _>("WindowsInstaller")?)
}

pub fn is_cur_exe_the_installed() -> bool {
    let (_, _, _, exe) = get_install_info();
    // Check if is installed, because `exe` is the default path if is not installed.
    if !std::fs::metadata(&exe).is_ok() {
        return false;
    }
    let mut path = std::env::current_exe().unwrap_or_default();
    if let Ok(linked) = path.read_link() {
        path = linked;
    }
    let path = path.to_string_lossy().to_lowercase();
    path == exe.to_lowercase()
}

#[cfg(not(target_pointer_width = "64"))]
pub fn get_pids_with_first_arg_check_session<S1: AsRef<str>, S2: AsRef<str>>(
    name: S1,
    arg: S2,
    same_session_id: bool,
) -> ResultType<Vec<hbb_common::sysinfo::Pid>> {
    // Though `wmic` can return the sessionId, for simplicity we only return processid.
    let pids = get_pids_with_first_arg_by_wmic(name, arg);
    if !same_session_id {
        return Ok(pids);
    }
    let Some(cur_sid) = get_current_process_session_id() else {
        bail!("Can't get current process session id");
    };
    let mut same_session_pids = vec![];
    for pid in pids.into_iter() {
        let mut sid = 0;
        if unsafe { ProcessIdToSessionId(pid.as_u32(), &mut sid) == TRUE } {
            if sid == cur_sid {
                same_session_pids.push(pid);
            }
        } else {
            // Only log here, because this call almost never fails.
            log::warn!(
                "Failed to get session id of the process id, error: {:?}",
                std::io::Error::last_os_error()
            );
        }
    }
    Ok(same_session_pids)
}

#[cfg(not(target_pointer_width = "64"))]
fn get_pids_with_args_from_wmic_output<S2: AsRef<str>>(
    output: std::borrow::Cow<'_, str>,
    name: &str,
    args: &[S2],
) -> Vec<hbb_common::sysinfo::Pid> {
    // CommandLine=
    // ProcessId=33796
    //
    // CommandLine=
    // ProcessId=34668
    //
    // CommandLine="C:\Program Files\RustDesk\RustDesk.exe" --tray
    // ProcessId=13728
    //
    // CommandLine="C:\Program Files\RustDesk\RustDesk.exe"
    // ProcessId=10136
    let mut pids = Vec::new();
    let mut proc_found = false;
    for line in output.lines() {
        if line.starts_with("ProcessId=") {
            if proc_found {
                if let Ok(pid) = line["ProcessId=".len()..].trim().parse::<u32>() {
                    pids.push(hbb_common::sysinfo::Pid::from_u32(pid));
                }
                proc_found = false;
            }
        } else if line.starts_with("CommandLine=") {
            proc_found = false;
            let cmd = line["CommandLine=".len()..].trim().to_lowercase();
            if args.is_empty() {
                if cmd.ends_with(&name) || cmd.ends_with(&format!("{}\"", &name)) {
                    proc_found = true;
                }
            } else {
                proc_found = args.iter().all(|arg| cmd.contains(arg.as_ref()));
            }
        }
    }
    pids
}

// Note the args are not compared strictly, only check if the args are contained in the command line.
// If we want to check the args strictly, we need to parse the command line and compare each arg.
// Maybe we have to introduce some external crate like `shell_words` to do this.
#[cfg(not(target_pointer_width = "64"))]
pub(super) fn get_pids_with_args_by_wmic<S1: AsRef<str>, S2: AsRef<str>>(
    name: S1,
    args: &[S2],
) -> Vec<hbb_common::sysinfo::Pid> {
    let name = name.as_ref().to_lowercase();
    std::process::Command::new("wmic.exe")
        .args([
            "process",
            "where",
            &format!("name='{}'", name),
            "get",
            "commandline,processid",
            "/value",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|output| {
            get_pids_with_args_from_wmic_output::<S2>(
                String::from_utf8_lossy(&output.stdout),
                &name,
                args,
            )
        })
        .unwrap_or_default()
}

#[cfg(not(target_pointer_width = "64"))]
fn get_pids_with_first_arg_from_wmic_output(
    output: std::borrow::Cow<'_, str>,
    name: &str,
    arg: &str,
) -> Vec<hbb_common::sysinfo::Pid> {
    let mut pids = Vec::new();
    let mut proc_found = false;
    for line in output.lines() {
        if line.starts_with("ProcessId=") {
            if proc_found {
                if let Ok(pid) = line["ProcessId=".len()..].trim().parse::<u32>() {
                    pids.push(hbb_common::sysinfo::Pid::from_u32(pid));
                }
                proc_found = false;
            }
        } else if line.starts_with("CommandLine=") {
            proc_found = false;
            let cmd = line["CommandLine=".len()..].trim().to_lowercase();
            if cmd.is_empty() {
                continue;
            }
            if !arg.is_empty() && cmd.starts_with(arg) {
                proc_found = true;
            } else {
                for x in [&format!("{}\"", name), &format!("{}", name)] {
                    if cmd.contains(x) {
                        let cmd = cmd.split(x).collect::<Vec<_>>()[1..].join("");
                        if arg.is_empty() {
                            if cmd.trim().is_empty() {
                                proc_found = true;
                            }
                        } else if cmd.trim().starts_with(arg) {
                            proc_found = true;
                        }
                        break;
                    }
                }
            }
        }
    }
    pids
}

// Note the args are not compared strictly, only check if the args are contained in the command line.
// If we want to check the args strictly, we need to parse the command line and compare each arg.
// Maybe we have to introduce some external crate like `shell_words` to do this.
#[cfg(not(target_pointer_width = "64"))]
pub(super) fn get_pids_with_first_arg_by_wmic<S1: AsRef<str>, S2: AsRef<str>>(
    name: S1,
    arg: S2,
) -> Vec<hbb_common::sysinfo::Pid> {
    let name = name.as_ref().to_lowercase();
    let arg = arg.as_ref().to_lowercase();
    std::process::Command::new("wmic.exe")
        .args([
            "process",
            "where",
            &format!("name='{}'", name),
            "get",
            "commandline,processid",
            "/value",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|output| {
            get_pids_with_first_arg_from_wmic_output(
                String::from_utf8_lossy(&output.stdout),
                &name,
                &arg,
            )
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_uninstall_cert() {
        println!("uninstall driver certs: {:?}", cert::uninstall_cert());
    }

    #[test]
    fn test_get_unicode_char_by_vk() {
        let chr = get_char_from_vk(0x41); // VK_A
        assert_eq!(chr, Some('a'));
        let chr = get_char_from_vk(VK_ESCAPE as u32); // VK_ESC
        assert_eq!(chr, None)
    }

    #[cfg(not(target_pointer_width = "64"))]
    #[test]
    fn test_get_pids_with_args_from_wmic_output() {
        let output = r#"
CommandLine=
ProcessId=33796

CommandLine=
ProcessId=34668

CommandLine="C:\Program Files\testapp\TestApp.exe" --tray
ProcessId=13728

CommandLine="C:\Program Files\testapp\TestApp.exe"
ProcessId=10136
"#;
        let name = "testapp.exe";
        let args = vec!["--tray"];
        let pids = super::get_pids_with_args_from_wmic_output(
            String::from_utf8_lossy(output.as_bytes()),
            name,
            &args,
        );
        assert_eq!(pids.len(), 1);
        assert_eq!(pids[0].as_u32(), 13728);

        let args: Vec<&str> = vec![];
        let pids = super::get_pids_with_args_from_wmic_output(
            String::from_utf8_lossy(output.as_bytes()),
            name,
            &args,
        );
        assert_eq!(pids.len(), 1);
        assert_eq!(pids[0].as_u32(), 10136);

        let args = vec!["--other"];
        let pids = super::get_pids_with_args_from_wmic_output(
            String::from_utf8_lossy(output.as_bytes()),
            name,
            &args,
        );
        assert_eq!(pids.len(), 0);
    }

    #[cfg(not(target_pointer_width = "64"))]
    #[test]
    fn test_get_pids_with_first_arg_from_wmic_output() {
        let output = r#"
CommandLine=
ProcessId=33796

CommandLine=
ProcessId=34668

CommandLine="C:\Program Files\testapp\TestApp.exe" --tray
ProcessId=13728

CommandLine="C:\Program Files\testapp\TestApp.exe"
ProcessId=10136
    "#;
        let name = "testapp.exe";
        let arg = "--tray";
        let pids = super::get_pids_with_first_arg_from_wmic_output(
            String::from_utf8_lossy(output.as_bytes()),
            name,
            arg,
        );
        assert_eq!(pids.len(), 1);
        assert_eq!(pids[0].as_u32(), 13728);

        let arg = "";
        let pids = super::get_pids_with_first_arg_from_wmic_output(
            String::from_utf8_lossy(output.as_bytes()),
            name,
            arg,
        );
        assert_eq!(pids.len(), 1);
        assert_eq!(pids[0].as_u32(), 10136);

        let arg = "--other";
        let pids = super::get_pids_with_first_arg_from_wmic_output(
            String::from_utf8_lossy(output.as_bytes()),
            name,
            arg,
        );
        assert_eq!(pids.len(), 0);
    }
}
