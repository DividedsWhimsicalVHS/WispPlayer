package io.github.dividedswhimsicalvhs.wispplayer;

import android.Manifest;
import android.app.NativeActivity;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.Bundle;

import java.util.ArrayList;

/**
 * The window of the player. The drawing and all the logic are native (android/src/lib.rs); this class
 * only asks for what Android makes an app ask for, and hands what plays to the playback service.
 */
public class MainActivity extends NativeActivity {
    private static final int REQUEST_PERMISSIONS = 1;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        askForPermissions();
    }

    /** Reading the music and, from Android 13, showing the notification with the playback buttons. */
    private void askForPermissions() {
        ArrayList<String> missing = new ArrayList<>();
        String audio = Build.VERSION.SDK_INT >= 33
                ? Manifest.permission.READ_MEDIA_AUDIO
                : Manifest.permission.READ_EXTERNAL_STORAGE;
        if (checkSelfPermission(audio) != PackageManager.PERMISSION_GRANTED) {
            missing.add(audio);
        }
        if (Build.VERSION.SDK_INT >= 33
                && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            missing.add(Manifest.permission.POST_NOTIFICATIONS);
        }
        if (!missing.isEmpty()) {
            requestPermissions(missing.toArray(new String[0]), REQUEST_PERMISSIONS);
        }
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(code, permissions, results);
        if (code == REQUEST_PERMISSIONS) {
            Bridge.permissionsGranted();
        }
    }

    /** Called by the player (android/src/bridge.rs) whenever the song or whether it plays changes. */
    public void nowPlaying(String title, String artist, boolean playing) {
        PlaybackService.show(this, title, artist, playing);
    }
}
