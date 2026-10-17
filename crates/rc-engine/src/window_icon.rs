//! The window's title-bar and taskbar icon on Windows: winit registers its window class without one, so the
//! executable's icon group (id 1, linked by `build.rs`) is loaded and sent to each new window. Elsewhere nothing: macOS
//! shows a bundle's icon, and a bare Linux binary has none.

use bevy::prelude::*;

pub struct WindowIconPlugin;

impl Plugin for WindowIconPlugin {
    #[allow(unused_variables)]
    fn build(&self, app: &mut App) {
        #[cfg(all(windows, target_env = "msvc"))]
        app.add_systems(Update, win32::set_icon);
    }
}

#[cfg(all(windows, target_env = "msvc"))]
mod win32 {
    use bevy::prelude::*;
    use bevy::window::RawHandleWrapper;
    use raw_window_handle::RawWindowHandle;
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" { fn GetModuleHandleW(name: *const u16) -> *mut c_void; }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn LoadImageW(instance: *mut c_void, name: *const u16, kind: u32, cx: i32, cy: i32, load: u32) -> *mut c_void;
        fn GetSystemMetrics(index: i32) -> i32;
        fn PostMessageW(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> i32;
    }

    const GROUP_ID: usize = 1;
    const IMAGE_ICON: u32 = 1;
    const WM_SETICON: u32 = 0x80;
    const ICON_SMALL: usize = 0;
    const ICON_BIG: usize = 1;
    const SM_CXICON: i32 = 11;
    const SM_CYICON: i32 = 12;
    const SM_CXSMICON: i32 = 49;
    const SM_CYSMICON: i32 = 50;

    /// Posted, not sent: this system may run off the thread that owns the window, which is busy in the app update.
    pub fn set_icon(windows: Query<&RawHandleWrapper, Added<RawHandleWrapper>>) {
        for w in &windows {
            let RawWindowHandle::Win32(h) = w.get_window_handle() else { continue };
            let hwnd = h.hwnd.get() as *mut c_void;
            for (which, cx, cy) in [(ICON_BIG, SM_CXICON, SM_CYICON), (ICON_SMALL, SM_CXSMICON, SM_CYSMICON)] {
                // SAFETY: the module handle of the running executable, a resource id (MAKEINTRESOURCE) and a live
                // window handle; a missing resource returns null and is skipped.
                unsafe {
                    let icon = LoadImageW(GetModuleHandleW(std::ptr::null()), GROUP_ID as *const u16, IMAGE_ICON,
                        GetSystemMetrics(cx), GetSystemMetrics(cy), 0);
                    if !icon.is_null() { PostMessageW(hwnd, WM_SETICON, which, icon as isize); }                }
            }
        }
    }
}
