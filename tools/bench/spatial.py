#!/usr/bin/env python3
"""Characterise a pair's residual: is it spatial (blur / shift) or a mapping
error? Reports, for the mirror and the device shot, edge energy (mean absolute
Laplacian on luma) and, after removing a per-channel gain/offset, the residual
under the device as-is, under a +-1 px shift, and under a 3x3 box blur.

usage: spatial.py <mirror.png> <ideal.png> <label>
"""
import array
import subprocess
import sys

FFMPEG = r"C:/Users/Lextronaut/AppData/Local/hermes/tools/ffmpeg-9.0.1-win32-x64/bin/ffmpeg"
W = 412


def rgb(path):
    out = subprocess.run(
        [FFMPEG, "-v", "error", "-i", path, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
        capture_output=True, check=True).stdout
    b = array.array("B")
    b.frombytes(out)
    return b


def luma(buf):
    return [(buf[i * 3] + 2 * buf[i * 3 + 1] + buf[i * 3 + 2]) / 4 for i in range(len(buf) // 3)]


def edge_energy(lum, w, h):
    t = 0.0
    n = 0
    for y in range(1, h - 1):
        r = y * w
        for x in range(1, w - 1):
            i = r + x
            t += abs(4 * lum[i] - lum[i - 1] - lum[i + 1] - lum[i - w] - lum[i + w])
            n += 1
    return t / n


def main():
    mir, ideal = rgb(sys.argv[1]), rgb(sys.argv[2])
    label = sys.argv[3] if len(sys.argv) > 3 else "pair"
    n = len(mir) // 3
    h = n // W
    for name, buf in (("device", ideal), ("mirror", mir)):
        print(f"  {name} edge energy (mean |laplacian| on luma): {edge_energy(luma(buf), W, h):6.2f}")
    ab = []
    for c in range(3):
        x, y = ideal[c::3], mir[c::3]
        mx, my = sum(x) / n, sum(y) / n
        sxx = sum((u - mx) ** 2 for u in x)
        sxy = sum((u - mx) * (v - my) for u, v in zip(x, y))
        a = sxy / sxx
        ab.append((a, my - a * mx))

    def rmse(pred_fn):
        t = 0.0
        m = 0
        for y in range(h):
            for x in range(W):
                o = (y * W + x) * 3
                for c in range(3):
                    p = pred_fn(x, y, c)
                    d = mir[o + c] - p
                    t += d * d
                    m += 1
        return (t / m) ** 0.5

    def affine(x, y, c):
        return ab[c][0] * ideal[(y * W + x) * 3 + c] + ab[c][1]

    def shifted(x, y, c, dx, dy):
        xx, yy = min(max(x + dx, 0), W - 1), min(max(y + dy, 0), h - 1)
        return ab[c][0] * ideal[(yy * W + xx) * 3 + c] + ab[c][1]

    def blurred(x, y, c):
        t = 0.0
        k = 0
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                xx, yy = min(max(x + dx, 0), W - 1), min(max(y + dy, 0), h - 1)
                t += ideal[(yy * W + xx) * 3 + c]
                k += 1
        return ab[c][0] * (t / k) + ab[c][1]

    print(f"{label}: {W}x{h}")
    print(f"  residual after affine (device as-is): {rmse(affine):5.2f}")
    # the sharper image is the reference: if the mirror is blurred, blurring the
    # device should LOWER the residual; if it is shifted, a shift should.
    for dx, dy in ((0, -1), (0, 1), (-1, 0), (1, 0)):
        print(f"  residual with device shifted by ({dx:+d},{dy:+d}): {rmse(lambda x, y, c, dx=dx, dy=dy: shifted(x, y, c, dx, dy)):5.2f}"
              .replace("by (+0,-1)", "by (0,-1)"))
    print(f"  residual with device 3x3 box-blurred: {rmse(blurred):5.2f}")


if __name__ == "__main__":
    main()
