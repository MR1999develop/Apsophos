"""The SCUNet network ("Practical Blind Image Denoising via Swin-Conv-UNet and
Data Synthesis", K. Zhang et al. 2022) as an ONNX graph with a fixed square
input, built from the original KAIR weights.

The graph reproduces SCUNet(in_nc=3, config=[4]*7, dim=64): a U-Net with
three down-sampling stages, a body and three up-sampling stages, each made of
Swin-Conv blocks (a convolution branch and a window-attention branch side by
side). The graph builder and the Swin block are in `onnx_graph.py`.

The input side must be a multiple of 64 (three halvings and 8×8 windows);
the input is 1×3×N×N in 0..1, the output has the same shape.
"""
from __future__ import annotations

from typing import Final

import numpy as np

import onnxproto as pb
from onnx_graph import Graph, SwinWeights, swin_block

WINDOW: Final = 8
HEAD_DIM: Final = 32
DIM: Final = 64
BLOCKS_PER_STAGE: Final = 4
OPSET: Final = 13


# --- SCUNet blocks ---------------------------------------------------------

def _transformer(g: Graph, x: str, p: dict, prefix: str, size: int, ch: int, shifted: bool) -> str:
    """Swin block on x (1, H, W, C), with the weights named as in KAIR's SCUNet."""
    msa = f"{prefix}.msa"
    wt = SwinWeights(
        ln1=(p[f"{prefix}.ln1.weight"], p[f"{prefix}.ln1.bias"]),
        qkv=(p[f"{msa}.embedding_layer.weight"], p[f"{msa}.embedding_layer.bias"]),
        table=p[f"{msa}.relative_position_params"],
        proj=(p[f"{msa}.linear.weight"], p[f"{msa}.linear.bias"]),
        ln2=(p[f"{prefix}.ln2.weight"], p[f"{prefix}.ln2.bias"]),
        fc1=(p[f"{prefix}.mlp.0.weight"], p[f"{prefix}.mlp.0.bias"]),
        fc2=(p[f"{prefix}.mlp.2.weight"], p[f"{prefix}.mlp.2.bias"]),
    )
    return swin_block(g, x, wt, size=size, ch=ch, heads=ch // HEAD_DIM, window=WINDOW, shifted=shifted)


def _swin_conv_block(g: Graph, x: str, p: dict, prefix: str, size: int, half: int, shifted: bool) -> str:
    """Swin-Conv block on x (1, 2·half, H, W): a 1×1 convolution splits the
    channels into a residual convolution branch and a transformer branch, a
    second 1×1 convolution mixes them back, plus a residual."""
    y = g.conv(x, p[f"{prefix}.conv1_1.weight"], p[f"{prefix}.conv1_1.bias"])
    conv_x = g.slice(y, 1, 0, half)
    trans_x = g.slice(y, 1, half, 2 * half)
    c = g.conv(conv_x, p[f"{prefix}.conv_block.0.weight"], None, pad=1)
    c = g.conv(g.op("Relu", [c]), p[f"{prefix}.conv_block.2.weight"], None, pad=1)
    conv_x = g.op("Add", [c, conv_x])
    t = g.transpose(trans_x, [0, 2, 3, 1])
    t = _transformer(g, t, p, f"{prefix}.trans_block", size, half, shifted)
    trans_x = g.transpose(t, [0, 3, 1, 2])
    res = g.conv(g.concat([conv_x, trans_x], 1), p[f"{prefix}.conv1_2.weight"], p[f"{prefix}.conv1_2.bias"])
    return g.op("Add", [x, res])


def _stage(g: Graph, x: str, p: dict, prefix: str, first: int, size: int, half: int) -> str:
    """BLOCKS_PER_STAGE Swin-Conv blocks, alternating plain and shifted windows.
    In KAIR's SCUNet a window covering the whole stage is never shifted; with
    the training resolution of 256 px that never happens."""
    for i in range(BLOCKS_PER_STAGE):
        x = _swin_conv_block(g, x, p, f"{prefix}.{first + i}", size, half, shifted=i % 2 == 1)
    return x


def build(p: dict[str, np.ndarray], tile_px: int) -> bytes:
    """ONNX model of SCUNet for 1×3×tile_px×tile_px inputs, with the weights `p`
    (the KAIR state dict). Raises ValueError for an unusable tile size and
    KeyError if a weight is missing."""
    if tile_px % 64 or tile_px < 64:
        raise ValueError("the tile side must be a positive multiple of 64")
    g = Graph()
    n = tile_px
    x1 = g.conv("input", p["m_head.0.weight"], None, pad=1)                         # 64 × n
    x = _stage(g, x1, p, "m_down1", 0, n, DIM // 2)
    x2 = g.conv(x, p["m_down1.4.weight"], None, stride=2)                            # 128 × n/2
    x = _stage(g, x2, p, "m_down2", 0, n // 2, DIM)
    x3 = g.conv(x, p["m_down2.4.weight"], None, stride=2)                            # 256 × n/4
    x = _stage(g, x3, p, "m_down3", 0, n // 4, 2 * DIM)
    x4 = g.conv(x, p["m_down3.4.weight"], None, stride=2)                            # 512 × n/8
    x = _stage(g, x4, p, "m_body", 0, n // 8, 4 * DIM)
    x = g.conv_transpose(g.op("Add", [x, x4]), p["m_up3.0.weight"])                  # 256 × n/4
    x = _stage(g, x, p, "m_up3", 1, n // 4, 2 * DIM)
    x = g.conv_transpose(g.op("Add", [x, x3]), p["m_up2.0.weight"])                  # 128 × n/2
    x = _stage(g, x, p, "m_up2", 1, n // 2, DIM)
    x = g.conv_transpose(g.op("Add", [x, x2]), p["m_up1.0.weight"])                  # 64 × n
    x = _stage(g, x, p, "m_up1", 1, n, DIM // 2)
    g.conv(g.op("Add", [x, x1]), p["m_tail.0.weight"], None, pad=1, out="output")   # 3 × n
    return pb.model(g.nodes, g.inits, [pb.value_info("input", [1, 3, n, n])], [pb.value_info("output", [1, 3, n, n])],
                    graph_name="scunet_color_real", producer="apsophos", opset=OPSET)
