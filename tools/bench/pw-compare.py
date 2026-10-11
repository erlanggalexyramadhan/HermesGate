#!/usr/bin/env python3
"""Compare a mirror-window capture against the device's own screencap.

usage: compare.py <mirror.png> <device.png> <width> <height>
Both images are scaled to the mirror's size with lanczos (the same method the
parity runs used) and compared with the rawvideo method: mean absolute
difference, RMSE, PSNR, worst pixel and the number of rows whose per-row RMSE
sits above 24 (a title bar, frame or letterbox shows up as such a band).
"""
import subprocess
import sys

FF = "C:/Users/Lextronaut/AppData/Local/hermes/tools/ffmpeg-9.0.1-win32-x64/bin/ffmpeg"


def raw(path, width, height):
    out = subprocess.run(
        [
            FF, "-v", "error", "-i", path,
            "-vf", f"scale={width}:{height}:flags=lanczos",
            "-f", "rawvideo", "-pix_fmt", "rgb24", "-",
        ],
        capture_output=True,
    )
    data = out.stdout
    expected = width * height * 3
    if len(data) != expected:
        print(f"  {path}: {len(data)} bytes, expected {expected} ({out.stderr.decode()[:120]})")
    return data


mirror_path, device_path, w, h = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
a = raw(mirror_path, w, h)
b = raw(device_path, w, h)
if len(a) != len(b):
    sys.exit("size mismatch")

total = 0
worst = 0
bands = 0
rows = []
sq = 0
for y in range(h):
    row = 0
    ss = 0
    for x in range(w):
        index = (y * w + x) * 3
        for c in range(3):
            d = a[index + c] - b[index + c]
            row += abs(d)
            ss += d * d
            worst = max(worst, abs(d))
    row_mean = row / (w * 3)
    rmse = (ss / (w * 3)) ** 0.5
    total += row_mean
    sq += ss
    rows.append((y, row_mean, rmse))
    if rmse > 24:
        bands += 1

mae = total / h
rmse = (sq / (w * h * 3)) ** 0.5
psnr = 99.0 if rmse == 0 else 20 * __import__("math").log10(255 / rmse)
mean_a = sum(a[::397]) / len(a[::397])
mean_b = sum(b[::397]) / len(b[::397])
dark_a = sum(1 for v in a[::397] if v < 2) / len(a[::397]) * 100

print(f"  mirror mean {mean_a:6.2f}   device mean {mean_b:6.2f}   mirror near-black {dark_a:5.1f}%")
print(f"  MAE {mae:6.3f}/255   RMSE {rmse:6.3f}   PSNR {psnr:5.1f} dB   worst pixel {worst}   rows above 24: {bands}")
profile = [(y, m) for y, m, _ in rows if y % max(1, h // 12) == 0]
print("  mirror row profile:", " ".join(f"{y}:{m:.1f}" for y, m in profile[:12]))
