#!/usr/bin/env bash
# Runs inside the CI emulator (see .github/workflows/ci.yml): installs the debug APK, puts a few songs
# in the Music folder, starts the player and takes screenshots into ./shots. It does not judge them; a
# person (or Claude) looks at the pictures, and the script only fails when the app cannot even start.
set -ux

package=io.github.dividedswhimsicalvhs.wispplayer
mkdir -p shots

adb install -g -r apk/app-debug.apk || exit 1

# Songs for the library, then tell Android's media scanner about them.
adb shell mkdir -p /sdcard/Music
adb push music/. /sdcard/Music/
# (The file names have no spaces, so the loop may split on them.)
for f in $(adb shell find /sdcard/Music -type f | tr -d '\r'); do
  adb shell am broadcast -a android.intent.action.MEDIA_SCANNER_SCAN_FILE -d "file://$f" > /dev/null
done

adb logcat -c
adb shell am start -n "$package/.MainActivity"
sleep 25
adb exec-out screencap -p > shots/01-launch.png

# Is it still alive?
if ! adb shell pidof "$package" > /dev/null; then
  echo "the app is not running"
  adb logcat -d > shots/logcat.txt
  exit 1
fi

# The tap coordinates assume the emulator's 1080x2400 screen; later steps are added once the first
# screenshots show where things are.
size=$(adb shell wm size | tr -d '\r')
echo "$size" > shots/screen-size.txt

# Tap the first song of the list, let it play for a few seconds, and look again.
adb shell input tap 540 520
sleep 8
adb exec-out screencap -p > shots/02-playing.png

# Home, then the notification shade: the media notification should be there with the app out of sight.
adb shell input keyevent KEYCODE_HOME
sleep 3
adb shell cmd statusbar expand-notifications
sleep 2
adb exec-out screencap -p > shots/03-notification.png
adb shell cmd statusbar collapse

adb logcat -d > shots/logcat.txt
adb shell dumpsys media_session | head -60 > shots/media-session.txt
exit 0
