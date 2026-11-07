"""Converts the SwinIR real-world super-resolution weights (.pth) into the
upscaling models used by Apsophos.

Only Python and NumPy are needed (no PyTorch, no onnx package): the
checkpoint is read by `pth_weights.py`, the network is built by
`swinir_graph.py` and written by `onnxproto.py`, all in this folder.

    python tools/export_upscaler_model.py --weights <file.pth> --output-dir models

The weights come from https://github.com/JingyunLiang/SwinIR/releases/tag/v0.0
(Apache-2.0). Four files are known, each by its SHA-256, and each gives one
model; any other file is refused:

    003_realSR_BSRGAN_DFO_s64w8_SwinIR-M_x2_PSNR.pth     → swinir_real_x2_psnr_<tile>.onnx
    003_realSR_BSRGAN_DFO_s64w8_SwinIR-M_x2_GAN.pth      → swinir_real_x2_gan_<tile>.onnx
    003_realSR_BSRGAN_DFOWMFC_s64w8_SwinIR-L_x4_PSNR.pth → swinir_real_x4_psnr_<tile>.onnx
    003_realSR_BSRGAN_DFOWMFC_s64w8_SwinIR-L_x4_GAN.pth  → swinir_real_x4_gan_<tile>.onnx

PSNR models are trained for fidelity, GAN models to invent plausible detail.
The graph has a fixed 1×3×N×N input: by default two files are written, a
large tile for TensorRT for RTX and a small one for DirectML (see
DEFAULT_TILES_PX); --tile writes one size only. Apsophos processes tiles of
this size with an overlap.

--verify needs onnxruntime: the graph must match a direct NumPy forward pass
(`swinir_reference.py`) on a small input, and a faithful model must enlarge a
test image, blurred like a camera's and reduced, better (higher PSNR) than
Catmull-Rom interpolation.
"""
from __future__ import annotations

import argparse
import hashlib
import os
import sys
import time
from pathlib import Path
from typing import Final, NamedTuple

import numpy as np

import pth_weights
import swinir_graph


class Known(NamedTuple):
    file: str
    stem: str
    generative: bool


KNOWN: Final = {
    "c698a3dd7c4859563bbce7bf1051d835839e43ec63a78dbeac0098458f3a2c75":
        Known("003_realSR_BSRGAN_DFO_s64w8_SwinIR-M_x2_PSNR.pth", "swinir_real_x2_psnr", False),
    "f397408977a3e07eb06afb7238d453a12ef35ebab7328a54241f307860dbe342":
        Known("003_realSR_BSRGAN_DFO_s64w8_SwinIR-M_x2_GAN.pth", "swinir_real_x2_gan", True),
    "450be6eac63a59959b55a83df6743444de9f018547f81d16f699b3680d366ad7":
        Known("003_realSR_BSRGAN_DFOWMFC_s64w8_SwinIR-L_x4_PSNR.pth", "swinir_real_x4_psnr", False),
    "99adfa91350a84c99e946c1eb3d8fce34bc28f57d807b09dc8fe40a316328c0a":
        Known("003_realSR_BSRGAN_DFOWMFC_s64w8_SwinIR-L_x4_GAN.pth", "swinir_real_x4_gan", True),
}
# input sides written by default, per scale: the large one for TensorRT for
# RTX; the small one for DirectML, where Windows resets a GPU that works more
# than about 2 s on one submission (a whole tile): on an RTX 3050 laptop a
# 2× tile of 320 px takes 1.5 s and one of 384 px stalls, a 4× tile of 256 px
# 1.6 s and one of 320 px stalls
DEFAULT_TILES_PX: Final = {2: (640, 320), 4: (448, 256)}
# Apsophos discards 64 px along each edge of a tile (measured: with 32 px the
# tiles differ from the whole image by up to 76 levels of 255 in fine
# textures, with 64 px by 10)
MIN_TILE_PX: Final = 192
# side of the reduced test image of --verify
VERIFY_PX: Final = 64
# the graph must match the NumPy forward pass to this (float32 vs float64)
REFERENCE_TOLERANCE: Final = 1e-4
# blur of a camera (lens, sensor, demosaicing), in pixels of the reduced
# image: real-world models expect it and sharpen it away, so on a reduction
# without blur they score below Catmull-Rom (measured: −2.6 dB at 2×, −0.5 dB
# at 4×) and above it with this blur
CAMERA_BLUR_PX: Final = 0.5


