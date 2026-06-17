use enigo::{Enigo, Mouse, Settings};

/// Get the global cursor position in physical pixels.
/// Returns None if the platform backend fails to initialize.
pub fn get_cursor_position() -> Option<(i32, i32)> {
    let enigo = Enigo::new(&Settings::default()).ok()?;
    enigo.location().ok()
}

/// Detect whether the system focus is currently on a text-input control.
///
/// On Windows this checks whether the foreground thread has an active
/// caret (text insertion point) via `GetGUIThreadInfo`.
/// On other platforms it conservatively returns `false`, causing the
/// window to be centred rather than placed near the cursor.
pub fn is_input_focused() -> bool {
    #[cfg(target_os = "windows")]
    {
        is_input_focused_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

// ── Windows implementation ───────────────────────────────────────────

#[cfg(target_os = "windows")]
fn is_input_focused_windows() -> bool {
    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct GuiThreadInfo {
        cb_size: u32,
        flags: u32,
        hwnd_active: isize,
        hwnd_focus: isize,
        hwnd_capture: isize,
        hwnd_menu_owner: isize,
        hwnd_move_size: isize,
        hwnd_caret: isize,
        rc_caret: Rect,
    }

    extern "system" {
        fn GetForegroundWindow() -> isize;
        fn GetWindowThreadProcessId(hwnd: isize, process_id: *mut u32) -> u32;
        fn GetGUIThreadInfo(thread_id: u32, gui: *mut GuiThreadInfo) -> i32;
    }

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 {
            return false;
        }

        let thread_id = GetWindowThreadProcessId(hwnd, std::ptr::null_mut());
        if thread_id == 0 {
            return false;
        }

        let mut gui: GuiThreadInfo = std::mem::zeroed();
        gui.cb_size = std::mem::size_of::<GuiThreadInfo>() as u32;

        if GetGUIThreadInfo(thread_id, &mut gui) == 0 {
            return false;
        }

        // hwndCaret ≠ 0 → there is a visible text caret in the focused window.
        // This is the most reliable indicator of an active edit control.
        gui.hwnd_caret != 0
    }
}
