This crate tries to hide the ugliness of playing the video (using the shin engine)

It supports only H264 + aac packed in `mp4`.

`mp4` demuxing is done using the `mp4` crate.
`aac` decoding is done using the `symphonia` crate.

`H264` decoding is where it gets nasty. Currently, `shin-video` has support for the following three backends, with different trade-offs:

- `media_foundation` (Windows only) - uses Windows' own H.264 decoder (`CLSID_MSH264DecoderMFT`) on a thread per movie, so no ffmpeg is needed on Windows. On editions without it (the "N" editions lacking the Media Feature Pack) it falls back to `spawn_ffmpeg` by itself.

- `spawn_ffmpeg` - spawns an ffmpeg process, pipes the video to it, and reads the decoded frames from it. It's nice because it doesn't require much code from our side (no linking to `ffmpeg`, you know). It also keeps us away from GPL code. However, not every user has ffmpeg handy, especially on windows. It also doesn't support hardware-accelerated decoding, because ffmpeg is not really smart about it.
- `gstreamer` - this links to `gstreamer`, and uses it to decode the video. It's nice because it supports hardware-accelerated decoding, and it's available on all platforms. However, building (and, especially, cross-compiling) becomes more complicated, as one needs to have all the `gstreamer` dependencies available. It also requires installing `gstreamer` on the user's machine (at least if we are linking dynamically, which makes sense for LGPL).

By default, `shin-video` uses `media_foundation` on Windows and `spawn_ffmpeg` elsewhere. To use `gstreamer` (on every platform), you need to enable the `gstreamer` feature.

For the `shin` crate, use the `gstreamer-video` feature to enable the `gstreamer` backend.