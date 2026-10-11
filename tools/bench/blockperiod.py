#!/usr/bin/env python3
"""Is a pair's residual block-structured (a 2x2 chroma cell repeated, as
nearest-neighbour chroma upsampling produces) or independent per pixel?

ratio = mean within-2x2-block variance / total variance, per channel.
     ~1.0 -> error is independent per pixel (not chroma blockiness)
     ~0.0 -> error is constant inside each 2x2 block

usage: blockperiod.py <mirror.png> <ideal.png> <label>
"""
import array, subprocess, sys
FF = r"C:/Users/Lextronaut/AppData/Local/hermes/tools/ffmpeg-9.0.1-win32-x64/bin/ffmpeg"
W = 412

def rgb(p):
    out = subprocess.run([FF, "-v", "error", "-i", p, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                         capture_output=True, check=True).stdout
    b = array.array("B"); b.frombytes(out); return b

mir, ideal = rgb(sys.argv[1]), rgb(sys.argv[2])
n = len(mir) // 3
h = n // W
print(sys.argv[3] if len(sys.argv) > 3 else "pair")
for c, name in enumerate("RGB"):
    x, y = ideal[c::3], mir[c::3]
    mx, my = sum(x) / n, sum(y) / n
    sxx = sum((u - mx) ** 2 for u in x)
    sxy = sum((u - mx) * (v - my) for u, v in zip(x, y))
    a = sxy / sxx
    b = my - a * mx
    d = [y[i] - (a * x[i] + b) for i in range(n)]
    mu = sum(d) / n
    tot = sum((v - mu) ** 2 for v in d) / n
    within = 0.0
    blocks = 0
    for by in range(0, h - 1, 2):
        row = by * W
        for bx in range(0, W - 1, 2):
            i = row + bx
            q = (d[i], d[i + 1], d[i + W], d[i + W + 1])
            m = sum(q) / 4
            within += sum((v - m) ** 2 for v in q) / 4
            blocks += 1
    within /= blocks
    print(f"  {name}: within-2x2/total = {within / tot:5.3f}  (rmse {(tot) ** .5:5.2f})")
