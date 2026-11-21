# Test media

Small real files for the tests. Each carries one second of a half-scale 262 Hz sine; the video files
also have a solid-colour picture track, which comes first, so that the tests prove the picture is
ignored.

| File | What it is |
| --- | --- |
| `sine-262hz-with-video.mp4` | MP4: H.264 picture, then AAC-LC stereo at 44.1 kHz. Encoded by Windows' own encoders. |
| `sine-262hz-opus.webm` | WebM: Opus stereo at 48 kHz, with a duration and an index, as yt-dlp and ffmpeg write them. |
| `sine-262hz-opus-recorder.webm` | The same audio laid out like a browser's recording: no duration, no index, sizes unknown. |
| `sine-262hz-vorbis-video.mkv` | Matroska: H.264 picture, then Vorbis. Track-level tags: title "Fixture Song", artist "Fixture Artist". |
| `sine-262hz-aac-video.mkv` | Matroska: the MP4's H.264 and AAC, repackaged. |

## Making them again

The MP4 comes from `make_mp4.ps1`, which drives the Media Foundation encoders that ship with Windows
(Windows PowerShell 5.1) from a one-second 44.1 kHz stereo 16-bit WAV of the sine:

```powershell
.\make_mp4.ps1 -Wav sine.wav -OutDir out -Name fixture.mp4 -Seconds 1
```

The WebM and Matroska files come from `make_mkv.py`, which writes the EBML itself. Opus and Vorbis are
encoded by libsndfile (through the `soundfile` package); the H.264 and AAC are lifted from the MP4:

```powershell
python -m venv venv
venv\Scripts\pip install soundfile numpy
venv\Scripts\python make_mkv.py out out\fixture.mp4
```
