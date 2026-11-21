package io.github.dividedswhimsicalvhs.wispplayer;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.ServiceInfo;
import android.graphics.drawable.Icon;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;

/**
 * Keeps the music going while the app is out of sight and shows what plays in a notification with
 * playback buttons (on the lock screen too). The player itself is native: this service mirrors its
 * state, and passes the buttons, a call taking the audio, and unplugged headphones back to it.
 */
public class PlaybackService extends Service {
    private static final String CHANNEL = "playback";
    private static final int NOTIFICATION = 1;
    private static final String EXTRA_WHAT = "what";
    private static final String EXTRA_TITLE = "title";
    private static final String EXTRA_ARTIST = "artist";

    /** The running service, if there is one: the player's updates go straight to it. */
    private static volatile PlaybackService running;

    private final Handler main = new Handler(Looper.getMainLooper());
    private MediaSession session;
    private AudioManager audio;
    private AudioFocusRequest focus;
    private PowerManager.WakeLock wake;
    private boolean foreground;

    private final BroadcastReceiver unplugged = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            Bridge.action(Bridge.PAUSE);
        }
    };

    /** Shows the song, starting the service when it is not running. Called from the player's thread. */
    static void show(Context context, String title, String artist, boolean playing) {
        PlaybackService service = running;
        if (title.isEmpty()) {
            // Nothing left to play.
            if (service != null) {
                service.main.post(service::stopSelf);
            }
        } else if (service != null) {
            service.main.post(() -> service.update(title, artist, playing));
        } else if (playing) {
            context.startForegroundService(new Intent(context, PlaybackService.class)
                    .putExtra(EXTRA_TITLE, title)
                    .putExtra(EXTRA_ARTIST, artist));
        }
    }

    @Override
    public void onCreate() {
        super.onCreate();
        running = this;
        audio = getSystemService(AudioManager.class);
        wake = getSystemService(PowerManager.class).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "wisp:playback");
        wake.setReferenceCounted(false);

        NotificationChannel channel = new NotificationChannel(
                CHANNEL, getString(R.string.playback_channel), NotificationManager.IMPORTANCE_LOW);
        channel.setShowBadge(false);
        getSystemService(NotificationManager.class).createNotificationChannel(channel);

        session = new MediaSession(this, "Wisp Player");
        session.setCallback(new MediaSession.Callback() {
            @Override public void onPlay() { Bridge.action(Bridge.PLAY); }
            @Override public void onPause() { Bridge.action(Bridge.PAUSE); }
            @Override public void onSkipToNext() { Bridge.action(Bridge.NEXT); }
            @Override public void onSkipToPrevious() { Bridge.action(Bridge.PREV); }
        }, main);

        IntentFilter noisy = new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY);
        if (Build.VERSION.SDK_INT >= 33) {
            registerReceiver(unplugged, noisy, Context.RECEIVER_NOT_EXPORTED);
        } else {
            registerReceiver(unplugged, noisy);
        }
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && intent.hasExtra(EXTRA_WHAT)) {
            // A button of the notification.
            Bridge.action(intent.getIntExtra(EXTRA_WHAT, -1));
        } else if (intent != null) {
            // Started by show(): the notification has to go up within seconds.
            String title = intent.getStringExtra(EXTRA_TITLE);
            String artist = intent.getStringExtra(EXTRA_ARTIST);
            update(title == null ? "" : title, artist == null ? "" : artist, true);
        }
        return START_NOT_STICKY;
    }

    private void update(String title, String artist, boolean playing) {
        session.setMetadata(new MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, artist)
                .build());
        session.setPlaybackState(new PlaybackState.Builder()
                .setActions(PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PAUSE | PlaybackState.ACTION_PLAY_PAUSE
                        | PlaybackState.ACTION_SKIP_TO_NEXT | PlaybackState.ACTION_SKIP_TO_PREVIOUS)
                .setState(playing ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED,
                        PlaybackState.PLAYBACK_POSITION_UNKNOWN, 1f)
                .build());
        session.setActive(true);

        Notification notification = notification(title, artist, playing);
        if (playing) {
            if (!takeFocus()) {
                Bridge.action(Bridge.PAUSE); // a call has the audio
            }
            if (!wake.isHeld()) {
                wake.acquire();
            }
            try {
                if (Build.VERSION.SDK_INT >= 29) {
                    startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
                } else {
                    startForeground(NOTIFICATION, notification);
                }
                foreground = true;
                return;
            } catch (RuntimeException e) {
                // Android 12+ may refuse to go to the foreground from the background; the music plays on.
            }
        } else {
            if (wake.isHeld()) {
                wake.release();
            }
            dropFocus();
            if (foreground) {
                // Paused: the notification stays but can be swiped away, and the process may rest.
                stopForeground(STOP_FOREGROUND_DETACH);
                foreground = false;
            }
        }
        getSystemService(NotificationManager.class).notify(NOTIFICATION, notification);
    }

    private Notification notification(String title, String artist, boolean playing) {
        PendingIntent open = PendingIntent.getActivity(this, 0,
                new Intent(this, MainActivity.class).setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_IMMUTABLE);
        return new Notification.Builder(this, CHANNEL)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle(title)
                .setContentText(artist)
                .setContentIntent(open)
                .setVisibility(Notification.VISIBILITY_PUBLIC)
                .setOnlyAlertOnce(true)
                .setShowWhen(false)
                .setOngoing(playing)
                .addAction(button(R.drawable.ic_prev, R.string.previous, Bridge.PREV))
                .addAction(playing
                        ? button(R.drawable.ic_pause, R.string.pause, Bridge.PAUSE)
                        : button(R.drawable.ic_play, R.string.play, Bridge.PLAY))
                .addAction(button(R.drawable.ic_next, R.string.next, Bridge.NEXT))
                .setStyle(new Notification.MediaStyle()
                        .setMediaSession(session.getSessionToken())
                        .setShowActionsInCompactView(0, 1, 2))
                .build();
    }

    private Notification.Action button(int icon, int label, int what) {
        PendingIntent intent = PendingIntent.getService(this, what,
                new Intent(this, PlaybackService.class).putExtra(EXTRA_WHAT, what),
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        return new Notification.Action.Builder(Icon.createWithResource(this, icon), getString(label), intent).build();
    }

    /** Native audio is not tied to Android's audio focus, so ask for it here, and pause when it goes. */
    private boolean takeFocus() {
        if (focus != null) {
            return true;
        }
        focus = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                .setAudioAttributes(new AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build())
                .setOnAudioFocusChangeListener(change -> {
                    if (change == AudioManager.AUDIOFOCUS_LOSS || change == AudioManager.AUDIOFOCUS_LOSS_TRANSIENT) {
                        Bridge.action(Bridge.PAUSE);
                    }
                }, main)
                .build();
        if (audio.requestAudioFocus(focus) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
            return true;
        }
        focus = null;
        return false;
    }

    private void dropFocus() {
        if (focus != null) {
            audio.abandonAudioFocusRequest(focus);
            focus = null;
        }
    }

    /** The app was swiped away: the player went with it, so the notification goes too. */
    @Override
    public void onTaskRemoved(Intent rootIntent) {
        stopSelf();
    }

    @Override
    public void onDestroy() {
        running = null;
        unregisterReceiver(unplugged);
        dropFocus();
        if (wake.isHeld()) {
            wake.release();
        }
        session.release();
        stopForeground(STOP_FOREGROUND_REMOVE);
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
