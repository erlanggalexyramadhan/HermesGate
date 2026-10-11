#!/usr/bin/env python3
"""Is the mirror-versus-device difference a translation?

usage: shift.py <mirror.png> <device.png> <width> <height>
Both are reduced to grayscale at the mirror's size, then the mean absolute
difference is computed for a grid of integer offsets. A sharp minimum away from
(0,0) means the pipeline's picture is offset (a scaling/crop finding); a flat
minimum at (0,0) means the difference is content, not geometry.
"""
import subprocess
import sys

FF = "C:/Users/Lextronaut/AppData/Local/hermes/tools/ffmpeg-9.0.1-win32-x64/bin/ffmpeg"


def gray(path, width, height):
    out = subprocess.run(
        [FF, "-v", "error", "-i", path, "-vf", f"scale={width}:{height}:flags=lanczos",
         "-f", "rawvideo", "-pix_fmt", "gray", "-"],
        capture_output=True,
    ).stdout
    return out[: width * height]


mirror, device, w, h = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
a = gray(mirror, w, h)
b = gray(device, w, h)
if len(a) != w * h or len(b) != w * h:
    sys.exit(f"short read: mirror {len(a)} device {len(b)} expected {w*h}")

# Vertical offsets only, and only rows that exist in both images.
span = 8
print("  dy :", " ".join(f"{dy:+d}" for dy in range(-span, span + 1)))
for dx in (-4, -2, 0, 2, 4):
    line = []
    for dy in range(-span, span + 1):
        total = 0
        n = 0
        for y in range(max(0, -dy), min(h, h - dy)):
            base_a = y * w + max(0, -dx)
            base_b = (y + dy) * w + max(0, dx)
            count = w - abs(dx)
            row = a[base_a:base_a + count]
            other = b[base_b:base_b + count]
            total += sum(abs(p - q) for p, q in zip(row, other))
            n += count
        line.append(f"{total / n:5.2f}")
    print(f"  dx={dx:+d}:", " ".join(line))
