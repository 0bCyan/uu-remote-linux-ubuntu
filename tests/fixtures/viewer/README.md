The `red-{420,444}.{h264,hevc}` fixtures contain six synthetic 32x32 red IDRs,
not desktop captures. Their decoded pixel formats are yuv420p and yuv444p.
They let the CLI decoder test run without requiring encoders at test time.

Generated with FFmpeg (substitute `420` / `444` for `CHROMA`):

```sh
ffmpeg -f lavfi -i color=c=red:s=32x32:r=10 -frames:v 6 -pix_fmt yuvCHROMAp \
  -c:v libx264 -threads 1 -preset ultrafast -tune zerolatency -g 1 \
  -f h264 red-CHROMA.h264
ffmpeg -f lavfi -i color=c=red:s=32x32:r=10 -frames:v 6 -pix_fmt yuvCHROMAp \
  -c:v libx265 -threads 1 -preset ultrafast -tune zerolatency -g 1 \
  -x265-params pools=none:frame-threads=1:log-level=none:info=0 \
  -f hevc red-CHROMA.hevc
```
