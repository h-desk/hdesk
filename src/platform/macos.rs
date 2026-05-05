// https://developer.apple.com/documentation/appkit/nscursor
// https://github.com/servo/core-foundation-rs
// https://github.com/rust-windowing/winit

use super::{CursorData, ResultType};
use cocoa::{
    appkit::{NSApp, NSApplication, NSApplicationActivationPolicy::*},
    base::{id, nil, BOOL, NO, YES},
    foundation::{NSDictionary, NSPoint, NSSize, NSString},
};
use core_foundation::{
    array::{CFArrayGetCount, CFArrayGetValueAtIndex},
    base::{CFTypeRef, TCFType},
    dictionary::CFDictionaryRef,
    string::{CFString, CFStringRef},
};
use core_graphics::{
    display::{kCGNullWindowID, kCGWindowListOptionOnScreenOnly, CGWindowListCopyWindowInfo},
    window::{kCGWindowName, kCGWindowOwnerPID},
};
use hbb_common::{
    anyhow::anyhow,
    bail, log,
    message_proto::{DisplayInfo, Resolution},
    sysinfo::{Pid, Process, ProcessRefreshKind, System},
};
use include_dir::{include_dir, Dir};
use objc::rc::autoreleasepool;
use objc::{class, msg_send, sel, sel_impl};
use scrap::{libc::c_void, quartz::ffi::*};
use std::{
    collections::HashMap,
    io::ErrorKind,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr,
    sync::Mutex,
    time::{Duration, Instant},
};

// macOS boolean_t is defined as `int` in <mach/boolean.h>
type BooleanT = hbb_common::libc::c_int;
type AXUIElementRef = *const c_void;
type AXValueRef = *const c_void;
type AXError = i32;

const AX_ERROR_SUCCESS: AXError = 0;
const AX_VALUE_CGPOINT_TYPE: u32 = 1;
const AX_VALUE_CGSIZE_TYPE: u32 = 2;

static PRIVILEGES_SCRIPTS_DIR: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/src/platform/privileges_scripts");
static mut LATEST_SEED: i32 = 0;

/// Editable focus hint data returned by `get_editable_focus_hint`.
/// All rects are in desktop global coordinates (pixels, origin = virtual-desktop top-left).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct EditableFocusHintInfo {
    pub editable: bool,
    pub caret: (i32, i32, i32, i32),
    pub editor: (i32, i32, i32, i32),
    pub window: (i32, i32, i32, i32),
    pub pane: (i32, i32, i32, i32),
    pub display_idx: i32,
    pub content_kind: i32,
    /// Local-only stable window identity for change detection. Not sent over wire.
    pub foreground_hwnd: isize,
}

#[inline]
fn get_update_temp_dir() -> PathBuf {
    let euid = unsafe { hbb_common::libc::geteuid() };
    Path::new("/tmp").join(format!(".rustdeskupdate-{}", euid))
}

#[inline]
fn get_update_temp_dir_string() -> String {
    get_update_temp_dir().to_string_lossy().into_owned()
}

/// Global mutex to serialize CoreGraphics cursor operations.
/// This prevents race conditions between cursor visibility (hide depth tracking)
/// and cursor positioning/clipping operations.
static CG_CURSOR_MUTEX: Mutex<()> = Mutex::new(());

extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXValueGetType(value: AXValueRef) -> u32;
    fn AXValueGetValue(value: AXValueRef, ax_type: u32, value_ptr: *mut c_void) -> u8;
    fn CFBooleanGetValue(boolean: *const c_void) -> u8;
    fn CGSCurrentCursorSeed() -> i32;
    fn CGEventCreate(r: *const c_void) -> *const c_void;
    fn CGEventGetLocation(e: *const c_void) -> CGPoint;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> BOOL;
    fn InputMonitoringAuthStatus(_: BOOL) -> BOOL;
    fn IsCanScreenRecording(_: BOOL) -> BOOL;
    fn CanUseNewApiForScreenCaptureCheck() -> BOOL;
    fn MacCheckAdminAuthorization() -> BOOL;
    fn MacGetModeNum(display: u32, numModes: *mut u32) -> BOOL;
    fn MacGetModes(
        display: u32,
        widths: *mut u32,
        heights: *mut u32,
        hidpis: *mut BOOL,
        max: u32,
        numModes: *mut u32,
    ) -> BOOL;
    fn majorVersion() -> u32;
    fn MacGetMode(display: u32, width: *mut u32, height: *mut u32) -> BOOL;
    fn MacSetMode(display: u32, width: u32, height: u32, tryHiDPI: bool) -> BOOL;
    fn CGWarpMouseCursorPosition(newCursorPosition: CGPoint) -> CGError;
    fn CGAssociateMouseAndMouseCursorPosition(connected: BooleanT) -> CGError;
}

pub fn major_version() -> u32 {
    unsafe { majorVersion() }
}

pub fn is_process_trusted(prompt: bool) -> bool {
    autoreleasepool(|| unsafe_is_process_trusted(prompt))
}

fn unsafe_is_process_trusted(prompt: bool) -> bool {
    unsafe {
        let value = if prompt { YES } else { NO };
        let value: id = msg_send![class!(NSNumber), numberWithBool: value];
        let options = NSDictionary::dictionaryWithObject_forKey_(
            nil,
            value,
            kAXTrustedCheckOptionPrompt as _,
        );
        AXIsProcessTrustedWithOptions(options as _) == YES
    }
}

pub fn is_can_input_monitoring(prompt: bool) -> bool {
    unsafe {
        let value = if prompt { YES } else { NO };
        InputMonitoringAuthStatus(value) == YES
    }
}

pub fn is_can_screen_recording(prompt: bool) -> bool {
    autoreleasepool(|| unsafe_is_can_screen_recording(prompt))
}

pub fn get_editable_focus_hint(displays: &[DisplayInfo]) -> Option<EditableFocusHintInfo> {
    autoreleasepool(|| unsafe_get_editable_focus_hint(displays))
}

pub fn get_window_rect_for_point(x: i32, y: i32) -> Option<(i32, i32, i32, i32)> {
    autoreleasepool(|| unsafe_get_window_rect_for_point(x, y))
}

fn rect_has_area(rect: (i32, i32, i32, i32)) -> bool {
    rect.2 > 0 && rect.3 > 0
}

fn rect_area(rect: (i32, i32, i32, i32)) -> i64 {
    (rect.2.max(0) as i64) * (rect.3.max(0) as i64)
}

fn rect_intersection(
    left: (i32, i32, i32, i32),
    right: (i32, i32, i32, i32),
) -> Option<(i32, i32, i32, i32)> {
    let x1 = left.0.max(right.0);
    let y1 = left.1.max(right.1);
    let x2 = (left.0 + left.2).min(right.0 + right.2);
    let y2 = (left.1 + left.3).min(right.1 + right.3);
    let rect = (x1, y1, (x2 - x1).max(0), (y2 - y1).max(0));
    rect_has_area(rect).then_some(rect)
}

fn rect_contains_rect_with_margin(
    outer: (i32, i32, i32, i32),
    inner: (i32, i32, i32, i32),
    margin_x: i32,
    margin_y: i32,
) -> bool {
    rect_has_area(outer)
        && rect_has_area(inner)
        && inner.0 >= outer.0 - margin_x
        && inner.1 >= outer.1 - margin_y
        && inner.0 + inner.2 <= outer.0 + outer.2 + margin_x
        && inner.1 + inner.3 <= outer.1 + outer.3 + margin_y
}

fn pane_candidate_is_meaningful(
    candidate: (i32, i32, i32, i32),
    editor: (i32, i32, i32, i32),
    window: (i32, i32, i32, i32),
) -> bool {
    if !rect_has_area(candidate) || !rect_has_area(editor) || !rect_has_area(window) {
        return false;
    }
    if !rect_contains_rect_with_margin(candidate, editor, 64, 96) {
        return false;
    }

    let candidate_area = rect_area(candidate);
    let editor_area = rect_area(editor);
    let window_area = rect_area(window).max(1);
    if candidate_area <= editor_area + 4_000 {
        return false;
    }

    let width_diff = (window.2 - candidate.2).abs();
    let height_diff = (window.3 - candidate.3).abs();
    let origin_diff = (window.0 - candidate.0).abs().max((window.1 - candidate.1).abs());
    let meaningfully_smaller_than_window = width_diff >= 32 || height_diff >= 32 || origin_diff >= 24;
    let window_area_ratio = candidate_area as f64 / window_area as f64;

    meaningfully_smaller_than_window && window_area_ratio <= 0.9
}

