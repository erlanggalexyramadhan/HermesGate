#!/usr/bin/env python3
"""Where does a pair's residual error live, once a per-channel gain/offset has
been taken out? Splits by row band, column band and by how colourful the device
pixel is, which separates a chroma-sampling difference (error grows with
saturation, concentrated on edges) from a geometry difference (error
concentrated in bands) and from a plain range/gain error (uniform part).

usage: resid.py <mirror.png> <ideal.png> <label>
"""
import array
import subprocess
import sys

FFMPEG = r"C:/Users/Lextronaut/AppData/Local/hermes/tools/ffmpeg-9.0.1-win32-x64/bin/ffmpeg"


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
    # assume 412x918 (the mirror client area these captures were taken at)
    w = 412
    h = n // w
    print(f"{label}: {w}x{h}")
    # global per-channel affine
    ab = []
    for c in range(3):
        x, y = ideal[c::3], mir[c::3]
        mx, my = sum(x) / n, sum(y) / n
        sxx = sum((u - mx) ** 2 for u in x)
        sxy = sum((u - mx) * (v - my) for u, v in zip(x, y))
        a = sxy / sxx
        ab.append((a, my - a * mx))
    print("  affine: " + "  ".join(f"{name}:{a:.3f}/{b:+.1f}" for name, (a, b) in zip("RGB", ab)))

    def residual(px_ideal, px_mirror):
        t = 0.0
        m = 0
        for o in range(3):
            d = px_mirror[o] - (ab[o][0] * px_ideal[o] + ab[o][1])
            t += d * d
            m += 1
        return t, m

    bands = 6
    row_t = [0.0] * bands
    row_m = [0] * bands
    col_t = [0.0] * bands
    col_m = [0] * bands
    sat_t = [0.0] * 3
    sat_m = [0] * 3
    overall_t = 0.0
    for y in range(h):
        base = y * w * 3
        rb = y * bands // h
        for x in range(w):
            o = base + x * 3
            p = ideal[o:o + 3]
            q = mir[o:o + 3]
            t, m = residual(p, q)
            row_t[rb] += t
            row_m[rb] += m
            cb = x * bands // w
            col_t[cb] += t
            col_m[cb] += m
            sat = max(p) - min(p)
            b = 0 if sat < 10 else (1 if sat < 40 else 2)
            sat_t[b] += t
            sat_m[b] += m
            overall_t += t
    spread = w * h * 3
    print(f"  residual after affine (all): rmse {(overall_t / spread) ** .5:5.2f}")
    print("  by row band:    " + "  ".join(f"{i}:{(row_t[i] / row_m[i]) ** .5:5.2f}" for i in range(bands)))
    print("  by column band: " + "  ".join(f"{i}:{(col_t[i] / col_m[i]) ** .5:5.2f}" for i in range(bands)))
    print("  by device saturation (<10, 10-40, >40): "
          + "  ".join(f"{(sat_t[i] / sat_m[i]) ** .5:5.2f} (n={sat_m[i] // 3})" for i in range(3)))


if __name__ == "__main__":
    main()
