"""Building blocks shared by the ONNX graph builders of Apsophos: a small
graph builder and the Swin transformer block (window attention, layer
normalisation, GELU MLP) that both SCUNet and SwinIR use.

Only standard ONNX operators (opset 13) are used, so ONNX Runtime and its GPU
back ends (DirectML, TensorRT for RTX) can run the graphs; layer
normalisation and GELU are written out in the form ONNX Runtime fuses.
Tensors in a Swin block are channels-last: (1, H, W, C).
"""
from __future__ import annotations

import math
from typing import Final, NamedTuple

import numpy as np

import onnxproto as pb

LN_EPS: Final = 1e-5
# an output name starting with this keeps its node in FP32 in the FP16
# variant (`make_fp16_model.py`): for steps whose values exceed FP16's range
KEEP_FP32_PREFIX: Final = "keep32_"
# added to the attention logits of masked pairs: large enough that the
# softmax gives them no weight, small enough to stay finite in FP16
MASK_VALUE: Final = -30000.0


class Graph:
    """ONNX graph under construction: operators return the name of their
    output tensor; weights are stored as initializers."""

    def __init__(self) -> None:
        self.nodes: list[bytes] = []
        self.inits: list[bytes] = []
        self._consts: dict[tuple, str] = {}
        self._masks: dict[tuple[int, int], tuple[str, str]] = {}
        self._n = 0

    def _name(self, prefix: str) -> str:
        self._n += 1
        return f"{prefix}_{self._n}"

    def const(self, arr: np.ndarray, data_type: int = pb.FLOAT) -> str:
        """An initializer; small identical constants (shapes, scalars) are shared."""
        arr = np.ascontiguousarray(arr)
        key = None
        if arr.size <= 16:
            key = (data_type, arr.shape, arr.astype(pb.NP_TYPE[data_type]).tobytes())
            if key in self._consts:
                return self._consts[key]
        name = self._name("c" if key else "w")
        self.inits.append(pb.tensor(name, arr, data_type))
        if key:
            self._consts[key] = name
        return name

    def shift_masks(self, n_win: int, window: int) -> tuple[str, str]:
        """The shifted-window masks for a feature map of `n_win` windows per
        side, created once per size."""
        if (n_win, window) not in self._masks:
            rows, cols = shift_masks(n_win, window)
            self._masks[(n_win, window)] = (self.const(rows), self.const(cols))
        return self._masks[(n_win, window)]

    def fp32_name(self, prefix: str) -> str:
        """A unique output name that keeps its node in FP32 in the FP16 variant."""
        return KEEP_FP32_PREFIX + self._name(prefix)

    def ints(self, values: list[int]) -> str:
        return self.const(np.array(values, dtype=np.int64), pb.INT64)

    def op(self, op_type: str, inputs: list[str], attrs: list[bytes] = (), *, out: str | None = None) -> str:
        # a named output (the graph's) gets a node name of its own
        name = self._name(op_type.lower()) if out else None
        out = out or self._name(op_type.lower())
        self.nodes.append(pb.node(op_type, inputs, out, list(attrs), name=name))
        return out

    # --- layers -----------------------------------------------------------

    def conv(self, x: str, w: np.ndarray, b: np.ndarray | None, *, stride: int = 1, pad: int = 0,
             out: str | None = None) -> str:
        k = int(w.shape[2])
        inputs = [x, self.const(w)] + ([self.const(b)] if b is not None else [])
        return self.op("Conv", inputs, [pb.attr_ints("kernel_shape", [k, k]), pb.attr_ints("strides", [stride, stride]),
                                        pb.attr_ints("pads", [pad] * 4)], out=out)

    def conv_transpose(self, x: str, w: np.ndarray) -> str:
        k = int(w.shape[2])
        return self.op("ConvTranspose", [x, self.const(w)],
                       [pb.attr_ints("kernel_shape", [k, k]), pb.attr_ints("strides", [k, k])])

    def linear(self, x: str, w: np.ndarray, b: np.ndarray) -> str:
        """y = x · wᵀ + b over the last axis (PyTorch nn.Linear)."""
        return self.op("Add", [self.op("MatMul", [x, self.const(np.ascontiguousarray(w.T))]), self.const(b)])

    def layer_norm(self, x: str, gamma: np.ndarray, beta: np.ndarray) -> str:
        mean = self.op("ReduceMean", [x], [pb.attr_ints("axes", [-1]), pb.attr_int("keepdims", 1)])
        d = self.op("Sub", [x, mean])
        var = self.op("ReduceMean", [self.op("Pow", [d, self.const(np.float32(2.0))])],
                      [pb.attr_ints("axes", [-1]), pb.attr_int("keepdims", 1)])
        y = self.op("Div", [d, self.op("Sqrt", [self.op("Add", [var, self.const(np.float32(LN_EPS))])])])
        return self.op("Add", [self.op("Mul", [y, self.const(gamma)]), self.const(beta)])

    def gelu(self, x: str) -> str:
        """Exact GELU: x · ½ · (1 + erf(x / √2))."""
        e = self.op("Erf", [self.op("Div", [x, self.const(np.float32(math.sqrt(2.0)))])])
        y = self.op("Mul", [x, self.op("Add", [e, self.const(np.float32(1.0))])])
        return self.op("Mul", [y, self.const(np.float32(0.5))])

    def slice(self, x: str, axis: int, start: int, end: int) -> str:
        return self.op("Slice", [x, self.ints([start]), self.ints([end]), self.ints([axis])])

    def concat(self, xs: list[str], axis: int) -> str:
        return self.op("Concat", xs, [pb.attr_int("axis", axis)])

    def reshape(self, x: str, shape: list[int]) -> str:
        return self.op("Reshape", [x, self.ints(shape)])

    def transpose(self, x: str, perm: list[int]) -> str:
        return self.op("Transpose", [x], [pb.attr_ints("perm", perm)])

    def roll(self, x: str, shift: int, axis: int, size: int) -> str:
        """torch.roll(x, shift, axis) for an axis of length `size`."""
        s = shift % size
        if s == 0:
            return x
        return self.concat([self.slice(x, axis, size - s, size), self.slice(x, axis, 0, size - s)], axis)