# ------------------------------------------------------------ verification

def _catmull_rom_1d(x: np.ndarray, scale: int, axis: int) -> np.ndarray:
    """Catmull-Rom (a = −0.5) enlargement by `scale` along `axis`, pixel
    centres aligned, edges mirrored."""
    n = x.shape[axis]
    pos = (np.arange(n * scale) + 0.5) / scale - 0.5
    base = np.floor(pos).astype(int)
    t = pos - base
    weights = [
        ((-0.5 * t + 1.0) * t - 0.5) * t,
        (1.5 * t - 2.5) * t * t + 1.0,
        ((-1.5 * t + 2.0) * t + 0.5) * t,
        (0.5 * t - 0.5) * t * t,
    ]
    out = 0.0
    for k, w in zip(range(-1, 3), weights):
        idx = np.abs(base + k)
        idx = np.where(idx >= n, 2 * n - 1 - idx, idx)
        shape = [1] * x.ndim
        shape[axis] = -1
        out = out + np.take(x, idx, axis=axis) * w.reshape(shape)
    return out


def catmull_rom(img: np.ndarray, scale: int) -> np.ndarray:
    return _catmull_rom_1d(_catmull_rom_1d(img, scale, 1), scale, 2)


def test_scene(size: int) -> np.ndarray:
    """A photo-like test image (3, size, size) in 0..1: colour gradients, hard
    edges, discs and fine stripes, drawn at 4× and averaged down (anti-aliased)."""
    s = 4 * size
    y, x = (np.mgrid[0:s, 0:s] + 0.5) / s
    img = np.stack([0.25 + 0.5 * x, 0.3 + 0.4 * y, 0.6 - 0.3 * x * y])
    for cx, cy, r, colour in ((0.3, 0.3, 0.15, (0.9, 0.2, 0.1)), (0.7, 0.55, 0.2, (0.1, 0.5, 0.8)),
                              (0.4, 0.8, 0.1, (0.95, 0.9, 0.3))):
        disc = (x - cx) ** 2 + (y - cy) ** 2 < r * r
        img[:, disc] = np.array(colour)[:, None]
    bar = (x > 0.55) & (x < 0.9) & (y > 0.1) & (y < 0.3)
    img[:, bar] = 0.05
    stripes = (y > 0.65) & (x > 0.6)
    img[:, stripes] = (0.5 + 0.4 * np.sin(2 * np.pi * x[stripes] * size / 12))[None]
    return img.reshape(3, size, 4, size, 4).mean(axis=(2, 4)).astype(np.float32)


def gaussian_blur(img: np.ndarray, sigma: float) -> np.ndarray:
    """Separable Gaussian blur of img (3, H, W), edges mirrored."""
    r = int(3 * sigma + 1)
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma) ** 2)
    k /= k.sum()
    for axis in (1, 2):
        pad = [(0, 0)] * 3
        pad[axis] = (r, r)
        padded = np.pad(img, pad, mode="symmetric")
        n = img.shape[axis]
        img = sum(w * np.take(padded, range(i, i + n), axis=axis) for i, w in enumerate(k))
    return img


def psnr(a: np.ndarray, b: np.ndarray, border: int) -> float:
    d = (a - b)[:, border:-border, border:-border]
    return float(10 * np.log10(1.0 / np.mean(d * d)))


