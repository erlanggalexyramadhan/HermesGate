#!/usr/bin/env python3
"""Decide whether a mirror/device pair differs *only* by a studio<->full range
expansion (the signature of the video processor being told the wrong RGB range)
or additionally by something spatial (blur, chroma upsampling, compression).

For each channel it reports:
  direct rmse            - the raw disagreement (what PSNR reflects)
  rmse vs clamp(1.1638x-18.62)  - the exact full-range-RGB -> studio expansion,
                           clamped at 0/255 the way a driver would clamp
  rmse vs the best affine        - the best possible per-channel gain/offset
  share of device pixels above 235 - how much of the frame is clipped by that
                           expansion (clipping is why a fitted slope comes out
                           below 1.164)

If the clamped-expansion rmse lands near the control level, the whole
difference is that expansion: nothing spatial is left.

usage: expansion_test.py <mirror.png> <ideal.png> <label>
"""
import array
import math
import subprocess
import sys

FFMPEG = r"C:/Users/Lextronaut/AppData/Local/hermes/tools/ffmpeg-9.0.1-win32-x64/bin/ffmpeg"
A = 255.0 / 219.0
B = -16 * 255.0 / 219.0


def rgb(path):
    out = subprocess.run(
        [FFMPEG, "-v", "error", "-i", path, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
        capture_output=True, check=True).stdout
    buf = array.array("B")
    buf.frombytes(out)
    return buf


def report(mirror, ideal, label):
    print(f"{label}")
    total_direct = total_exp = total_aff = 0
    n_total = 0
    for c, name in enumerate("RGB"):
        x, y = ideal[c::3], mirror[c::3]
        n = len(x)
        direct = sum((u - v) ** 2 for u, v in zip(x, y)) / n
        exp = 0
        clipped = 0
        for u, v in zip(x, y):
            p = A * u + B
            if p > 255.0:
                p = 255.0
                clipped += 1
            elif p < 0.0:
                p = 0.0
            exp += (v - p) ** 2
        exp /= n
        mx = sum(x) / n
        my = sum(y) / n
        sxx = sum((u - mx) ** 2 for u in x)
        sxy = sum((u - mx) * (v - my) for u, v in zip(x, y))
        a = sxy / sxx
        b = my - a * mx
        aff = sum((v - (a * u + b)) ** 2 for u, v in zip(x, y)) / n
        print(f"  {name}: direct rmse {direct ** .5:5.2f} | expansion-clamped rmse {exp ** .5:5.2f}"
              f" | best-affine rmse {aff ** .5:5.2f} (a={a:.3f} b={b:+.1f})"
              f" | device>{235}: {100 * clipped / n:4.1f}%")
        total_direct += direct
        total_exp += exp
        total_aff += aff
        n_total += 1
    for tag, val in (("direct", total_direct), ("expansion-clamped", total_exp), ("best-affine", total_aff)):
        mse = val / n_total
        print(f"  all channels {tag:17s}: rmse {mse ** .5:5.2f}  psnr {10 * math.log10(255 ** 2 / mse):5.2f} dB")


if __name__ == "__main__":
    report(rgb(sys.argv[1]), rgb(sys.argv[2]), sys.argv[3])