# --- Swin transformer block ------------------------------------------------

class SwinWeights(NamedTuple):
    """The weights of one Swin block, in PyTorch's layout. `table` is the
    learned relative position bias as (heads, 2w−1, 2w−1)."""
    ln1: tuple[np.ndarray, np.ndarray]
    qkv: tuple[np.ndarray, np.ndarray]
    table: np.ndarray
    proj: tuple[np.ndarray, np.ndarray]
    ln2: tuple[np.ndarray, np.ndarray]
    fc1: tuple[np.ndarray, np.ndarray]
    fc2: tuple[np.ndarray, np.ndarray]


def relative_bias(table: np.ndarray, window: int) -> np.ndarray:
    """(heads, 2w−1, 2w−1) learned table → (heads, w², w²) bias between the
    positions of a window."""
    w = window
    cord = np.array([[i, j] for i in range(w) for j in range(w)])
    rel = cord[:, None, :] - cord[None, :, :] + w - 1
    return table[:, rel[:, :, 0], rel[:, :, 1]]


def shift_masks(n_win: int, window: int) -> tuple[np.ndarray, np.ndarray]:
    """Additive masks of the shifted windows: after the roll, the last row and
    the last column of windows hold pixels from opposite sides of the image,
    which must not attend to each other. Returned as a mask per window row
    (n_win, 1, w², w²) and one per window column (n_win, w², w²), to be added
    to the logits viewed as (…, rows, columns, w², w²): a pair is masked if
    either says so, and the sum stays finite in FP16."""
    w, s = window, window - window // 2
    y, x = np.divmod(np.arange(w * w), w)
    # pairs split by the seam: one end before the shift, the other after it
    split_y = (y[:, None] < s) != (y[None, :] < s)
    split_x = (x[:, None] < s) != (x[None, :] < s)
    rows = np.zeros((n_win, 1, w * w, w * w), dtype=np.float32)
    cols = np.zeros((n_win, w * w, w * w), dtype=np.float32)
    rows[-1, 0][split_y] = MASK_VALUE
    cols[-1][split_x] = MASK_VALUE
    return rows, cols


