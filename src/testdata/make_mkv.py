"""Makes small WebM / Matroska test files from real encoded audio and video.

Opus and Vorbis come from libsndfile (through the soundfile package), H.264 and AAC are lifted from
an MP4 that Windows' own encoders made. Everything is muxed by the tiny EBML writer below.
"""
import math
import os
import struct
import sys

import numpy as np
import soundfile as sf

# ---------------------------------------------------------------------------------------------
# EBML writing
# ---------------------------------------------------------------------------------------------


def vint_size(n, width=None):
    """An EBML data size. `width` forces a length (1..8 bytes); None picks the shortest."""
    if width is None:
        width = 1
        while n >= (1 << (7 * width)) - 1:
            width += 1
    return ((1 << (7 * width)) | n).to_bytes(width, "big") if width < 8 else (0x01 << 56 | n).to_bytes(8, "big")


UNKNOWN_SIZE = bytes([0x01]) + b"\xff" * 7


def el(eid, payload, size_width=None, unknown=False):
    return eid + (UNKNOWN_SIZE if unknown else vint_size(len(payload), size_width)) + payload


def uint(eid, v):
    n = max(1, (v.bit_length() + 7) // 8)
    return el(eid, v.to_bytes(n, "big"))


def flt(eid, v):
    return el(eid, struct.pack(">d", v))


def string(eid, s):
    return el(eid, s.encode("utf-8"))


EBML = b"\x1a\x45\xdf\xa3"
SEGMENT = b"\x18\x53\x80\x67"
SEEKHEAD = b"\x11\x4d\x9b\x74"
SEEK = b"\x4d\xbb"
INFO = b"\x15\x49\xa9\x66"
TRACKS = b"\x16\x54\xae\x6b"
TAGS = b"\x12\x54\xc3\x67"
CLUSTER = b"\x1f\x43\xb6\x75"
CUES = b"\x1c\x53\xbb\x6b"


def ebml_header(doctype):
    body = (
        uint(b"\x42\x86", 1) + uint(b"\x42\xf7", 1) + uint(b"\x42\xf2", 4) + uint(b"\x42\xf3", 8)
        + string(b"\x42\x82", doctype) + uint(b"\x42\x87", 4) + uint(b"\x42\x85", 2)
    )
    return el(EBML, body)


def info(duration_ms=None, title=None):
    body = uint(b"\x2a\xd7\xb1", 1_000_000) + string(b"\x4d\x80", "wisp-test") + string(b"\x57\x41", "wisp-test")
    if duration_ms is not None:
        body += flt(b"\x44\x89", float(duration_ms))
    if title:
        body += string(b"\x7b\xa9", title)
    return el(INFO, body)


class Track:
    def __init__(self, number, kind, codec_id, private=None, rate=None, channels=None, size=None, delay_ns=None, preroll_ns=None):
        self.number, self.kind, self.codec_id, self.private = number, kind, codec_id, private
        self.rate, self.channels, self.size = rate, channels, size
        self.delay_ns, self.preroll_ns = delay_ns, preroll_ns

    def entry(self):
        body = uint(b"\xd7", self.number) + uint(b"\x73\xc5", 1000 + self.number) + uint(b"\x83", self.kind)
        body += uint(b"\x9c", 0) + string(b"\x86", self.codec_id)
        if self.delay_ns is not None:
            body += uint(b"\x56\xaa", self.delay_ns)
        if self.preroll_ns is not None:
            body += uint(b"\x56\xbb", self.preroll_ns)
        if self.private is not None:
            body += el(b"\x63\xa2", self.private)
        if self.kind == 2:
            body += el(b"\xe1", flt(b"\xb5", float(self.rate)) + uint(b"\x9f", self.channels))
        else:
            w, h = self.size
            body += el(b"\xe0", uint(b"\xb0", w) + uint(b"\xba", h))
        return el(b"\xae", body)


def tracks_element(tracks):
    return el(TRACKS, b"".join(t.entry() for t in tracks))


def tags_element(**tags):
    simple = b"".join(el(b"\x67\xc8", string(b"\x45\xa3", k.upper()) + string(b"\x44\x87", v)) for k, v in tags.items())
    # Track-level tags, the way MKVToolNix writes them: target value 30 and the name "TRACK".
    targets = el(b"\x63\xc0", uint(b"\x68\xca", 30) + string(b"\x63\xca", "TRACK"))
    return el(TAGS, el(b"\x73\x73", targets + simple))


def simple_block(track_number, rel_ms, data, key=True):
    return el(b"\xa3", bytes([0x80 | track_number]) + struct.pack(">hB", rel_ms, 0x80 if key else 0) + data)


def mux(path, doctype, tracks, frames, title=None, tags=None, cluster_ms=480, cues=True, duration_ms=None, unknown_sizes=False):
    """frames: list of (track_number, timestamp_ms, data, key) sorted by time."""
    parts_after_head = [info(duration_ms, title), tracks_element(tracks)]
    if tags:
        parts_after_head.append(tags_element(**tags))
    # Clusters
    clusters, cue_points = [], []
    i = 0
    while i < len(frames):
        start = (frames[i][1] // cluster_ms) * cluster_ms
        body = uint(b"\xe7", start)
        first_audio = None
        while i < len(frames) and frames[i][1] < start + cluster_ms:
            tn, ts, data, key = frames[i]
            if first_audio is None and any(t.number == tn and t.kind == 2 for t in tracks):
                first_audio = ts
            body += simple_block(tn, ts - start, data, key)
            i += 1
        clusters.append((start, el(CLUSTER, body, size_width=8, unknown=unknown_sizes)))
    # Layout: SeekHead (fixed size), then the rest; positions are relative to the segment data.
    def seek_entry(eid, pos):
        return el(SEEK, el(b"\x53\xab", eid) + el(b"\x53\xac", pos.to_bytes(4, "big")))

    head_len = len(el(SEEKHEAD, seek_entry(INFO, 0) + seek_entry(TRACKS, 0) + seek_entry(CUES, 0)))
    pos = head_len
    offsets = {}
    for name, blob in zip(("info", "tracks"), parts_after_head[:2]):
        offsets[name] = pos
        pos += len(blob)
    for blob in parts_after_head[2:]:
        pos += len(blob)
    cluster_pos = []
    for start, blob in clusters:
        cluster_pos.append((start, pos))
        pos += len(blob)
    offsets["cues"] = pos
    cues_blob = b""
    if cues:
        points = b"".join(
            el(b"\xbb", uint(b"\xb3", start) + el(b"\xb7", uint(b"\xf7", next(t.number for t in tracks if t.kind == 2)) + uint(b"\xf1", cpos)))
            for start, cpos in cluster_pos
        )
        cues_blob = el(CUES, points)
    seekhead = (
        el(SEEKHEAD, seek_entry(INFO, offsets["info"]) + seek_entry(TRACKS, offsets["tracks"]) + seek_entry(CUES, offsets["cues"]))
        if cues
        else b""
    )
    if not cues:
        # no SeekHead and no Cues: shift is zero because the head is absent
        pass
    body = seekhead + b"".join(parts_after_head) + b"".join(b for _, b in clusters) + cues_blob
    if not cues:
        body = b"".join(parts_after_head) + b"".join(b for _, b in clusters)
    seg = el(SEGMENT, body, size_width=8, unknown=unknown_sizes)
    with open(path, "wb") as f:
        f.write(ebml_header(doctype) + seg)


# ---------------------------------------------------------------------------------------------
# Ogg (Opus, Vorbis) reading
# ---------------------------------------------------------------------------------------------


def ogg_packets(path):
    """Returns [(packet, granule_of_its_page or None)] in order."""
    data = open(path, "rb").read()
    pos, out, cur, pending = 0, [], b"", []
    while pos < len(data):
        assert data[pos:pos + 4] == b"OggS", "not an Ogg page"
        granule = struct.unpack("<q", data[pos + 6:pos + 14])[0]
        nseg = data[pos + 26]
        table = data[pos + 27:pos + 27 + nseg]
        body = data[pos + 27 + nseg:pos + 27 + nseg + sum(table)]
        pos += 27 + nseg + sum(table)
        off = 0
        page_packets = []
        for seg in table:
            cur += body[off:off + seg]
            off += seg
            if seg < 255:
                page_packets.append(cur)
                cur = b""
        out.extend((p, granule if i == len(page_packets) - 1 else None) for i, p in enumerate(page_packets))
    return out


def opus_frame_samples(packet):
    """Samples (at 48 kHz) in one Opus packet, from its TOC byte."""
    toc = packet[0]
    config, code = toc >> 3, toc & 3
    if config < 12:
        size = [480, 960, 1920, 2880][config % 4]
    elif config < 16:
        size = [480, 960][config % 2]
    else:
        size = [120, 240, 480, 960][config % 4]
    frames = 1 if code == 0 else 2 if code in (1, 2) else (packet[1] & 0x3F)
    return size * frames


def make_opus(wav_rate_hz, seconds, freq, path_ogg, amplitude=0.5):
    n = int(wav_rate_hz * seconds)
    t = np.arange(n) / wav_rate_hz
    x = (amplitude * np.sin(2 * np.pi * freq * t)).astype("float32")
    sf.write(path_ogg, np.stack([x, x], axis=1), wav_rate_hz, format="OGG", subtype="OPUS")


def make_vorbis(rate, seconds, freq, path_ogg, amplitude=0.5):
    n = int(rate * seconds)
    t = np.arange(n) / rate
    x = (amplitude * np.sin(2 * np.pi * freq * t)).astype("float32")
    sf.write(path_ogg, np.stack([x, x], axis=1), rate, format="OGG", subtype="VORBIS")


def opus_frames(path_ogg):
    pk = ogg_packets(path_ogg)
    head = pk[0][0]
    assert head[:8] == b"OpusHead"
    preskip = struct.unpack("<H", head[10:12])[0]
    frames, t48 = [], 0
    for p, _ in pk[2:]:
        frames.append((t48 * 1000 // 48000, p))
        t48 += opus_frame_samples(p)
    return head, preskip, frames, t48 * 1000 // 48000


def vorbis_frames(path_ogg, rate):
    pk = ogg_packets(path_ogg)
    ident, comment, setup = pk[0][0], pk[1][0], pk[2][0]
    assert ident[0] == 1 and setup[0] == 5
    audio = pk[3:]
    # interpolate packet start times between page granule positions
    frames, prev_g, run = [], 0, []
    for p, g in audio:
        run.append(p)
        if g is not None:
            step = (g - prev_g) / max(1, len(run))
            for k, q in enumerate(run):
                frames.append((int((prev_g + k * step) * 1000 / rate), q))
            prev_g, run = g, []
    for k, q in enumerate(run):  # trailing packets without a page end
        frames.append((int((prev_g + k * 1024) * 1000 / rate), q))
    last_ms = int(prev_g * 1000 / rate)
    return ident, comment, setup, frames, last_ms


def xiph_lace(*sizes):
    out = b""
    for s in sizes:
        out += b"\xff" * (s // 255) + bytes([s % 255])
    return out


# ---------------------------------------------------------------------------------------------
# MP4 reading (H.264 and AAC samples from the Windows-made clip)
# ---------------------------------------------------------------------------------------------


def boxes(data, start=0, end=None):
    end = len(data) if end is None else end
    pos = start
    while pos + 8 <= end:
        size, kind = struct.unpack(">I4s", data[pos:pos + 8])
        hdr = 8
        if size == 1:
            size = struct.unpack(">Q", data[pos + 8:pos + 16])[0]
            hdr = 16
        elif size == 0:
            size = end - pos
        yield kind.decode("latin1"), pos + hdr, pos + size
        pos += size


def find(data, path, start=0, end=None):
    cur = (start, len(data) if end is None else end)
    for name in path:
        for kind, s, e in boxes(data, *cur):
            if kind == name:
                cur = (s, e)
                break
        else:
            return None
    return cur


def mp4_tracks(path):
    data = open(path, "rb").read()
    moov = find(data, ["moov"])
    result = {}
    for kind, s, e in boxes(data, *moov):
        if kind != "trak":
            continue
        hdlr = find(data, ["mdia", "hdlr"], s, e)
        handler = data[hdlr[0] + 8:hdlr[0] + 12].decode()
        mdhd = find(data, ["mdia", "mdhd"], s, e)
        timescale = struct.unpack(">I", data[mdhd[0] + 12:mdhd[0] + 16])[0]
        stbl = find(data, ["mdia", "minf", "stbl"], s, e)
        sizes, chunks, stsc, stts, stsd = [], [], [], [], None
        for k2, s2, e2 in boxes(data, *stbl):
            if k2 == "stsz":
                fixed, count = struct.unpack(">II", data[s2 + 4:s2 + 12])
                sizes = [fixed] * count if fixed else list(struct.unpack(f">{count}I", data[s2 + 12:s2 + 12 + 4 * count]))
            elif k2 == "stco":
                count = struct.unpack(">I", data[s2 + 4:s2 + 8])[0]
                chunks = list(struct.unpack(f">{count}I", data[s2 + 8:s2 + 8 + 4 * count]))
            elif k2 == "co64":
                count = struct.unpack(">I", data[s2 + 4:s2 + 8])[0]
                chunks = list(struct.unpack(f">{count}Q", data[s2 + 8:s2 + 8 + 8 * count]))
            elif k2 == "stsc":
                count = struct.unpack(">I", data[s2 + 4:s2 + 8])[0]
                stsc = [struct.unpack(">III", data[s2 + 8 + 12 * i:s2 + 20 + 12 * i]) for i in range(count)]
            elif k2 == "stts":
                count = struct.unpack(">I", data[s2 + 4:s2 + 8])[0]
                stts = [struct.unpack(">II", data[s2 + 8 + 8 * i:s2 + 16 + 8 * i]) for i in range(count)]
            elif k2 == "stsd":
                stsd = (s2, e2)
        # samples per chunk
        per_chunk = []
        for ci in range(len(chunks)):
            n = 0
            for first, spc, _ in stsc:
                if first <= ci + 1:
                    n = spc
            per_chunk.append(n)
        samples, si = [], 0
        for off, n in zip(chunks, per_chunk):
            for _ in range(n):
                samples.append((off, sizes[si]))
                off += sizes[si]
                si += 1
        deltas = [d for count, d in stts for _ in range(count)]
        dts, t = [], 0
        for d in deltas:
            dts.append(t)
            t += d
        result[handler] = dict(data=data, timescale=timescale, samples=samples, dts=dts, stsd=stsd)
    return result


def sample_entry_child(data, stsd, child, header_len):
    s, e = stsd
    entry_start = s + 8  # skip version/flags and entry count
    esize, _ = struct.unpack(">I4s", data[entry_start:entry_start + 8])
    inner = entry_start + 8 + header_len
    for kind, s2, e2 in boxes(data, inner, entry_start + esize):
        if kind == child:
            return data[s2:e2], data, entry_start
    raise SystemExit(f"{child} not found")


def avcc_and_size(track):
    payload, data, entry_start = sample_entry_child(track["data"], track["stsd"], "avcC", 78)
    w, h = struct.unpack(">HH", data[entry_start + 8 + 24:entry_start + 8 + 28])
    return payload, (w, h)


def aac_asc(track):
    payload, _, _ = sample_entry_child(track["data"], track["stsd"], "esds", 28)
    pos = 4  # version/flags
    asc = None
    while pos < len(payload):
        tag = payload[pos]
        pos += 1
        length = 0
        for _ in range(4):
            b = payload[pos]
            pos += 1
            length = (length << 7) | (b & 0x7F)
            if not b & 0x80:
                break
        if tag == 3:
            pos += 3  # ES_ID + flags
        elif tag == 4:
            pos += 13  # object type, stream type, buffer size, max/avg bitrate
        elif tag == 5:
            asc = payload[pos:pos + length]
            break
        else:
            pos += length
    assert asc, "no AudioSpecificConfig"
    return asc


def mp4_frames(track):
    ts = track["timescale"]
    return [(d * 1000 // ts, track["data"][o:o + n]) for (o, n), d in zip(track["samples"], track["dts"])]


# ---------------------------------------------------------------------------------------------
# The test files
# ---------------------------------------------------------------------------------------------


def main(out_dir, mp4_path, long_seconds=0):
    os.makedirs(out_dir, exist_ok=True)
    tmp = os.path.join(out_dir, "_tmp")
    os.makedirs(tmp, exist_ok=True)

    # 1. WebM, Opus only, with an index (what yt-dlp / ffmpeg write)
    make_opus(48000, 1.0, 262.0, os.path.join(tmp, "tone.opus"))
    head, preskip, of, end_ms = opus_frames(os.path.join(tmp, "tone.opus"))
    opus_track = Track(1, 2, "A_OPUS", private=head, rate=48000, channels=2, delay_ns=preskip * 1_000_000_000 // 48000, preroll_ns=80_000_000)
    frames = [(1, t, p, True) for t, p in of]
    mux(os.path.join(out_dir, "sine-262hz-opus.webm"), "webm", [opus_track], frames, duration_ms=end_ms)
    print("opus.webm", len(of), "packets", end_ms, "ms")

    # 2. WebM the way a browser's MediaRecorder writes it: unknown sizes, no duration, no index
    mux(os.path.join(out_dir, "recorder.webm"), "webm", [opus_track], frames, cues=False, unknown_sizes=True)

    # 3/4. Matroska with the H.264 picture first, and Vorbis or AAC sound
    mp4 = mp4_tracks(mp4_path)
    avcc, size = avcc_and_size(mp4["vide"])
    video = Track(1, 1, "V_MPEG4/ISO/AVC", private=avcc, size=size)
    vframes = [(1, t, d, i == 0) for i, (t, d) in enumerate(mp4_frames(mp4["vide"]))]

    make_vorbis(44100, 1.0, 262.0, os.path.join(tmp, "tone.ogg"))
    ident, comment, setup, vf, last = vorbis_frames(os.path.join(tmp, "tone.ogg"), 44100)
    private = bytes([2]) + xiph_lace(len(ident), len(comment)) + ident + comment + setup
    vorbis = Track(2, 2, "A_VORBIS", private=private, rate=44100, channels=2)
    allf = sorted(vframes + [(2, t, p, True) for t, p in vf], key=lambda f: (f[1], f[0]))
    mux(os.path.join(out_dir, "sine-262hz-vorbis-video.mkv"), "matroska", [video, vorbis], allf,
        tags={"title": "Fixture Song", "artist": "Fixture Artist"}, duration_ms=last)
    print("vorbis mkv", len(vf), "packets", last, "ms")

    asc = aac_asc(mp4["soun"])
    aac = Track(2, 2, "A_AAC", private=asc, rate=44100, channels=2)
    af = [(2, t, d, True) for t, d in mp4_frames(mp4["soun"])]
    allf = sorted(vframes + af, key=lambda f: (f[1], f[0]))
    mux(os.path.join(out_dir, "sine-262hz-aac-video.mkv"), "matroska", [video, aac], allf, duration_ms=1000)
    print("aac mkv", len(af), "packets, asc", asc.hex())

    # 5. A longer WebM with a (dummy) VP9 picture track, for the real-window test
    if long_seconds:
        make_opus(48000, float(long_seconds), 441.0, os.path.join(tmp, "long.opus"))
        head, preskip, of, end_ms = opus_frames(os.path.join(tmp, "long.opus"))
        track = Track(2, 2, "A_OPUS", private=head, rate=48000, channels=2, delay_ns=preskip * 1_000_000_000 // 48000, preroll_ns=80_000_000)
        vp9 = Track(1, 1, "V_VP9", size=(1280, 720))
        rng = np.random.default_rng(1)
        pic = [(1, t, rng.bytes(12_000), t % 1000 == 0) for t in range(0, end_ms, 33)]
        allf = sorted(pic + [(2, t, p, True) for t, p in of], key=lambda f: (f[1], f[0]))
        mux(os.path.join(out_dir, "Test Artist - Long WebM (Official Video).webm"), "webm", [vp9, track], allf, cluster_ms=5000, duration_ms=end_ms)
        print("long webm", end_ms, "ms")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 0)
