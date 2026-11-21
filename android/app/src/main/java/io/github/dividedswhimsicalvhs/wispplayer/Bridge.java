package io.github.dividedswhimsicalvhs.wispplayer;

/** What the player library implements for the Java side (android/src/bridge.rs). */
final class Bridge {
    static final int PLAY = 0;
    static final int PAUSE = 1;
    static final int NEXT = 2;
    static final int PREV = 3;

    static {
        // NativeActivity loads the library itself, but the VM only finds the native methods of a class
        // whose class loader loaded it through System.loadLibrary.
        System.loadLibrary("wispplayer");
    }

    private Bridge() {}

    /** A playback button was pressed outside the window: one of the constants above. */
    static native void action(int what);

    /** The permission dialog was answered: look for the music (a no-op when it was refused). */
    static native void permissionsGranted();
}