def window_attention(g: Graph, x: str, wt: SwinWeights, *, size: int, ch: int, heads: int, window: int,
                     shifted: bool) -> str:
    """Multi-head self-attention inside w×w windows of x (1, size, size, ch);
    with `shifted`, the windows are moved by half a window (cyclic shift).
    The products run on 4-D tensors (heads, windows, pixels, ·): DirectML
    refuses some 5-D matrix products."""
    w = window
    head_dim = ch // heads
    n_win = size // w
    n_all, n_px = n_win * n_win, w * w
    if shifted:
        x = g.roll(g.roll(x, -(w // 2), 1, size), -(w // 2), 2, size)
    # (1, H, W, C) → (windows, w², C)
    t = g.reshape(x, [n_win, w, n_win, w, ch])
    t = g.reshape(g.transpose(t, [0, 2, 1, 3, 4]), [n_all, n_px, ch])
    qkv = g.linear(t, *wt.qkv)
    # (nW, P, 3·heads·hd) → (3·heads, nW, P, hd): q, k, v heads in order
    qkv = g.transpose(g.reshape(qkv, [n_all, n_px, 3 * heads, head_dim]), [2, 0, 1, 3])
    q = g.slice(qkv, 0, 0, heads)
    k = g.slice(qkv, 0, heads, 2 * heads)
    v = g.slice(qkv, 0, 2 * heads, 3 * heads)
    sim = g.op("MatMul", [q, g.transpose(k, [0, 1, 3, 2])])
    sim = g.op("Mul", [sim, g.const(np.float32(head_dim ** -0.5))])
    bias = relative_bias(wt.table, w).reshape(heads, 1, n_px, n_px)
    sim = g.op("Add", [sim, g.const(bias.astype(np.float32))])
    if shifted:
        rows, cols = g.shift_masks(n_win, w)
        sim = g.reshape(sim, [heads, n_win, n_win, n_px, n_px])
        sim = g.op("Add", [g.op("Add", [sim, rows]), cols])
        sim = g.reshape(sim, [heads, n_all, n_px, n_px])
    probs = g.op("Softmax", [sim], [pb.attr_int("axis", -1)])
    out = g.op("MatMul", [probs, v])  # (heads, nW, P, hd)
    out = g.reshape(g.transpose(out, [1, 2, 0, 3]), [n_all, n_px, ch])
    out = g.linear(out, *wt.proj)
    # back to (1, H, W, C)
    out = g.reshape(out, [n_win, n_win, w, w, ch])
    out = g.reshape(g.transpose(out, [0, 2, 1, 3, 4]), [1, size, size, ch])
    if shifted:
        out = g.roll(g.roll(out, w // 2, 1, size), w // 2, 2, size)
    return out


def swin_block(g: Graph, x: str, wt: SwinWeights, *, size: int, ch: int, heads: int, window: int,
               shifted: bool) -> str:
    """Swin block on x (1, H, W, C): window attention and a GELU MLP, each
    after a layer normalisation and with a residual."""
    a = g.layer_norm(x, *wt.ln1)
    x = g.op("Add", [x, window_attention(g, a, wt, size=size, ch=ch, heads=heads, window=window, shifted=shifted)])
    m = g.layer_norm(x, *wt.ln2)
    m = g.linear(m, *wt.fc1)
    m = g.linear(g.gelu(m), *wt.fc2)
    return g.op("Add", [x, m])
