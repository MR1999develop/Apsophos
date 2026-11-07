"""A direct NumPy forward pass of real-world SwinIR, written the way the
authors' PyTorch code works (window partition, region labels for the masks of
the shifted windows, index into the relative position table), independently
of the ONNX graph: the converter checks the graph against it. In float64 and
slow: meant for inputs of a few windows.
"""
from __future__ import annotations

import math

import numpy as np

from swinir_graph import NUM_FEAT, RGB_MEAN, WINDOW, config

_erf = np.vectorize(math.erf, otypes=[np.float64])


def _conv(x: np.ndarray, w: np.ndarray, b: np.ndarray) -> np.ndarray:
    """2-D convolution of x (C, H, W), stride 1, zero padding keeping the size."""
    c, h, wd = x.shape
    k = w.shape[2]
    pad = k // 2
    xp = np.pad(x, ((0, 0), (pad, pad), (pad, pad)))
    cols = np.empty((c, k, k, h, wd))
    for dy in range(k):
        for dx in range(k):
            cols[:, dy, dx] = xp[:, dy:dy + h, dx:dx + wd]
    return np.tensordot(w.astype(np.float64), cols, axes=([1, 2, 3], [0, 1, 2])) + b[:, None, None]


def _leaky(x: np.ndarray, slope: float) -> np.ndarray:
    return np.where(x >= 0, x, x * slope)


def _layer_norm(x: np.ndarray, gamma: np.ndarray, beta: np.ndarray) -> np.ndarray:
    m = x.mean(-1, keepdims=True)
    v = ((x - m) ** 2).mean(-1, keepdims=True)
    return (x - m) / np.sqrt(v + 1e-5) * gamma + beta


