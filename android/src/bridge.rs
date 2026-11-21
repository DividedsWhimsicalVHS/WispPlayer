//! The glue between the player and the Java side (`Bridge`, `MainActivity` and `PlaybackService` in
//! `app/src/main/java`): it tells the activity what is playing, and passes on what Java receives (a
//! button of the media notification, the answer to the storage permission).

// Only `android_main` (Android builds) uses the calls into Java; the rest still compiles on a computer.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::ffi::c_void;
use std::sync::Mutex;

use jni::objects::JObject;
use jni::refs::Global;
use jni::sys::jobject;
use jni::{Env, JValue, JavaVM, jni_sig, jni_str};
use wisp::host::{Hooks, MediaAction, NowPlaying};

/// The activity the player calls into. Android may create a new one while the process lives, which
/// replaces this one.
static ACTIVITY: Mutex<Option<Global<JObject<'static>>>> = Mutex::new(None);

pub const HOOKS: Hooks = Hooks { now_playing, leave };

/// Keeps a reference to the activity for the calls below.
///
/// # Safety
/// `vm` and `activity` must be the pointers `android-activity` passes to `android_main`.
pub unsafe fn attach(vm: *mut c_void, activity: *mut c_void) {
    // SAFETY: the caller promises that `vm` is the process's JavaVM.
    let vm = unsafe { JavaVM::from_raw(vm.cast()) };
    let attached = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        let raw: jobject = activity.cast();
        // SAFETY: the caller promises that `activity` is a live reference to the activity.
        let borrowed = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
        let global = env.new_global_ref(&*borrowed)?;
        *ACTIVITY.lock().unwrap() = Some(global);
        Ok(())
    });
    if let Err(e) = attached {
        eprintln!("wisp: no Java activity: {e}");
    }
}

/// Runs `call` on the activity from whichever thread asks. Failures are logged, never fatal: a
/// notification that does not update must not stop the music.
fn with_activity(what: &str, call: impl FnOnce(&mut Env, &JObject) -> jni::errors::Result<()>) {
    let guard = ACTIVITY.lock().unwrap();
    let (Some(activity), Ok(vm)) = (guard.as_ref(), JavaVM::singleton()) else { return };
    if let Err(e) = vm.attach_current_thread(|env| call(env, activity.as_ref())) {
        eprintln!("wisp: {what} failed: {e}");
    }
}

/// `MainActivity.nowPlaying(title, artist, playing)`: the media notification and the foreground
/// service that keeps the music going with the screen off.
fn now_playing(now: &NowPlaying) {
    with_activity("now playing", |env, activity| {
        let title = env.new_string(now.title)?;
        let artist = env.new_string(now.artist)?;
        env.call_method(
            activity,
            jni_str!("nowPlaying"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;Z)V"),
            &[JValue::Object(&title), JValue::Object(&artist), JValue::Bool(now.playing)],
        )?;
        Ok(())
    });
}

/// Back on the first screen: `Activity.moveTaskToBack(true)`. Finishing the activity would end the
/// process, and the music with it.
fn leave() {
    with_activity("leaving", |env, activity| {
        env.call_method(activity, jni_str!("moveTaskToBack"), jni_sig!("(Z)Z"), &[JValue::Bool(true)])?;
        Ok(())
    });
}

/// `Bridge.action(int)`: a media button was pressed (0 play, 1 pause, 2 next, 3 previous).
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn Java_io_github_dividedswhimsicalvhs_wispplayer_Bridge_action(_env: *mut c_void, _class: *mut c_void, what: i32) {
    let action = match what {
        0 => MediaAction::Play,
        1 => MediaAction::Pause,
        2 => MediaAction::Next,
        3 => MediaAction::Prev,
        _ => return,
    };
    wisp::host::media_action(action);
}

/// `Bridge.permissionsGranted()`: the user allowed reading the music, so look for it.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn Java_io_github_dividedswhimsicalvhs_wispplayer_Bridge_permissionsGranted(_env: *mut c_void, _class: *mut c_void) {
    wisp::host::rescan();
}
