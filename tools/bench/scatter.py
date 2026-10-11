#!/usr/bin/env python3
"""Per-channel scatter of a mirror capture against the device screenshot it was
taken against: slope, intercept and residual of mirror = a*device + b.

A slope near 1.164 with intercept near -18.6 is the signature of full-range RGB
content being processed as studio range (16-235), i.e. an unwanted range
expansion. A slope near 1.0 with a small residual is a correct match.

usage: scatter.py <mirror.png> <ideal.png> <label>
Reads PNGs through ffmpeg (no numpy/PIL on this host) and streams rgb24 bytes.
"""
import array
import subprocess
import sys

FFMPEG = r"C:/Users/Lextronaut/AppData/Local/hermes/tools/ffmpeg-9.0.1-win32-x64/bin/ffmpeg"


def rgb(path):
    out = subprocess.run(
        [FFMPEG, "-v", "error", "-i", path, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
        capture_output=True, check=True).stdout
    buf = array.array("B")
    buf.frombytes(out)
    return buf


def main():
    mir, ideal = rgb(sys.argv[1]), rgb(sys.argv[2])
    label = sys.argv[3] if len(sys.argv) > 3 else "pair"
    assert len(mir) == len(ideal), f"size mismatch: {len(mir)} vs {len(ideal)}"
    n = len(mir) // 3
    print(f"{label}: {n} pixels")
    worst = 0.0
    for c, name in enumerate("RGB"):
        x = ideal[c::3]
        y = mir[c::3]
        sx, sy = sum(x), sum(y)
        mx, my = sx / n, sy / n
        sxx = sum(v * v for v in x)
        sxy = sum(u * v for u, v in zip(x, y))
        a = (sxy - n * mx * my) / (sxx - n * mx * mx)
        b = my - a * mx
        err = sum((u - v) ** 2 for u, v in zip(x, y))
        rmse = (err / n) ** 0.5
        resid = (max(0.0, err - a * a * (sxx - n * mx * mx)) / n) ** 0.5
        print(f"  {name}: mirror = {a:.4f}*device {b:+.2f}   rmse {rmse:5.2f}   resid-after-line {resid:5.2f}"
              f"   device mean {mx:6.1f} mirror mean {my:6.1f}")
        worst = max(worst, rmse)
    # what a pure 16-235 -> 0-255 expansion of the device would score
    for a, b in ((255 / 219, -16 * 255 / 219),):
        err = 0
        m = 0
        for c in range(3):
            x = ideal[c::3]
            y = mir[c::3]
            for u, v in zip(x, y):
                p = a * u + b
                if p > 255:
                    p = 255
                elif p < 0:
                    p = 0
                d = v - p
                err += d * d
            m += n
        print(f"  hypothesis 'range expansion (16-235 -> 0-255)': rmse {((err / m) ** 0.5):5.2f}"
              f"  -> psnr {10 * __import__('math').log10(255 ** 2 / (err / m)):5.2f} dB")


if __name__ == "__main__":
    main()