fn point_in_rect(rect: (i32, i32, i32, i32), x: i32, y: i32) -> bool {
    rect_has_area(rect)
        && x >= rect.0
        && x <= rect.0 + rect.2
        && y >= rect.1
        && y <= rect.1 + rect.3
}

fn rect_center(rect: (i32, i32, i32, i32)) -> Option<(i32, i32)> {
    if !rect_has_area(rect) {
        return None;
    }
    Some((rect.0 + rect.2 / 2, rect.1 + rect.3 / 2))
}

fn point_in_display(display: &DisplayInfo, x: i32, y: i32) -> bool {
    let right = display.x + display.width;
    let bottom = display.y + display.height;
    x >= display.x && x <= right && y >= display.y && y <= bottom
}

fn find_display_idx_for_point(displays: &[DisplayInfo], x: i32, y: i32) -> Option<usize> {
    displays
        .iter()
        .position(|display| point_in_display(display, x, y))
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
        .or_else(|| rect_center(hint.pane).and_then(|(cx, cy)| find_display_idx_for_point(displays, cx, cy)))
        .or_else(|| rect_center(hint.window).and_then(|(cx, cy)| find_display_idx_for_point(displays, cx, cy)))
        .or_else(|| rect_center(hint.caret).and_then(|(cx, cy)| find_display_idx_for_point(displays, cx, cy)));

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

fn make_foreground_window_id(pid: i32, window_number: i32) -> isize {
    (((pid as i64) << 32) | (window_number as u32 as i64)) as isize
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct MacWindowCandidate {
    rect: (i32, i32, i32, i32),
    foreground_hwnd: isize,
}

fn inset_rect(rect: (i32, i32, i32, i32), inset_x: i32, inset_y: i32) -> Option<(i32, i32, i32, i32)> {
    if !rect_has_area(rect) {
        return None;
    }

    let inset_x = inset_x.min(rect.2.saturating_sub(1) / 2);
    let inset_y = inset_y.min(rect.3.saturating_sub(1) / 2);
    let inner = (
        rect.0 + inset_x,
        rect.1 + inset_y,
        rect.2 - inset_x * 2,
        rect.3 - inset_y * 2,
    );
    rect_has_area(inner).then_some(inner)
}

const AX_SYNTH_EDITOR_WIDTH: i32 = 240;
const AX_SYNTH_EDITOR_HEIGHT: i32 = 40;

fn synth_editor_rect_from_cursor(
    window: (i32, i32, i32, i32),
    cursor: (i32, i32),
) -> Option<(i32, i32, i32, i32)> {
    if !rect_has_area(window) || !point_in_rect(window, cursor.0, cursor.1) {
        return None;
    }

    let content = inset_rect(window, 8, 8)?;
    let target_width = AX_SYNTH_EDITOR_WIDTH.min(content.2);
    let target_height = AX_SYNTH_EDITOR_HEIGHT.min(content.3);
    let max_left = content.0 + content.2 - target_width;
    let max_top = content.1 + content.3 - target_height;

    Some((
        (cursor.0 - target_width / 2).clamp(content.0, max_left),
        (cursor.1 - target_height / 2).clamp(content.1, max_top),
        target_width,
        target_height,
    ))
}

fn choose_window_candidate(
    candidates: &[MacWindowCandidate],
    target: (i32, i32, i32, i32),
) -> Option<MacWindowCandidate> {
    if !rect_has_area(target) {
        return None;
    }

    let target_center = rect_center(target);
    candidates
        .iter()
        .copied()
        .filter_map(|candidate| {
            if !rect_has_area(candidate.rect) {
                return None;
            }
            let center_match = target_center
                .map(|(x, y)| point_in_rect(candidate.rect, x, y))
                .unwrap_or(false);
            let overlap = rect_intersection(candidate.rect, target)
                .map(rect_area)
                .unwrap_or(0);
            if !center_match && overlap == 0 {
                return None;
            }

            let origin_delta = (candidate.rect.0 - target.0).abs() as i64
                + (candidate.rect.1 - target.1).abs() as i64;
            let size_delta = (candidate.rect.2 - target.2).abs() as i64
                + (candidate.rect.3 - target.3).abs() as i64;
            Some((candidate, center_match, overlap, -origin_delta, -size_delta))
        })
        .max_by_key(|(_, center_match, overlap, origin_delta, size_delta)| {
            (*center_match, *overlap, *origin_delta, *size_delta)
        })
        .map(|(candidate, _, _, _, _)| candidate)
}

fn find_window_candidate_containing_point(
    candidates: &[MacWindowCandidate],
    x: i32,
    y: i32,
) -> Option<MacWindowCandidate> {
    candidates
        .iter()
        .copied()
        .find(|candidate| point_in_rect(candidate.rect, x, y))
}

fn pane_candidate_role_rank(role: &str) -> i32 {
    match role.to_ascii_lowercase().as_str() {
        "axdocument" | "axwebarea" | "axscrollarea" => 3,
        "axsplitgroup" | "axgroup" | "axlayoutarea" => 2,
        _ => 1,
    }
}

fn classify_ax_editable_role(
    role: &str,
    subrole: Option<&str>,
    editable_attr: Option<bool>,
) -> (bool, i32) {
    let role = role.to_ascii_lowercase();
    let subrole = subrole.unwrap_or_default().to_ascii_lowercase();
    let code_like = role.contains("code")
        || subrole.contains("code")
        || subrole.contains("editor")
        || subrole.contains("source");

    if editable_attr == Some(false) {
        return (false, 0);
    }

    if role == "axtextfield"
        || role == "axsearchfield"
        || role == "axcombobox"
        || subrole.contains("secure")
    {
        return (true, 1);
    }
    if role == "axtextarea" {
        return (true, if code_like { 3 } else { 2 });
    }
    if matches!(role.as_str(), "axdocument" | "axwebarea") && editable_attr == Some(true) {
        return (true, if code_like || role == "axdocument" { 3 } else { 2 });
    }
    if editable_attr == Some(true) {
        if code_like {
            return (true, 3);
        }
        return (true, 2);
    }
    (false, 0)
}

unsafe fn ax_copy_attribute_value(
    element: AXUIElementRef,
    attribute: &str,
) -> Option<CFTypeRef> {
    if element.is_null() {
        return None;
    }
    let attribute = CFString::new(attribute);
    let mut value: CFTypeRef = ptr::null();
    let err = AXUIElementCopyAttributeValue(
        element,
        attribute.as_concrete_TypeRef(),
        &mut value,
    );
    if err == AX_ERROR_SUCCESS && !value.is_null() {
        Some(value)
    } else {
        None
    }
}

unsafe fn ax_copy_attribute_element(
    element: AXUIElementRef,
    attribute: &str,
) -> Option<AXUIElementRef> {
    ax_copy_attribute_value(element, attribute).map(|value| value as AXUIElementRef)
}

unsafe fn ax_copy_attribute_string(
    element: AXUIElementRef,
    attribute: &str,
) -> Option<String> {
    let value = ax_copy_attribute_value(element, attribute)?;
    Some(CFString::wrap_under_create_rule(value as CFStringRef).to_string())
}

unsafe fn ax_copy_attribute_bool(
    element: AXUIElementRef,
    attribute: &str,
) -> Option<bool> {
    let value = ax_copy_attribute_value(element, attribute)?;
    let enabled = CFBooleanGetValue(value as *const c_void) != 0;
    CFRelease(value);
    Some(enabled)
}

unsafe fn ax_copy_attribute_point(
    element: AXUIElementRef,
    attribute: &str,
) -> Option<CGPoint> {
    let value = ax_copy_attribute_value(element, attribute)?;
    let mut point = CGPoint { x: 0.0, y: 0.0 };
    let ok = AXValueGetType(value as AXValueRef) == AX_VALUE_CGPOINT_TYPE
        && AXValueGetValue(
            value as AXValueRef,
            AX_VALUE_CGPOINT_TYPE,
            &mut point as *mut _ as *mut c_void,
        )
            != 0;
    CFRelease(value);
    ok.then_some(point)
}

unsafe fn ax_copy_attribute_size(
    element: AXUIElementRef,
    attribute: &str,
) -> Option<NSSize> {
    let value = ax_copy_attribute_value(element, attribute)?;
    let mut size = NSSize {
        width: 0.0,
        height: 0.0,
    };
    let ok = AXValueGetType(value as AXValueRef) == AX_VALUE_CGSIZE_TYPE
        && AXValueGetValue(
            value as AXValueRef,
            AX_VALUE_CGSIZE_TYPE,
            &mut size as *mut _ as *mut c_void,
        )
            != 0;
    CFRelease(value);
    ok.then_some(size)
}

unsafe fn ax_element_rect(element: AXUIElementRef) -> Option<(i32, i32, i32, i32)> {
    let position = ax_copy_attribute_point(element, "AXPosition")?;
    let size = ax_copy_attribute_size(element, "AXSize")?;
    let rect = (
        position.x.round() as i32,
        position.y.round() as i32,
        size.width.round() as i32,
        size.height.round() as i32,
    );
    rect_has_area(rect).then_some(rect)
}

unsafe fn ax_pick_pane_rect(
    element: AXUIElementRef,
    editor: (i32, i32, i32, i32),
    window: (i32, i32, i32, i32),
) -> Option<(i32, i32, i32, i32)> {
    let mut best: Option<((i32, i32, i32, i32), i32)> = None;
    let mut current = ax_copy_attribute_element(element, "AXParent")?;

    for _ in 0..6 {
        if let Some(candidate) = ax_element_rect(current)
            .and_then(|rect| rect_intersection(rect, window))
            .filter(|rect| pane_candidate_is_meaningful(*rect, editor, window))
        {
            let role = ax_copy_attribute_string(current, "AXRole").unwrap_or_default();
            let rank = pane_candidate_role_rank(&role);
            if best
                .map(|(previous, previous_rank)| {
                    rank > previous_rank
                        || (rank == previous_rank && rect_area(candidate) < rect_area(previous))
                })
                .unwrap_or(true)
            {
                best = Some((candidate, rank));
            }
        }

        let next = ax_copy_attribute_element(current, "AXParent");
        CFRelease(current as *const c_void);
        let Some(next) = next else {
            return best.map(|(rect, _)| rect);
        };
        current = next;
    }

    CFRelease(current as *const c_void);
    best.map(|(rect, _)| rect)
}

unsafe fn resolve_ax_window_candidate(
    app: AXUIElementRef,
    focused_element: AXUIElementRef,
    cg_windows: &[MacWindowCandidate],
) -> Option<MacWindowCandidate> {
    for (owner, attribute) in [
        (focused_element, "AXWindow"),
        (focused_element, "AXTopLevelUIElement"),
        (app, "AXFocusedWindow"),
        (app, "AXMainWindow"),
    ] {
        let Some(window_element) = ax_copy_attribute_element(owner, attribute) else {
            continue;
        };
        let target_rect = ax_element_rect(window_element);
        CFRelease(window_element as *const c_void);
        if let Some(candidate) = target_rect.and_then(|rect| choose_window_candidate(cg_windows, rect)) {
            return Some(candidate);
        }
    }

    None
}

struct AxFocusBuildResult {
    window: MacWindowCandidate,
    editable_hint: Option<EditableFocusHintInfo>,
}

unsafe fn build_ax_editable_focus_hint(
    displays: &[DisplayInfo],
    frontmost_pid: i32,
    cg_windows: &[MacWindowCandidate],
    fallback_window: MacWindowCandidate,
) -> Option<AxFocusBuildResult> {
    if !is_process_trusted(false) {
        return None;
    }

    let app = AXUIElementCreateApplication(frontmost_pid);
    if app.is_null() {
        return None;
    }

    let focused_element = ax_copy_attribute_element(app, "AXFocusedUIElement");
    let Some(focused_element) = focused_element else {
        CFRelease(app as *const c_void);
        return None;
    };

    let resolved_window = resolve_ax_window_candidate(app, focused_element, cg_windows)
        .unwrap_or(fallback_window);
    let window = resolved_window.rect;

    let role = ax_copy_attribute_string(focused_element, "AXRole").unwrap_or_default();
    let subrole = ax_copy_attribute_string(focused_element, "AXSubrole");
    let editable_attr = ax_copy_attribute_bool(focused_element, "AXEditable");
    let (editable, content_kind) =
        classify_ax_editable_role(&role, subrole.as_deref(), editable_attr);

    if !editable {
        CFRelease(focused_element as *const c_void);
        CFRelease(app as *const c_void);
        return Some(AxFocusBuildResult {
            window: resolved_window,
            editable_hint: None,
        });
    }

    let editor = ax_element_rect(focused_element)
        .and_then(|rect| rect_intersection(rect, window))
        .or_else(|| {
            get_cursor_pos()
                .filter(|(x, y)| point_in_rect(window, *x, *y))
                .and_then(|cursor| synth_editor_rect_from_cursor(window, cursor))
        });
    let pane = editor.and_then(|editor_rect| ax_pick_pane_rect(focused_element, editor_rect, window));
    CFRelease(focused_element as *const c_void);
    CFRelease(app as *const c_void);

    let Some(editor) = editor else {
        return Some(AxFocusBuildResult {
            window: resolved_window,
            editable_hint: None,
        });
    };
    let pane = pane.unwrap_or(window);

    log::debug!(
        "editable_focus macOS AX hit: role={} subrole={:?} editable_attr={:?} kind={} editor={:?} pane={:?} window={:?}",
        role,
        subrole,
        editable_attr,
        content_kind,
        editor,
        pane,
        window,
    );

    Some(AxFocusBuildResult {
        window: resolved_window,
        editable_hint: Some(normalize_editable_focus_hint_to_display(
            displays,
            EditableFocusHintInfo {
                editable: true,
                caret: (0, 0, 0, 0),
                editor,
                window,
                pane,
                display_idx: 0,
                content_kind,
                foreground_hwnd: resolved_window.foreground_hwnd,
            },
        )),
    })
}

unsafe fn get_frontmost_pid() -> Option<i32> {
    let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
    if workspace == nil {
        return None;
    }
    let app: id = msg_send![workspace, frontmostApplication];
    if app == nil {
        return None;
    }
    let pid: i32 = msg_send![app, processIdentifier];
    if pid > 0 { Some(pid) } else { None }
}

unsafe fn number_value(dict: id, key: &str) -> Option<f64> {
    let key = CFString::new(key);
    let value: id = msg_send![dict, objectForKey: key.as_concrete_TypeRef() as id];
    if value == nil {
        return None;
    }
    let value: f64 = msg_send![value, doubleValue];
    Some(value)
}

unsafe fn read_window_bounds(bounds_dict: id) -> Option<(i32, i32, i32, i32)> {
    if bounds_dict == nil {
        return None;
    }
    let x = number_value(bounds_dict, "X")?;
    let y = number_value(bounds_dict, "Y")?;
    let width = number_value(bounds_dict, "Width")?;
    let height = number_value(bounds_dict, "Height")?;
    let rect = (
        x.round() as i32,
        y.round() as i32,
        width.round() as i32,
        height.round() as i32,
    );
    rect_has_area(rect).then_some(rect)
}

unsafe fn list_frontmost_windows(frontmost_pid: i32) -> Vec<MacWindowCandidate> {
    let window_list = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID);
    if window_list.is_null() {
        return Vec::new();
    }

    let result = {
        let layer_key = CFString::new("kCGWindowLayer");
        let bounds_key = CFString::new("kCGWindowBounds");
        let window_number_key = CFString::new("kCGWindowNumber");
        let count = CFArrayGetCount(window_list);
        let mut windows = Vec::new();

        for idx in 0..count {
            let window: id = CFArrayGetValueAtIndex(window_list, idx) as _;
            if window == nil {
                continue;
            }

            let pid_value: id = msg_send![window, valueForKey: kCGWindowOwnerPID as id];
            if pid_value == nil {
                continue;
            }
            let owner_pid: i32 = msg_send![pid_value, intValue];
            if owner_pid != frontmost_pid {
                continue;
            }

            let layer_value: id = msg_send![window, objectForKey: layer_key.as_concrete_TypeRef() as id];
            if layer_value != nil {
                let layer: i32 = msg_send![layer_value, intValue];
                if layer != 0 {
                    continue;
                }
            }

            let bounds_value: id = msg_send![window, objectForKey: bounds_key.as_concrete_TypeRef() as id];
            let Some(window_rect) = read_window_bounds(bounds_value) else {
                continue;
            };

            let window_number_value: id = msg_send![window, objectForKey: window_number_key.as_concrete_TypeRef() as id];
            let window_number: i32 = if window_number_value == nil {
                0
            } else {
                msg_send![window_number_value, intValue]
            };

            windows.push(MacWindowCandidate {
                rect: window_rect,
                foreground_hwnd: make_foreground_window_id(owner_pid, window_number),
            });
        }

        windows
    };

    CFRelease(window_list as *const c_void);
    result
}

fn unsafe_get_window_rect_for_point(x: i32, y: i32) -> Option<(i32, i32, i32, i32)> {
    let frontmost_pid = unsafe { get_frontmost_pid() }?;
    let cg_windows = unsafe { list_frontmost_windows(frontmost_pid) };
    find_window_candidate_containing_point(&cg_windows, x, y)
        .or_else(|| cg_windows.first().copied())
        .map(|window| window.rect)
}

fn fallback_window_hint(displays: &[DisplayInfo], frontmost_pid: i32) -> Option<EditableFocusHintInfo> {
    let display_idx = get_focused_display(displays.to_vec()).or_else(|| (!displays.is_empty()).then_some(0))?;
    let display = displays.get(display_idx)?;
    let window = (display.x, display.y, display.width, display.height);
    Some(normalize_editable_focus_hint_to_display(
        displays,
        EditableFocusHintInfo {
            editable: false,
            caret: (0, 0, 0, 0),
            editor: (0, 0, 0, 0),
            window,
            pane: window,
            display_idx: display_idx as i32,
            content_kind: 0,
            foreground_hwnd: make_foreground_window_id(frontmost_pid, 0),
        },
    ))
}

fn unsafe_get_editable_focus_hint(displays: &[DisplayInfo]) -> Option<EditableFocusHintInfo> {
    let frontmost_pid = unsafe { get_frontmost_pid() }?;
    let cg_windows = unsafe { list_frontmost_windows(frontmost_pid) };
    if let Some(fallback_window) = cg_windows.first().copied() {
        if let Some(ax_result) = unsafe {
            build_ax_editable_focus_hint(displays, frontmost_pid, &cg_windows, fallback_window)
        } {
            if let Some(editable_hint) = ax_result.editable_hint {
                return Some(editable_hint);
            }
            return Some(normalize_editable_focus_hint_to_display(
                displays,
                EditableFocusHintInfo {
                    editable: false,
                    caret: (0, 0, 0, 0),
                    editor: (0, 0, 0, 0),
                    window: ax_result.window.rect,
                    pane: ax_result.window.rect,
                    display_idx: 0,
                    content_kind: 0,
                    foreground_hwnd: ax_result.window.foreground_hwnd,
                },
            ));
        }
        return Some(normalize_editable_focus_hint_to_display(
            displays,
            EditableFocusHintInfo {
                editable: false,
                caret: (0, 0, 0, 0),
                editor: (0, 0, 0, 0),
                window: fallback_window.rect,
                pane: fallback_window.rect,
                display_idx: 0,
                content_kind: 0,
                foreground_hwnd: fallback_window.foreground_hwnd,
            },
        ));
    }

    fallback_window_hint(displays, frontmost_pid)
}

#[cfg(test)]
mod tests {
    use super::{
        choose_window_candidate, classify_ax_editable_role,
        find_window_candidate_containing_point, pane_candidate_is_meaningful,
        synth_editor_rect_from_cursor, MacWindowCandidate,
    };

    #[test]
    fn classify_text_field_as_single_line() {
        assert_eq!(classify_ax_editable_role("AXTextField", None, None), (true, 1));
    }

    #[test]
    fn classify_text_area_as_multi_line() {
        assert_eq!(classify_ax_editable_role("AXTextArea", None, None), (true, 2));
    }

    #[test]
    fn classify_explicitly_editable_web_area_as_multi_line() {
        assert_eq!(
            classify_ax_editable_role("AXWebArea", None, Some(true)),
            (true, 2)
        );
    }

    #[test]
    fn read_only_text_field_is_not_editable() {
        assert_eq!(
            classify_ax_editable_role("AXTextField", None, Some(false)),
            (false, 0)
        );
    }

    #[test]
    fn editable_document_is_classified_as_code() {
        assert_eq!(
            classify_ax_editable_role("AXDocument", Some("AXSourceEditor"), Some(true)),
            (true, 3)
        );
    }

    #[test]
    fn keep_noneditable_group_closed() {
        assert_eq!(classify_ax_editable_role("AXGroup", None, None), (false, 0));
    }

    #[test]
    fn reject_window_sized_pane_candidate() {
        assert!(!pane_candidate_is_meaningful(
            (0, 0, 1200, 900),
            (100, 700, 320, 40),
            (0, 0, 1200, 900)
        ));
    }

    #[test]
    fn accept_meaningful_nested_pane_candidate() {
        assert!(pane_candidate_is_meaningful(
            (40, 80, 860, 620),
            (120, 540, 360, 44),
            (0, 0, 1200, 900)
        ));
    }

    #[test]
    fn synth_editor_rect_stays_inside_window() {
        let rect = synth_editor_rect_from_cursor((0, 0, 1200, 900), (1180, 880)).unwrap();
        assert!(rect.0 >= 8);
        assert!(rect.1 >= 8);
        assert!(rect.0 + rect.2 <= 1192);
        assert!(rect.1 + rect.3 <= 892);
    }

    #[test]
    fn synth_editor_rect_clamps_inside_tiny_window() {
        let window = (40, 70, 42, 18);
        let rect = synth_editor_rect_from_cursor(window, (78, 84)).unwrap();

        assert!(rect.0 >= window.0);
        assert!(rect.1 >= window.1);
        assert!(rect.0 + rect.2 <= window.0 + window.2);
        assert!(rect.1 + rect.3 <= window.1 + window.3);
        assert!(rect.2 <= 26);
        assert!(rect.3 <= 2);
    }

    #[test]
    fn choose_window_candidate_prefers_matching_focus_window() {
        let candidates = [
            MacWindowCandidate {
                rect: (0, 0, 640, 480),
                foreground_hwnd: 1001,
            },
            MacWindowCandidate {
                rect: (720, 40, 900, 720),
                foreground_hwnd: 2002,
            },
        ];

        let chosen = choose_window_candidate(&candidates, (780, 120, 420, 320)).unwrap();
        assert_eq!(chosen.foreground_hwnd, 2002);
    }

    #[test]
    fn window_point_anchor_picks_topmost_hit() {
        let candidates = [
            MacWindowCandidate {
                rect: (0, 0, 1280, 820),
                foreground_hwnd: 3003,
            },
            MacWindowCandidate {
                rect: (160, 120, 520, 360),
                foreground_hwnd: 4004,
            },
        ];

        let chosen = find_window_candidate_containing_point(&candidates, 320, 260).unwrap();
        assert_eq!(chosen.foreground_hwnd, 3003);
    }
}

// macOS >= 10.15
// https://stackoverflow.com/questions/56597221/detecting-screen-recording-settings-on-macos-catalina/
// remove just one app from all the permissions: tccutil reset All cn.yunjichuangzhi.hdesk
fn legacy_can_record_screen(prompt: bool) -> bool {
    let mut can_record_screen = false;
    unsafe {
        let our_pid: i32 = std::process::id() as _;
        let our_pid: id = msg_send![class!(NSNumber), numberWithInteger: our_pid];
        let window_list =
            CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID);
        let n = CFArrayGetCount(window_list);
        let dock = NSString::alloc(nil).init_str("Dock");
        for i in 0..n {
            let w: id = CFArrayGetValueAtIndex(window_list, i) as _;
            let name: id = msg_send![w, valueForKey: kCGWindowName as id];
            if name.is_null() {
                continue;
            }
            let pid: id = msg_send![w, valueForKey: kCGWindowOwnerPID as id];
            let is_me: BOOL = msg_send![pid, isEqual: our_pid];
            if is_me == YES {
                continue;
            }
            let pid: i32 = msg_send![pid, intValue];
            let p: id = msg_send![
                class!(NSRunningApplication),
                runningApplicationWithProcessIdentifier: pid
            ];
            if p.is_null() {
                // Ignore processes we don't have access to, such as WindowServer.
                continue;
            }
            let url: id = msg_send![p, executableURL];
            let exe_name: id = msg_send![url, lastPathComponent];
            if exe_name.is_null() {
                continue;
            }
            let is_dock: BOOL = msg_send![exe_name, isEqual: dock];
            if is_dock == YES {
                // Ignore the Dock, which provides the desktop picture.
                continue;
            }
            can_record_screen = true;
            break;
        }
    }
    if !can_record_screen {
        use scrap::{Capturer, Display, TraitCapturer};
        if let Ok(display) = Display::primary() {
            if let Ok(mut capturer) = Capturer::new(display) {
                let deadline = Instant::now() + Duration::from_millis(250);
                while Instant::now() < deadline {
                    match capturer.frame(Duration::from_millis(30)) {
                        Ok(_) => {
                            can_record_screen = true;
                            break;
                        }
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(15));
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    }
    can_record_screen
}

fn unsafe_is_can_screen_recording(prompt: bool) -> bool {
    // we got some report that we show no permission even after set it, so we try to use new api for screen recording check
    // the new api is only available on macOS >= 10.15, but on stackoverflow, some people said it works on >= 10.16 (crash on 10.15),
    // but also some said it has bug on 10.16, so we just use it on 11.0.
    unsafe {
        if CanUseNewApiForScreenCaptureCheck() == YES {
            let can_record_screen = IsCanScreenRecording(if prompt { YES } else { NO }) == YES;
            if can_record_screen {
                return true;
            }
        }
    }
    legacy_can_record_screen(prompt)
}

pub fn install_service() -> bool {
    is_installed_daemon(false)
}

// Remember to check if `update_daemon_agent()` need to be changed if changing `is_installed_daemon()`.
// No need to merge the existing dup code, because the code in these two functions are too critical.
// New code should be written in a common function.
pub fn is_installed_daemon(prompt: bool) -> bool {
    let daemon = format!("{}_service.plist", crate::get_full_name());
    let agent = format!("{}_server.plist", crate::get_full_name());
    let agent_plist_file = format!("/Library/LaunchAgents/{}", agent);
    if !prompt {
        // in macos 13, there is new way to check if they are running or enabled, https://developer.apple.com/documentation/servicemanagement/updating-helper-executables-from-earlier-versions-of-macos#Respond-to-changes-in-System-Settings
        if !std::path::Path::new(&format!("/Library/LaunchDaemons/{}", daemon)).exists() {
            return false;
        }
        if !std::path::Path::new(&agent_plist_file).exists() {
            return false;
        }
        return true;
    }

    let Some(install_script) = PRIVILEGES_SCRIPTS_DIR.get_file("install.scpt") else {
        return false;
    };
    let Some(install_script_body) = install_script.contents_utf8().map(correct_app_name) else {
        return false;
    };

    let Some(daemon_plist) = PRIVILEGES_SCRIPTS_DIR.get_file("daemon.plist") else {
        return false;
    };
    let Some(daemon_plist_body) = daemon_plist.contents_utf8().map(correct_app_name) else {
        return false;
    };

    let Some(agent_plist) = PRIVILEGES_SCRIPTS_DIR.get_file("agent.plist") else {
        return false;
    };
    let Some(agent_plist_body) = agent_plist.contents_utf8().map(correct_app_name) else {
        return false;
    };

    std::thread::spawn(move || {
        match std::process::Command::new("osascript")
            .arg("-e")
            .arg(install_script_body)
            .arg(daemon_plist_body)
            .arg(agent_plist_body)
            .arg(&get_active_username())
            .status()
        {
            Err(e) => {
                log::error!("run osascript failed: {}", e);
            }
            _ => {
                let installed = std::path::Path::new(&agent_plist_file).exists();
                log::info!("Agent file {} installed: {}", agent_plist_file, installed);
                if installed {
                    log::info!("launch server");
                    std::process::Command::new("launchctl")
                        .args(&["load", "-w", &agent_plist_file])
                        .status()
                        .ok();
                }
            }
        }
    });
    false
}

fn update_daemon_agent(agent_plist_file: String, update_source_dir: String, sync: bool) {
    let update_script_file = "update.scpt";
    let Some(update_script) = PRIVILEGES_SCRIPTS_DIR.get_file(update_script_file) else {
        return;
    };
    let Some(update_script_body) = update_script.contents_utf8().map(correct_app_name) else {
        return;
    };

    let Some(daemon_plist) = PRIVILEGES_SCRIPTS_DIR.get_file("daemon.plist") else {
        return;
    };
    let Some(daemon_plist_body) = daemon_plist.contents_utf8().map(correct_app_name) else {
        return;
    };
    let Some(agent_plist) = PRIVILEGES_SCRIPTS_DIR.get_file("agent.plist") else {
        return;
    };
    let Some(agent_plist_body) = agent_plist.contents_utf8().map(correct_app_name) else {
        return;
    };

    let func = move || {
        let mut binding = std::process::Command::new("osascript");
        let cmd = binding
            .arg("-e")
            .arg(update_script_body)
            .arg(daemon_plist_body)
            .arg(agent_plist_body)
            .arg(&get_active_username())
            .arg(std::process::id().to_string())
            .arg(update_source_dir);
        match cmd.status() {
            Err(e) => {
                log::error!("run osascript failed: {}", e);
            }
            Ok(status) if !status.success() => {
                log::warn!("run osascript failed with status: {}", status);
            }
            _ => {
                let installed = std::path::Path::new(&agent_plist_file).exists();
                log::info!("Agent file {} installed: {}", &agent_plist_file, installed);
            }
        }
    };
    if sync {
        func();
    } else {
        std::thread::spawn(func);
    }
}

fn correct_app_name(s: &str) -> String {
    let mut s = s.to_owned();
    if let Some(bundleid) = get_bundle_id() {
        s = s.replace("com.carriez.rustdesk", &bundleid);
    }
    s = s.replace("rustdesk", &crate::get_app_name().to_lowercase());
    s = s.replace("RustDesk", &crate::get_app_name());
    s
}

pub fn uninstall_service(show_new_window: bool, sync: bool) -> bool {
    // to-do: do together with win/linux about refactory start/stop service
    if !is_installed_daemon(false) {
        return false;
    }

    let Some(script_file) = PRIVILEGES_SCRIPTS_DIR.get_file("uninstall.scpt") else {
        return false;
    };
    let Some(script_body) = script_file.contents_utf8().map(correct_app_name) else {
        return false;
    };

    let func = move || {
        match std::process::Command::new("osascript")
            .arg("-e")
            .arg(script_body)
            .status()
        {
            Err(e) => {
                log::error!("run osascript failed: {}", e);
            }
            _ => {
                let agent = format!("{}_server.plist", crate::get_full_name());
                let agent_plist_file = format!("/Library/LaunchAgents/{}", agent);
                let uninstalled = !std::path::Path::new(&agent_plist_file).exists();
                log::info!(
                    "Agent file {} uninstalled: {}",
                    agent_plist_file,
                    uninstalled
                );
                if uninstalled {
                    if !show_new_window {
                        let _ = crate::ipc::close_all_instances();
                        // leave ipc a little time
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                    crate::ipc::set_option("stop-service", "Y");
                    std::process::Command::new("launchctl")
                        .args(&["remove", &format!("{}_server", crate::get_full_name())])
                        .status()
                        .ok();
                    if show_new_window {
                        std::process::Command::new("open")
                            .arg("-n")
                            .arg(&format!("/Applications/{}.app", crate::get_app_name()))
                            .spawn()
                            .ok();
                        // leave open a little time
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                    quit_gui();
                }
            }
        }
    };
    if sync {
        func();
    } else {
        std::thread::spawn(func);
    }
    true
}

pub fn get_cursor_pos() -> Option<(i32, i32)> {
    unsafe {
        let e = CGEventCreate(0 as _);
        let point = CGEventGetLocation(e);
        CFRelease(e);
        Some((point.x as _, point.y as _))
    }
    /*
    let mut pt: NSPoint = unsafe { msg_send![class!(NSEvent), mouseLocation] };
    let screen: id = unsafe { msg_send![class!(NSScreen), currentScreenForMouseLocation] };
    let frame: NSRect = unsafe { msg_send![screen, frame] };
    pt.x -= frame.origin.x;
    pt.y -= frame.origin.y;
    Some((pt.x as _, pt.y as _))
    */
}

/// Warp the mouse cursor to the specified screen position.
///
/// # Thread Safety
/// This function affects global cursor state and acquires `CG_CURSOR_MUTEX`.
/// Callers must ensure no nested calls occur while the mutex is held.
///
/// # Arguments
/// * `x` - X coordinate in screen points (macOS uses points, not pixels)
/// * `y` - Y coordinate in screen points
pub fn set_cursor_pos(x: i32, y: i32) -> bool {
    // Acquire lock with deadlock detection in debug builds.
    // In debug builds, try_lock detects re-entrant calls early; on failure we return immediately.
    // In release builds, we use blocking lock() which will wait if contended.
    #[cfg(debug_assertions)]
    let _guard = match CG_CURSOR_MUTEX.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::WouldBlock) => {
            log::error!(
                "[BUG] set_cursor_pos: CG_CURSOR_MUTEX is already held - potential deadlock!"
            );
            debug_assert!(false, "Re-entrant call to set_cursor_pos detected");
            return false;
        }
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
    };
    #[cfg(not(debug_assertions))]
    let _guard = CG_CURSOR_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let result = CGWarpMouseCursorPosition(CGPoint {
            x: x as f64,
            y: y as f64,
        });
        if result != CGError::Success {
            log::error!(
                "CGWarpMouseCursorPosition({}, {}) returned error: {:?}",
                x,
                y,
                result
            );
        }
        result == CGError::Success
    }
}

/// Toggle pointer lock (dissociate/associate mouse from cursor position).
///
/// On macOS, cursor clipping is not supported directly like Windows ClipCursor.
/// Instead, we use CGAssociateMouseAndMouseCursorPosition to dissociate mouse
/// movement from cursor position, achieving a "pointer lock" effect.
///
/// # Thread Safety
/// This function affects global cursor state and acquires `CG_CURSOR_MUTEX`.
/// Callers must ensure only one owner toggles pointer lock at a time;
/// nested Some/None transitions from different call sites may cause unexpected behavior.
///
/// # Arguments
/// * `rect` - When `Some(_)`, dissociates mouse from cursor (enables pointer lock).
///            When `None`, re-associates mouse with cursor (disables pointer lock).
///            The rect coordinate values are ignored on macOS; only `Some`/`None` matters.
///            The parameter signature matches Windows for API consistency.
pub fn clip_cursor(rect: Option<(i32, i32, i32, i32)>) -> bool {
    // Acquire lock with deadlock detection in debug builds.
    // In debug builds, try_lock detects re-entrant calls early; on failure we return immediately.
    // In release builds, we use blocking lock() which will wait if contended.
    #[cfg(debug_assertions)]
    let _guard = match CG_CURSOR_MUTEX.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::WouldBlock) => {
            log::error!("[BUG] clip_cursor: CG_CURSOR_MUTEX is already held - potential deadlock!");
            debug_assert!(false, "Re-entrant call to clip_cursor detected");
            return false;
        }
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
    };
    #[cfg(not(debug_assertions))]
    let _guard = CG_CURSOR_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    // CGAssociateMouseAndMouseCursorPosition takes a boolean_t:
    //   1 (true)  = associate mouse with cursor position (normal mode)
    //   0 (false) = dissociate mouse from cursor position (pointer lock mode)
    // When rect is Some, we want pointer lock (dissociate), so associate = false (0).
    // When rect is None, we want normal mode (associate), so associate = true (1).
    let associate: BooleanT = if rect.is_some() { 0 } else { 1 };
    unsafe {
        let result = CGAssociateMouseAndMouseCursorPosition(associate);
        if result != CGError::Success {
            log::warn!(
                "CGAssociateMouseAndMouseCursorPosition({}) returned error: {:?}",
                associate,
                result
            );
        }
        result == CGError::Success
    }
}