def verify(weights: dict, known: Known) -> bool:
    """The checks of --verify; prints what it finds. Skipped (True) without
    onnxruntime."""
    try:
        import onnxruntime as ort  # noqa: PLC0415
    except ImportError:
        print("onnxruntime is not installed: verification skipped.")
        return True
    import swinir_reference  # noqa: PLC0415

    run = lambda model, img: ort.InferenceSession(model, providers=["CPUExecutionProvider"]).run(  # noqa: E731
        None, {"input": img[None]})[0][0]
    ok = True
    img = np.random.default_rng(16).random((3, 24, 24)).astype(np.float32)
    diff = float(np.abs(run(swinir_graph.build(weights, 24), img) - swinir_reference.forward(weights, img)).max())
    print(f"Graph vs NumPy forward pass: largest difference {diff:.1e}")
    ok &= diff < REFERENCE_TOLERANCE
    if known.generative:
        print("Generative model: no PSNR check (its detail is invented on purpose).")
        return ok
    scale = swinir_graph.config(weights).scale
    lr_px = VERIFY_PX
    hr = test_scene(lr_px * scale)
    model = swinir_graph.build(weights, lr_px)
    border = 4 * scale
    better = False
    for name, blur in (("without blur", 0.0), ("with a camera's blur", CAMERA_BLUR_PX)):
        src = gaussian_blur(hr, blur * scale) if blur else hr
        lr = src.reshape(3, lr_px, scale, lr_px, scale).mean(axis=(2, 4)).astype(np.float32)
        net = psnr(run(model, lr), hr, border)
        base = psnr(catmull_rom(lr, scale), hr, border)
        print(f"PSNR, test image reduced {name}: network {net:.2f} dB, Catmull-Rom {base:.2f} dB")
        better = net > base  # the last one, the photo-like case, decides
    return ok and better


# ------------------------------------------------------------ main

def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--weights", required=True, type=Path, help="one of the four SwinIR real-world .pth files")
    ap.add_argument("--output-dir", type=Path, default=Path("models"))
    ap.add_argument("--tile", type=int, help=f"side of the input tile in pixels, a multiple of 8, at least "
                                             f"{MIN_TILE_PX} (default: both {DEFAULT_TILES_PX[2]} for 2×, "
                                             f"both {DEFAULT_TILES_PX[4]} for 4×)")
    ap.add_argument("--verify", action="store_true", help="check the graph (needs onnxruntime)")
    a = ap.parse_args()

    try:
        data = a.weights.read_bytes()
    except OSError as e:
        print(f"Cannot read {a.weights}: {e.strerror}", file=sys.stderr)
        return 2
    digest = hashlib.sha256(data).hexdigest()
    known = KNOWN.get(digest)
    if known is None:
        print(f"{a.weights.name} is not one of the known SwinIR weights (SHA-256 {digest}).", file=sys.stderr)
        return 2
    try:
        weights = pth_weights.load_state_dict(a.weights)
        scale = swinir_graph.config(weights).scale
    except (pth_weights.WeightsError, ValueError) as e:
        print(f"Unusable weights: {e}", file=sys.stderr)
        return 2
    tiles = (a.tile,) if a.tile else DEFAULT_TILES_PX[scale]
    if any(t % swinir_graph.WINDOW or t < MIN_TILE_PX for t in tiles):
        print(f"--tile must be a multiple of {swinir_graph.WINDOW}, at least {MIN_TILE_PX}.", file=sys.stderr)
        return 2
    if a.verify and not verify(weights, known):
        print("Verification failed: no model written.", file=sys.stderr)
        return 1
    for tile in tiles:
        start = time.monotonic()
        model = swinir_graph.build(weights, tile)
        out = a.output_dir / f"{known.stem}_{tile}.onnx"
        # written aside and renamed: a half-written model is never left under the real name
        tmp = out.with_name(out.name + ".tmp")
        try:
            a.output_dir.mkdir(parents=True, exist_ok=True)
            tmp.write_bytes(model)
            os.replace(tmp, out)
        except OSError as e:
            tmp.unlink(missing_ok=True)
            print(f"Cannot write {out}: {e.strerror}", file=sys.stderr)
            return 2
        print(f"Written {out} ({len(model) / 1e6:.1f} MB, {time.monotonic() - start:.1f} s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
