"""The SwinIR network for real-world super-resolution ("SwinIR: Image
Restoration Using Swin Transformer", J. Liang et al. 2021) as an ONNX graph
with a fixed square input, built from the authors' weights.

The graph reproduces SwinIR with the `nearest+conv` upsampler of the
real-world models: a 3×3 convolution, groups of Swin blocks (each group with
a convolution and a residual), a convolution with a long residual, then
nearest-neighbour doublings each followed by a convolution. The
configuration is read from the weights (see `config`): SwinIR-M has 6
groups of 6 blocks with 180 channels, SwinIR-L 9 groups of 6 blocks with 240
channels and three-layer convolutions in the residuals.

The input is 1×3×N×N in 0..1 (N a multiple of 8, the window, and at least
16); the output is 1×3×sN×sN for the scale s of the weights, not clipped.
The graph builder and the Swin block are in `onnx_graph.py`.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Final

import numpy as np

import onnxproto as pb
from onnx_graph import Graph, SwinWeights, swin_block

WINDOW: Final = 8
# channels of the upsampler
NUM_FEAT: Final = 64
# subtracted from the input and added back to the output (DIV2K's mean)
RGB_MEAN: Final = (0.4488, 0.4371, 0.4040)
OPSET: Final = 13


@dataclass(frozen=True)
class Config:
    scale: int
    dim: int
    heads: int
    groups: int
    depth: int
    three_conv: bool


def config(p: dict[str, np.ndarray]) -> Config:
    """The configuration of the network whose weights are `p`. Raises
    ValueError if they are not those of a real-world SwinIR."""
    if "conv_up1.weight" not in p or "conv_hr.weight" not in p:
        raise ValueError("not the weights of a real-world SwinIR (nearest+conv upsampler)")
    groups = 0
    while f"layers.{groups}.residual_group.blocks.0.norm1.weight" in p:
        groups += 1
    depth = 0
    while f"layers.0.residual_group.blocks.{depth}.norm1.weight" in p:
        depth += 1
    table = p.get("layers.0.residual_group.blocks.0.attn.relative_position_bias_table")
    if not groups or not depth or table is None or table.shape[0] != (2 * WINDOW - 1) ** 2:
        raise ValueError("unexpected SwinIR layout (window of 8 expected)")
    dim = int(p["conv_first.weight"].shape[0])
    heads = int(table.shape[1])
    if dim % heads:
        raise ValueError(f"{dim} channels do not split into {heads} heads")
    return Config(scale=4 if "conv_up2.weight" in p else 2, dim=dim, heads=heads, groups=groups, depth=depth,
                  three_conv="conv_after_body.0.weight" in p)


def _block_weights(p: dict, prefix: str, heads: int) -> SwinWeights:
    side = 2 * WINDOW - 1
    # SwinIR's table is ((2w−1)², heads), indexed by Δrow·(2w−1) + Δcolumn
    table = p[f"{prefix}.attn.relative_position_bias_table"].T.reshape(heads, side, side)
    return SwinWeights(
        ln1=(p[f"{prefix}.norm1.weight"], p[f"{prefix}.norm1.bias"]),
        qkv=(p[f"{prefix}.attn.qkv.weight"], p[f"{prefix}.attn.qkv.bias"]),
        table=table,
        proj=(p[f"{prefix}.attn.proj.weight"], p[f"{prefix}.attn.proj.bias"]),
        ln2=(p[f"{prefix}.norm2.weight"], p[f"{prefix}.norm2.bias"]),
        fc1=(p[f"{prefix}.mlp.fc1.weight"], p[f"{prefix}.mlp.fc1.bias"]),
        fc2=(p[f"{prefix}.mlp.fc2.weight"], p[f"{prefix}.mlp.fc2.bias"]),
    )


def _leaky(g: Graph, x: str, slope: float) -> str:
    return g.op("LeakyRelu", [x], [pb.attr_float("alpha", slope)])


def _conv_block(g: Graph, x: str, p: dict, prefix: str, three_conv: bool) -> str:
    """The convolution closing a residual: one 3×3, or (SwinIR-L) a 3×3 to a
    quarter of the channels, a 1×1 and a 3×3 back, with leaky ReLUs. Kept in
    FP32 in the FP16 variant: the residual stream it works on grows from
    group to group (in SwinIR-M ×2 the last one reaches 474,000, beyond FP16's
    65,504), and the cost is small next to the attention."""
    keep = g.fp32_name
    if not three_conv:
        return g.conv(x, p[f"{prefix}.weight"], p[f"{prefix}.bias"], pad=1, out=keep("conv"))
    y = _leaky(g, g.conv(x, p[f"{prefix}.0.weight"], p[f"{prefix}.0.bias"], pad=1, out=keep("conv")), 0.2)
    y = _leaky(g, g.conv(y, p[f"{prefix}.2.weight"], p[f"{prefix}.2.bias"], out=keep("conv")), 0.2)
    return g.conv(y, p[f"{prefix}.4.weight"], p[f"{prefix}.4.bias"], pad=1, out=keep("conv"))


def _nearest_2x(g: Graph, x: str, ch: int, size: int) -> str:
    """Nearest-neighbour doubling of x (1, ch, size, size), with data
    movement only (Reshape and Expand), which every back end runs in FP16;
    in four dimensions, the most DirectML's Expand takes."""
    x = g.reshape(x, [ch * size, 1, size, 1])
    x = g.op("Expand", [x, g.ints([ch * size, 2, size, 2])])
    return g.reshape(x, [1, ch, 2 * size, 2 * size])