pub fn get_focused_display(displays: Vec<DisplayInfo>) -> Option<usize> {
    autoreleasepool(|| unsafe_get_focused_display(displays))
}

fn unsafe_get_focused_display(displays: Vec<DisplayInfo>) -> Option<usize> {
    unsafe {
        let main_screen: id = msg_send![class!(NSScreen), mainScreen];
        let screen: id = msg_send![main_screen, deviceDescription];
        let id: id =
            msg_send![screen, objectForKey: NSString::alloc(nil).init_str("NSScreenNumber")];
        let display_name: u32 = msg_send![id, unsignedIntValue];

        displays
            .iter()
            .position(|d| d.name == display_name.to_string())
    }
}

pub fn get_cursor() -> ResultType<Option<u64>> {
    autoreleasepool(|| unsafe_get_cursor())
}

fn unsafe_get_cursor() -> ResultType<Option<u64>> {
    unsafe {
        let seed = CGSCurrentCursorSeed();
        if seed == LATEST_SEED {
            return Ok(None);
        }
        LATEST_SEED = seed;
    }
    let c = get_cursor_id()?;
    Ok(Some(c.1))
}

pub fn reset_input_cache() {
    unsafe {
        LATEST_SEED = 0;
    }
}

fn get_cursor_id() -> ResultType<(id, u64)> {
    unsafe {
        let c: id = msg_send![class!(NSCursor), currentSystemCursor];
        if c == nil {
            bail!("Failed to call [NSCursor currentSystemCursor]");
        }
        let hotspot: NSPoint = msg_send![c, hotSpot];
        let img: id = msg_send![c, image];
        if img == nil {
            bail!("Failed to call [NSCursor image]");
        }
        let size: NSSize = msg_send![img, size];
        let tif: id = msg_send![img, TIFFRepresentation];
        if tif == nil {
            bail!("Failed to call [NSImage TIFFRepresentation]");
        }
        let rep: id = msg_send![class!(NSBitmapImageRep), imageRepWithData: tif];
        if rep == nil {
            bail!("Failed to call [NSBitmapImageRep imageRepWithData]");
        }
        let rep_size: NSSize = msg_send![rep, size];
        let mut hcursor =
            size.width + size.height + hotspot.x + hotspot.y + rep_size.width + rep_size.height;
        let x = (rep_size.width * hotspot.x / size.width) as usize;
        let y = (rep_size.height * hotspot.y / size.height) as usize;
        for i in 0..2 {
            let mut x2 = x + i;
            if x2 >= rep_size.width as usize {
                x2 = rep_size.width as usize - 1;
            }
            let mut y2 = y + i;
            if y2 >= rep_size.height as usize {
                y2 = rep_size.height as usize - 1;
            }
            let color: id = msg_send![rep, colorAtX:x2 y:y2];
            if color != nil {
                let r: f64 = msg_send![color, redComponent];
                let g: f64 = msg_send![color, greenComponent];
                let b: f64 = msg_send![color, blueComponent];
                let a: f64 = msg_send![color, alphaComponent];
                hcursor += (r + g + b + a) * (255 << i) as f64;
            }
        }
        Ok((c, hcursor as _))
    }
}

