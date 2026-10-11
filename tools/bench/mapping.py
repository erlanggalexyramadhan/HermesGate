#!/usr/bin/env python3
"""Print the actual value mapping of a pair: for each channel, device value
binned in 16s -> mean mirror value in that bin (the transfer curve the pipeline
really applies, which a single affine fit hides). Also the mirror's tint on
device pixels that are grey (max-min < 8), where a chroma error would show.

usage: mapping.py <mirror.png> <ideal.png> <label>
"""
import array
import subprocess
import sys

FFMPEG = r"C:/Users/Lextronaut/AppData/Local/hermes/tools/ffmpeg-9.0.1-win32-x64/bin/ffmpeg"
STEP = 16


def rgb(path):
    out = subprocess.run(
        [FFMPEG, "-v", "error", "-i", path, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
        capture_output=True, check=True).stdout
    b = array.array("B")
    b.frombytes(out)
    return b


def main():
    mir, ideal = rgb(sys.argv[1]), rgb(sys.argv[2])
    label = sys.argv[3] if len(sys.argv) > 3 else "pair"
    n = len(mir) // 3
    print(label)
    for c, name in enumerate("RGB"):
        sums = [0.0] * (256 // STEP)
        counts = [0] * (256 // STEP)
        x, y = ideal[c::3], mir[c::3]
        for u, v in zip(x, y):
            b = u // STEP
            sums[b] += v
            counts[b] += 1
        row = "  ".join(f"{i * STEP:3d}:{sums[i] / counts[i]:6.1f}" for i in range(len(sums)) if counts[i] > 200)
        print(f"  {name}: {row}")
    grey_dev = [0.0, 0.0, 0.0]
    grey_mir = [0.0, 0.0, 0.0]
    m = 0
    for i in range(n):
        o = i * 3
        p = ideal[o:o + 3]
        if max(p) - min(p) < 8:
            for c in range(3):
                grey_dev[c] += p[c]
                grey_mir[c] += mir[o + c]
            m += 1
    if m:
        dev = [v / m for v in grey_dev]
        mir_ = [v / m for v in grey_mir]
        print(f"  grey device pixels (n={m}): device mean {dev[0]:.1f},{dev[1]:.1f},{dev[2]:.1f}"
              f" | mirror mean {mir_[0]:.1f},{mir_[1]:.1f},{mir_[2]:.1f}"
              f" | mirror tint R-G {mir_[0] - mir_[1]:+.2f} G-B {mir_[1] - mir_[2]:+.2f}")


if __name__ == "__main__":
    main()
