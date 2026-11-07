This crate tries to hide the ugliness of playing the video (using the shin engine)

It supports only H264 + aac packed in `mp4`.

`mp4` demuxing is done using the `mp4` crate.
`aac` decoding is done using the `symphonia` crate.

`H264` decoding uses each system's own decoder, then ffmpeg, then none:

- **Windows:** `media_foundation`, Windows' own H.264 decoder (`CLSID_MSH264DecoderMFT`) on a thread per movie.
- **Linux:** `gstreamer`, the system's media framework (linked dynamically; LGPL). It needs an H.264 plugin, which
  most distributions install apart from GStreamer itself (gst-libav, or openh264 on Fedora); `can_decode_h264`
  checks the registry before a movie starts.
- **macOS:** VideoToolbox (to come).
- **Fallback:** `spawn_ffmpeg` pipes the video to an `ffmpeg` on the PATH and reads raw frames back (no linking, so
  no GPL code here; no hardware decoding).
- **None:** `none` ends every movie at once, so the game goes on without them, and says once in the log what to
  install.