pub fn get_cursor_data(hcursor: u64) -> ResultType<CursorData> {
    autoreleasepool(|| unsafe_get_cursor_data(hcursor))
}

// https://github.com/stweil/OSXvnc/blob/master/OSXvnc-server/mousecursor.c
fn unsafe_get_cursor_data(hcursor: u64) -> ResultType<CursorData> {
    unsafe {
        let (c, hcursor2) = get_cursor_id()?;
        if hcursor != hcursor2 {
            bail!("cursor changed");
        }
        let hotspot: NSPoint = msg_send![c, hotSpot];
        let img: id = msg_send![c, image];
        let size: NSSize = msg_send![img, size];
        let reps: id = msg_send![img, representations];
        if reps == nil {
            bail!("Failed to call [NSImage representations]");
        }
        let nreps: usize = msg_send![reps, count];
        if nreps == 0 {
            bail!("Get empty [NSImage representations]");
        }
        let rep: id = msg_send![reps, objectAtIndex: 0];
        /*
        let n: id = msg_send![class!(NSNumber), numberWithFloat:1.0];
        let props: id = msg_send![class!(NSDictionary), dictionaryWithObject:n forKey:NSString::alloc(nil).init_str("NSImageCompressionFactor")];
        let image_data: id = msg_send![rep, representationUsingType:2 properties:props];
        let () = msg_send![image_data, writeToFile:NSString::alloc(nil).init_str("cursor.jpg") atomically:0];
        */
        let mut colors: Vec<u8> = Vec::new();
        colors.reserve((size.height * size.width) as usize * 4);
        // TIFF is rgb colorspace, no need to convert
        // let cs: id = msg_send![class!(NSColorSpace), sRGBColorSpace];
        for y in 0..(size.height as _) {
            for x in 0..(size.width as _) {
                let color: id = msg_send![rep, colorAtX:x as cocoa::foundation::NSInteger y:y as cocoa::foundation::NSInteger];
                // let color: id = msg_send![color, colorUsingColorSpace: cs];
                if color == nil {
                    continue;
                }
                let r: f64 = msg_send![color, redComponent];
                let g: f64 = msg_send![color, greenComponent];
                let b: f64 = msg_send![color, blueComponent];
                let a: f64 = msg_send![color, alphaComponent];
                colors.push((r * 255.) as _);
                colors.push((g * 255.) as _);
                colors.push((b * 255.) as _);
                colors.push((a * 255.) as _);
            }
        }
        Ok(CursorData {
            id: hcursor,
            colors: colors.into(),
            hotx: hotspot.x as _,
            hoty: hotspot.y as _,
            width: size.width as _,
            height: size.height as _,
            ..Default::default()
        })
    }
}

