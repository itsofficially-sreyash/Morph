// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Native Wayland ignores "always on top" and window positions, so the orb would fall
    // behind the file manager as soon as a drag starts. Run through XWayland instead.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("GDK_BACKEND").is_none() {
        // SAFETY: still single-threaded; nothing else reads the environment yet.
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }
    morph_lib::run()
}
