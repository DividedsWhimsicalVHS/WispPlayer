//! The Android launcher of Wisp Player. Android starts `MainActivity` (a `NativeActivity`), which
//! loads this library and calls `android_main` on a thread of its own; that hands the activity to the
//! same player the computers run (`wisp::run`). Everything Java-side lives in `app/` next to this crate.

mod bridge;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    // The app's private folder holds the library index and the settings.
    if let Some(dir) = app.internal_data_path() {
        wisp::host::set_data_dir(dir);
    }
    // SAFETY: both pointers come from `android-activity` and stay valid until the activity is destroyed.
    unsafe { bridge::attach(app.vm_as_ptr(), app.activity_as_ptr()) };
    wisp::host::install(bridge::HOOKS);

    if let Err(e) = slint::android::init(app) {
        eprintln!("wisp: cannot start the window: {e}");
    } else if let Err(e) = wisp::run() {
        eprintln!("wisp: {e}");
    }
    // The player lives in this thread, so when the activity is gone the music is too. Slint can also
    // only be started once per process: end it here, rather than let a new activity reuse the process.
    std::process::exit(0);
}