fn get_active_user(t: &str) -> String {
    if let Ok(output) = std::process::Command::new("ls")
        .args(vec![t, "/dev/console"])
        .output()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(n) = line.split_whitespace().nth(2) {
                return n.to_owned();
            }
        }
    }
    "".to_owned()
}

pub fn get_active_username() -> String {
    get_active_user("-l")
}

pub fn get_active_userid() -> String {
    get_active_user("-n")
}

pub fn get_active_user_home() -> Option<PathBuf> {
    let username = get_active_username();
    if !username.is_empty() {
        let home = PathBuf::from(format!("/Users/{}", username));
        if home.exists() {
            return Some(home);
        }
    }
    None
}

pub fn is_prelogin() -> bool {
    get_active_userid() == "0"
}

// https://stackoverflow.com/questions/11505255/osx-check-if-the-screen-is-locked
// No "CGSSessionScreenIsLocked" can be found when macOS is not locked.
//
// `ioreg -n Root -d1` returns `"CGSSessionScreenIsLocked"=Yes`
// `ioreg -n Root -d1 -a` returns
// ```
// ...
//    <key>CGSSessionScreenIsLocked</key>
//    <true/>
// ...
// ```
pub fn is_locked() -> bool {
    match std::process::Command::new("ioreg")
        .arg("-n")
        .arg("Root")
        .arg("-d1")
        .output()
    {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            // Although `"CGSSessionScreenIsLocked"=Yes` was printed on my macOS,
            // I also check `"CGSSessionScreenIsLocked"=true` for better compability.
            output_str.contains("\"CGSSessionScreenIsLocked\"=Yes")
                || output_str.contains("\"CGSSessionScreenIsLocked\"=true")
        }
        Err(e) => {
            log::error!("Failed to query ioreg for the lock state: {}", e);
            false
        }
    }
}