def _partition(x: np.ndarray, w: int) -> np.ndarray:
    """(H, W, C) → (windows, w², C), windows in row order."""
    h, wd, c = x.shape
    return x.reshape(h // w, w, wd // w, w, c).transpose(0, 2, 1, 3, 4).reshape(-1, w * w, c)


def _reverse(win: np.ndarray, w: int, h: int, wd: int) -> np.ndarray:
    c = win.shape[-1]
    return win.reshape(h // w, wd // w, w, w, c).transpose(0, 2, 1, 3, 4).reshape(h, wd, c)


def _mask(h: int, wd: int, w: int, s: int) -> np.ndarray:
    """SwinIR's calculate_mask: −100 between pixels of different regions."""
    img = np.zeros((h, wd, 1))
    label = 0
    for hs in (slice(0, -w), slice(-w, -s), slice(-s, None)):
        for ws in (slice(0, -w), slice(-w, -s), slice(-s, None)):
            img[hs, ws] = label
            label += 1
    win = _partition(img, w)[:, :, 0]
    diff = win[:, None, :] - win[:, :, None]
    return np.where(diff != 0, -100.0, 0.0)


def _relative_index(w: int) -> np.ndarray:
    coords = np.stack(np.meshgrid(np.arange(w), np.arange(w), indexing="ij")).reshape(2, -1)
    rel = (coords[:, :, None] - coords[:, None, :]).transpose(1, 2, 0) + (w - 1)
    return rel[:, :, 0] * (2 * w - 1) + rel[:, :, 1]


def _block(x: np.ndarray, p: dict, prefix: str, heads: int, shift: int) -> np.ndarray:
    h, wd, c = x.shape
    w = WINDOW
    y = _layer_norm(x, p[f"{prefix}.norm1.weight"], p[f"{prefix}.norm1.bias"])
    if shift:
        y = np.roll(y, (-shift, -shift), axis=(0, 1))
    win = _partition(y, w)
    qkv = win @ p[f"{prefix}.attn.qkv.weight"].T + p[f"{prefix}.attn.qkv.bias"]
    n_win, n_pix, _ = qkv.shape
    hd = c // heads
    qkv = qkv.reshape(n_win, n_pix, 3, heads, hd).transpose(2, 0, 3, 1, 4)
    q, k, v = qkv[0] * hd ** -0.5, qkv[1], qkv[2]
    a = q @ k.transpose(0, 1, 3, 2)
    table = p[f"{prefix}.attn.relative_position_bias_table"]
    bias = table[_relative_index(w).reshape(-1)].reshape(n_pix, n_pix, heads).transpose(2, 0, 1)
    a = a + bias[None]
    if shift:
        a = a + _mask(h, wd, w, shift)[:, None]
    a = np.exp(a - a.max(-1, keepdims=True))
    a /= a.sum(-1, keepdims=True)
    o = (a @ v).transpose(0, 2, 1, 3).reshape(n_win, n_pix, c)
    o = o @ p[f"{prefix}.attn.proj.weight"].T + p[f"{prefix}.attn.proj.bias"]
    y = _reverse(o, w, h, wd)
    if shift:
        y = np.roll(y, (shift, shift), axis=(0, 1))
    x = x + y
    m = _layer_norm(x, p[f"{prefix}.norm2.weight"], p[f"{prefix}.norm2.bias"])
    m = m @ p[f"{prefix}.mlp.fc1.weight"].T + p[f"{prefix}.mlp.fc1.bias"]
    m = m * 0.5 * (1.0 + _erf(m / math.sqrt(2.0)))
    m = m @ p[f"{prefix}.mlp.fc2.weight"].T + p[f"{prefix}.mlp.fc2.bias"]
    return x + m


def _conv_block(x: np.ndarray, p: dict, prefix: str, three_conv: bool) -> np.ndarray:
    if not three_conv:
        return _conv(x, p[f"{prefix}.weight"], p[f"{prefix}.bias"])
    y = _leaky(_conv(x, p[f"{prefix}.0.weight"], p[f"{prefix}.0.bias"]), 0.2)
    y = _leaky(_conv(y, p[f"{prefix}.2.weight"], p[f"{prefix}.2.bias"]), 0.2)
    return _conv(y, p[f"{prefix}.4.weight"], p[f"{prefix}.4.bias"])


def forward(p: dict[str, np.ndarray], img: np.ndarray) -> np.ndarray:
    """SwinIR applied to img (3, N, N) in 0..1; returns (3, sN, sN)."""
    c = config(p)
    mean = np.array(RGB_MEAN)[:, None, None]
    first = _conv(img.astype(np.float64) - mean, p["conv_first.weight"], p["conv_first.bias"])
    t = _layer_norm(first.transpose(1, 2, 0), p["patch_embed.norm.weight"], p["patch_embed.norm.bias"])
    for i in range(c.groups):
        group_in = t
        for j in range(c.depth):
            t = _block(t, p, f"layers.{i}.residual_group.blocks.{j}", c.heads, 0 if j % 2 == 0 else WINDOW // 2)
        t = _conv_block(t.transpose(2, 0, 1), p, f"layers.{i}.conv", c.three_conv).transpose(1, 2, 0) + group_in
    t = _layer_norm(t, p["norm.weight"], p["norm.bias"])
    x = _conv_block(t.transpose(2, 0, 1), p, "conv_after_body", c.three_conv) + first
    x = _leaky(_conv(x, p["conv_before_upsample.0.weight"], p["conv_before_upsample.0.bias"]), 0.01)
    assert x.shape[0] == NUM_FEAT
    for name in ("conv_up1", "conv_up2")[: c.scale // 2]:
        x = x.repeat(2, axis=1).repeat(2, axis=2)
        x = _leaky(_conv(x, p[f"{name}.weight"], p[f"{name}.bias"]), 0.2)
    x = _conv(_leaky(_conv(x, p["conv_hr.weight"], p["conv_hr.bias"]), 0.2), p["conv_last.weight"],
              p["conv_last.bias"])
    return x + mean
