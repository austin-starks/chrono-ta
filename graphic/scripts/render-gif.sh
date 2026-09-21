#!/bin/sh
set -eu

mkdir -p out

npx remotion render WindowSemantics out/window-semantics.mp4 --codec=h264 --crf=18

ffmpeg \
  -hide_banner \
  -loglevel error \
  -y \
  -i out/window-semantics.mp4 \
  -filter_complex "[0:v]fps=10,format=rgb24,split[palette_input][gif_input];[palette_input]palettegen=max_colors=128:reserve_transparent=0:stats_mode=full[palette];[gif_input][palette]paletteuse=dither=bayer:bayer_scale=5" \
  -loop 0 \
  out/window-semantics.gif

gif_frames="$(ffprobe -v error -select_streams v:0 -show_entries stream=nb_frames -of csv=p=0 out/window-semantics.gif)"
gif_duration="$(ffprobe -v error -show_entries format=duration -of csv=p=0 out/window-semantics.gif)"
gif_dimensions="$(ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of csv=p=0:s=x out/window-semantics.gif)"

if [ "$gif_dimensions" != "1200x600" ]; then
  echo "GIF verification failed: expected 1200x600, got $gif_dimensions" >&2
  exit 1
fi

if [ "$gif_frames" -lt 70 ]; then
  echo "GIF verification failed: expected at least 70 frames, got $gif_frames" >&2
  exit 1
fi

if ! awk -v duration="$gif_duration" 'BEGIN { exit !(duration >= 7.9 && duration <= 8.1) }'; then
  echo "GIF verification failed: expected about 8 seconds, got $gif_duration" >&2
  exit 1
fi

echo "Verified GIF: ${gif_dimensions}, ${gif_frames} frames, ${gif_duration}s"