pub fn is_root() -> bool {
    crate::username() == "root"
}

pub fn run_as_user(arg: Vec<&str>) -> ResultType<Option<std::process::Child>> {
    let uid = get_active_userid();
    let cmd = std::env::current_exe()?;
    let mut args = vec!["asuser", &uid, cmd.to_str().unwrap_or("")];
    args.append(&mut arg.clone());
    let task = std::process::Command::new("launchctl").args(args).spawn()?;
    Ok(Some(task))
}

pub fn lock_screen() {
    std::process::Command::new(
        "/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession",
    )
    .arg("-suspend")
    .output()
    .ok();
}

pub fn start_os_service() {
    log::info!("Username: {}", crate::username());
    if let Err(err) = crate::ipc::start("_service") {
        log::error!("Failed to start ipc_service: {}", err);
    }

    /* // mouse/keyboard works in prelogin now with launchctl asuser.
       // below can avoid multi-users logged in problem, but having its own below problem.
       // Not find a good way to start --cm without root privilege (affect file transfer).
       // one way is to start with `launchctl asuser <uid> open -n -a /Applications/RustDesk.app/ --args --cm`,
       // this way --cm is started with the user privilege, but we will have problem to start another RustDesk.app
       // with open in explorer.
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let mut uid = "".to_owned();
        let mut server: Option<std::process::Child> = None;
        if let Err(err) = ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        }) {
            println!("Failed to set Ctrl-C handler: {}", err);
        }
        while running.load(Ordering::SeqCst) {
            let tmp = get_active_userid();
            let mut start_new = false;
            if tmp != uid && !tmp.is_empty() {
                uid = tmp;
                log::info!("active uid: {}", uid);
                if let Some(ps) = server.as_mut() {
                    hbb_common::allow_err!(ps.kill());
                }
            }
            if let Some(ps) = server.as_mut() {
                match ps.try_wait() {
                    Ok(Some(_)) => {
                        server = None;
                        start_new = true;
                    }
                    _ => {}
                }
            } else {
                start_new = true;
            }
            if start_new {
                match run_as_user("--server") {
                    Ok(Some(ps)) => server = Some(ps),
                    Err(err) => {
                        log::error!("Failed to start server: {}", err);
                    }
                    _ => { /*no happen*/ }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(super::SERVICE_INTERVAL));
        }

        if let Some(ps) = server.take().as_mut() {
            hbb_common::allow_err!(ps.kill());
        }
        log::info!("Exit");
    */
}