def build(p: dict[str, np.ndarray], tile_px: int) -> bytes:
    """ONNX model of SwinIR for 1×3×tile_px×tile_px inputs, with the weights
    `p`. Raises ValueError for an unusable tile size or weights, and KeyError
    if a weight is missing."""
    if tile_px % WINDOW or tile_px < 2 * WINDOW:
        raise ValueError(f"the tile side must be a multiple of {WINDOW}, at least {2 * WINDOW}")
    c = config(p)
    g = Graph()
    n = tile_px
    mean = np.array(RGB_MEAN, dtype=np.float32).reshape(1, 3, 1, 1)
    first = g.conv(g.op("Sub", ["input", g.const(mean)]), p["conv_first.weight"], p["conv_first.bias"], pad=1)
    t = g.layer_norm(g.transpose(first, [0, 2, 3, 1]), p["patch_embed.norm.weight"], p["patch_embed.norm.bias"])
    for i in range(c.groups):
        group_in = t
        for j in range(c.depth):
            wt = _block_weights(p, f"layers.{i}.residual_group.blocks.{j}", c.heads)
            t = swin_block(g, t, wt, size=n, ch=c.dim, heads=c.heads, window=WINDOW, shifted=j % 2 == 1)
        y = _conv_block(g, g.transpose(t, [0, 3, 1, 2]), p, f"layers.{i}.conv", c.three_conv)
        t = g.op("Add", [g.transpose(y, [0, 2, 3, 1]), group_in])
    t = g.layer_norm(t, p["norm.weight"], p["norm.bias"])
    x = g.op("Add", [_conv_block(g, g.transpose(t, [0, 3, 1, 2]), p, "conv_after_body", c.three_conv), first])
    # PyTorch's default slope here, 0.2 in the upsampler
    x = _leaky(g, g.conv(x, p["conv_before_upsample.0.weight"], p["conv_before_upsample.0.bias"], pad=1), 0.01)
    size = n
    for name in ("conv_up1", "conv_up2")[: c.scale // 2]:
        x = _nearest_2x(g, x, NUM_FEAT, size)
        size *= 2
        x = _leaky(g, g.conv(x, p[f"{name}.weight"], p[f"{name}.bias"], pad=1), 0.2)
    x = _leaky(g, g.conv(x, p["conv_hr.weight"], p["conv_hr.bias"], pad=1), 0.2)
    x = g.conv(x, p["conv_last.weight"], p["conv_last.bias"], pad=1)
    g.op("Add", [x, g.const(mean)], out="output")
    out = n * c.scale
    return pb.model(g.nodes, g.inits, [pb.value_info("input", [1, 3, n, n])],
                    [pb.value_info("output", [1, 3, out, out])],
                    graph_name=f"swinir_real_x{c.scale}", producer="apsophos", opset=OPSET)