pub fn toggle_blank_screen(_v: bool) {
    // https://unix.stackexchange.com/questions/17115/disable-keyboard-mouse-temporarily
}

pub fn block_input(_v: bool) -> (bool, String) {
    (true, "".to_owned())
}

pub fn is_installed() -> bool {
    if let Ok(p) = std::env::current_exe() {
        return p
            .to_str()
            .unwrap_or_default()
            .starts_with(&format!("/Applications/{}.app", crate::get_app_name()));
    }
    false
}

pub fn quit_gui() {
    unsafe {
        let () = msg_send!(NSApp(), terminate: nil);
    };
}

#[inline]
pub fn try_remove_temp_update_dir(dir: Option<&str>) {
    let target_path_buf = dir.map(PathBuf::from).unwrap_or_else(get_update_temp_dir);
    let target_path = target_path_buf.as_path();
    if target_path.exists() {
        std::fs::remove_dir_all(target_path).ok();
    }
}

pub fn update_me() -> ResultType<()> {
    let is_installed_daemon = is_installed_daemon(false);
    let option_stop_service = "stop-service";
    let is_service_stopped = hbb_common::config::option2bool(
        option_stop_service,
        &crate::ui_interface::get_option(option_stop_service),
    );

    let cmd = std::env::current_exe()?;
    // RustDesk.app/Contents/MacOS/RustDesk
    let app_dir = cmd
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(|d| d.to_string_lossy().to_string());
    let Some(app_dir) = app_dir else {
        bail!("Unknown app directory of current exe file: {:?}", cmd);
    };

    let app_name = crate::get_app_name();
    if is_installed_daemon && !is_service_stopped {
        let agent = format!("{}_server.plist", crate::get_full_name());
        let agent_plist_file = format!("/Library/LaunchAgents/{}", agent);
        update_daemon_agent(agent_plist_file, app_dir, true);
    } else {
        // `kill -9` may not work without "administrator privileges"
        let update_body = r#"
on run {app_name, cur_pid, app_dir, user_name}
    set app_bundle to "/Applications/" & app_name & ".app"
    set app_bundle_q to quoted form of app_bundle
    set app_dir_q to quoted form of app_dir
    set user_name_q to quoted form of user_name

    set check_source to "test -d " & app_dir_q & " || exit 1;"
    set kill_others to "pids=$(pgrep -x '" & app_name & "' | grep -vx " & cur_pid & " || true); if [ -n \"$pids\" ]; then echo \"$pids\" | xargs kill -9 || true; fi;"
    set copy_files to "rm -rf " & app_bundle_q & " && ditto " & app_dir_q & " " & app_bundle_q & " && chown -R " & user_name_q & ":staff " & app_bundle_q & " && (xattr -r -d com.apple.quarantine " & app_bundle_q & " || true);"
    set sh to "set -e;" & check_source & kill_others & copy_files

    do shell script sh with prompt app_name & " wants to update itself" with administrator privileges
end run
        "#;
        let active_user = get_active_username();
        let status = Command::new("osascript")
            .arg("-e")
            .arg(update_body)
            .arg(app_name.to_string())
            .arg(std::process::id().to_string())
            .arg(app_dir)
            .arg(active_user)
            .status();
        match status {
            Ok(status) if !status.success() => {
                log::error!("osascript execution failed with status: {}", status);
            }
            Err(e) => {
                log::error!("run osascript failed: {}", e);
            }
            _ => {}
        }
    }
    std::process::Command::new("open")
        .arg("-n")
        .arg(&format!("/Applications/{}.app", app_name))
        .spawn()
        .ok();
    // leave open a little time
    std::thread::sleep(std::time::Duration::from_millis(300));
    Ok(())
}

pub fn update_from_dmg(dmg_path: &str) -> ResultType<()> {
    let update_temp_dir = get_update_temp_dir_string();
    println!("Starting update from DMG: {}", dmg_path);
    extract_dmg(dmg_path, &update_temp_dir)?;
    println!("DMG extracted");
    update_extracted(&update_temp_dir)?;
    println!("Update process started");
    Ok(())
}

pub fn update_to(_file: &str) -> ResultType<()> {
    let update_temp_dir = get_update_temp_dir_string();
    update_extracted(&update_temp_dir)?;
    Ok(())
}

pub fn extract_update_dmg(file: &str) {
    let update_temp_dir = get_update_temp_dir_string();
    let mut evt: HashMap<&str, String> =
        HashMap::from([("name", "extract-update-dmg".to_string())]);
    match extract_dmg(file, &update_temp_dir) {
        Ok(_) => {
            log::info!("Extracted dmg file to {}", update_temp_dir);
        }
        Err(e) => {
            evt.insert("err", e.to_string());
            log::error!("Failed to extract dmg file {}: {}", file, e);
        }
    }
    let evt = serde_json::ser::to_string(&evt).unwrap_or("".to_owned());
    #[cfg(feature = "flutter")]
    crate::flutter::push_global_event(crate::flutter::APP_TYPE_MAIN, evt);
}

fn extract_dmg(dmg_path: &str, target_dir: &str) -> ResultType<()> {
    let mount_point = "/Volumes/RustDeskUpdate";
    let target_path = Path::new(target_dir);

    if target_path.exists() {
        std::fs::remove_dir_all(target_path)?;
    }
    std::fs::create_dir_all(target_path)?;

    let status = Command::new("hdiutil")
        .args(&["attach", "-nobrowse", "-mountpoint", mount_point, dmg_path])
        .status()?;

    if !status.success() {
        bail!("Failed to attach DMG image at {}: {:?}", dmg_path, status);
    }

    struct DmgGuard(&'static str);
    impl Drop for DmgGuard {
        fn drop(&mut self) {
            let _ = Command::new("hdiutil")
                .args(&["detach", self.0, "-force"])
                .status();
        }
    }
    let _guard = DmgGuard(mount_point);

    let app_name = format!("{}.app", crate::get_app_name());
    let src_path = format!("{}/{}", mount_point, app_name);
    let dest_path = format!("{}/{}", target_dir, app_name);

    let copy_status = Command::new("ditto")
        .args(&[&src_path, &dest_path])
        .status()?;

    if !copy_status.success() {
        bail!(
            "Failed to copy application from {} to {}: {:?}",
            src_path,
            dest_path,
            copy_status
        );
    }

    if !Path::new(&dest_path).exists() {
        bail!(
            "Copy operation failed - destination not found at {}",
            dest_path
        );
    }

    Ok(())
}

fn update_extracted(target_dir: &str) -> ResultType<()> {
    let app_name = crate::get_app_name();
    let exe_path = format!(
        "{}/{}.app/Contents/MacOS/{}",
        target_dir, app_name, app_name
    );
    let _child = unsafe {
        if let Err(e) = Command::new(&exe_path)
            .arg("--update")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .pre_exec(|| {
                hbb_common::libc::setsid();
                Ok(())
            })
            .spawn()
        {
            try_remove_temp_update_dir(Some(target_dir));
            bail!(e);
        }
    };
    Ok(())
}

pub fn get_double_click_time() -> u32 {
    // to-do: https://github.com/servo/core-foundation-rs/blob/786895643140fa0ee4f913d7b4aeb0c4626b2085/cocoa/src/appkit.rs#L2823
    500 as _
}

pub fn hide_dock() {
    unsafe {
        NSApp().setActivationPolicy_(NSApplicationActivationPolicyAccessory);
    }
}

#[inline]
#[allow(dead_code)]
fn get_server_start_time_of(p: &Process, path: &Path) -> Option<i64> {
    let cmd = p.cmd();
    if cmd.len() <= 1 {
        return None;
    }
    if &cmd[1] != "--server" {
        return None;
    }
    let Ok(cur) = std::fs::canonicalize(p.exe()) else {
        return None;
    };
    if &cur != path {
        return None;
    }
    Some(p.start_time() as _)
}

#[inline]
#[allow(dead_code)]
fn get_server_start_time(sys: &mut System, path: &Path) -> Option<(i64, Pid)> {
    sys.refresh_processes_specifics(ProcessRefreshKind::new());
    for (_, p) in sys.processes() {
        if let Some(t) = get_server_start_time_of(p, path) {
            return Some((t, p.pid() as _));
        }
    }
    None
}

pub fn handle_application_should_open_untitled_file() {
    hbb_common::log::debug!("icon clicked on finder");
    let x = std::env::args().nth(1).unwrap_or_default();
    if x == "--server" || x == "--cm" || x == "--tray" {
        std::thread::spawn(move || crate::handle_url_scheme("".to_lowercase()));
    }
}

/// Get all resolutions of the display. The resolutions are:
/// 1. Sorted by width and height in descending order, with duplicates removed.
/// 2. Filtered out if the width is less than 800 (800x600) if there are too many (e.g., >15).
/// 3. Contain HiDPI resolutions and the real resolutions.
///
/// We don't need to distinguish between HiDPI and real resolutions.
/// When the controlling side changes the resolution, it will call `change_resolution_directly()`.
/// `change_resolution_directly()` will try to use the HiDPI resolution first.
/// This is how teamviewer does it for now.
///
/// If we need to distinguish HiDPI and real resolutions, we can add a flag to the `Resolution` struct.
pub fn resolutions(name: &str) -> Vec<Resolution> {
    let mut v = vec![];
    if let Ok(display) = name.parse::<u32>() {
        let mut num = 0;
        unsafe {
            if YES == MacGetModeNum(display, &mut num) {
                let (mut widths, mut heights, mut _hidpis) =
                    (vec![0; num as _], vec![0; num as _], vec![NO; num as _]);
                let mut real_num = 0;
                if YES
                    == MacGetModes(
                        display,
                        widths.as_mut_ptr(),
                        heights.as_mut_ptr(),
                        _hidpis.as_mut_ptr(),
                        num,
                        &mut real_num,
                    )
                {
                    if real_num <= num {
                        v = (0..real_num)
                            .map(|i| Resolution {
                                width: widths[i as usize] as _,
                                height: heights[i as usize] as _,
                                ..Default::default()
                            })
                            .collect::<Vec<_>>();
                        // Sort by (w, h), desc
                        v.sort_by(|a, b| {
                            if a.width == b.width {
                                b.height.cmp(&a.height)
                            } else {
                                b.width.cmp(&a.width)
                            }
                        });
                        // Remove duplicates
                        v.dedup_by(|a, b| a.width == b.width && a.height == b.height);
                        // Filter out the ones that are less than width 800 (800x600) if there are too many.
                        // We can also do this filtering on the client side, but it is better not to change the client side to reduce the impact.
                        if v.len() > 15 {
                            // Most width > 800, so it's ok to remove the small ones.
                            v.retain(|r| r.width >= 800);
                        }
                        if v.len() > 15 {
                            // Ignore if the length is still too long.
                        }
                    }
                }
            }
        }
    }
    v
}

pub fn current_resolution(name: &str) -> ResultType<Resolution> {
    let display = name.parse::<u32>().map_err(|e| anyhow!(e))?;
    unsafe {
        let (mut width, mut height) = (0, 0);
        if NO == MacGetMode(display, &mut width, &mut height) {
            bail!("MacGetMode failed");
        }
        Ok(Resolution {
            width: width as _,
            height: height as _,
            ..Default::default()
        })
    }
}

pub fn change_resolution_directly(name: &str, width: usize, height: usize) -> ResultType<()> {
    let display = name.parse::<u32>().map_err(|e| anyhow!(e))?;
    unsafe {
        if NO == MacSetMode(display, width as _, height as _, true) {
            bail!("MacSetMode failed");
        }
    }
    Ok(())
}

pub fn check_super_user_permission() -> ResultType<bool> {
    unsafe { Ok(MacCheckAdminAuthorization() == YES) }
}

pub fn elevate(args: Vec<&str>, prompt: &str) -> ResultType<bool> {
    let cmd = std::env::current_exe()?;
    match cmd.to_str() {
        Some(cmd) => {
            let mut cmd_with_args = cmd.to_string();
            for arg in args {
                cmd_with_args = format!("{} {}", cmd_with_args, arg);
            }
            let script = format!(
                r#"do shell script "{}" with prompt "{}" with administrator privileges"#,
                cmd_with_args, prompt
            );
            match std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .arg(&get_active_username())
                .status()
            {
                Err(e) => {
                    bail!("Failed to run osascript: {}", e);
                }
                Ok(status) => Ok(status.success() && status.code() == Some(0)),
            }
        }
        None => {
            bail!("Failed to get current exe str");
        }
    }
}

pub struct WakeLock(Option<keepawake::AwakeHandle>);

impl WakeLock {
    pub fn new(display: bool, idle: bool, sleep: bool) -> Self {
        WakeLock(
            keepawake::Builder::new()
                .display(display)
                .idle(idle)
                .sleep(sleep)
                .create()
                .ok(),
        )
    }

    pub fn set_display(&mut self, display: bool) -> ResultType<()> {
        self.0
            .as_mut()
            .map(|h| h.set_display(display))
            .ok_or(anyhow!("no AwakeHandle"))?
    }
}

fn get_bundle_id() -> Option<String> {
    unsafe {
        let bundle: id = msg_send![class!(NSBundle), mainBundle];
        if bundle.is_null() {
            return None;
        }

        let bundle_id: id = msg_send![bundle, bundleIdentifier];
        if bundle_id.is_null() {
            return None;
        }

        let c_str: *const std::os::raw::c_char = msg_send![bundle_id, UTF8String];
        if c_str.is_null() {
            return None;
        }

        let bundle_id_str = std::ffi::CStr::from_ptr(c_str)
            .to_string_lossy()
            .to_string();
        Some(bundle_id_str)
    }
}
