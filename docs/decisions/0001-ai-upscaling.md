# 0001 — AI upscaling ×2 and ×4

Status: accepted, 5 November 2026 (the owner chose the variants and the 4× model).

## Context

The owner asks for a 2× and 4× upscaler of the highest quality, with a
neural network. What the program and its rules already fix:

- **Fidelity first.** A network trained for fidelity (L1/L2 loss) gives the
  most faithful result; a generative one (GAN) invents plausible detail that
  was not in the photo. Generated detail is offered only as a separate,
  labelled choice, and never judged with PSNR.
- **Upscaling is the last step**, after denoising and blending: steps after
  it would work on synthesised pixels. 4× is less credible than 2× on fine
  textures, so 2× is the suggested choice.
- **Task separate from model:** the program asks for "the upscaler for 2×",
  not for a file; every model has a card (source, licence, training data).
  No download from inside the program, no cloud, no telemetry; the result is
  always a new file.
- **Preview equal to the process:** a before/after of a small region, made
  on request with the same model and the same code as the export.
- **Large images:** 4× of a 24 MP photo is 384 MP; as floating-point RGB that
  is 4.6 GB. The memory peak is estimated before starting; disk space and
  format limits are checked before the work starts, not after it.
- **Long work:** progress after 1 s, estimated time and Cancel after 10 s;
  the window stays usable.

Cost, estimated from the networks' layers (to be measured): SwinIR-M does
about 13 million multiply-adds per input pixel at 4×, ten times SCUNet's 1.3
million; SwinIR-L about 30 million. On the RTX 3050 laptop with TensorRT for
RTX in FP16, where SCUNet takes 0.4–0.8 s per megapixel, SwinIR-M should take
4–8 s per input megapixel: 1–2 minutes for 4× of a 12 MP photo. On the CPU
the same work takes about an hour.

## Decision 1 — the network

| | Faithful variant | Generative variant | Licence | Notes |
|---|---|---|---|---|
| **SwinIR, real-world SR** (BSRGAN degradations) | ×2, ×4 (M); ×4 (L) | ×2, ×4 (M); ×4 (L) | Apache-2.0 | transformer, Swin blocks like SCUNet's; M 12 M parameters, L 28 M |
| Real-ESRGAN x2plus / x4plus | none | ×2, ×4 | BSD-3-Clause | convolutions only, simpler and faster |
| Classical interpolation (Catmull-Rom) | — | — | — | no detail added: the baseline |

Proposed: **SwinIR real-world**, faithful variant by default. Real-ESRGAN has
no faithful variant. Catmull-Rom stays as the baseline: in the tests (the
network must beat it) and in the preview comparison.

Decided by the owner: the generative variant is offered as a labelled
choice ("Generated detail"); 4× uses SwinIR-L (larger and trained on more
data, about 2.5 times slower than M); 2× exists only as SwinIR-M.

Measured on the converter's test image (a photo-like scene with gradients,
edges, discs and stripes, reduced and enlarged again): the faithful
real-world models expect a camera's blur and sharpen it away. With a blur of
0.5 px of the reduced image they beat Catmull-Rom, by 4.3 dB at 4× but only
by 0.04 dB at 2×; on a reduction without any blur they score below it (−2.6
dB at 2×, −0.5 dB at 4×) and slightly overshoot high-contrast stripes, with
a faint colour tint at the peaks. 2× via SwinIR-L 4× and a halving does no
better (−0.3 dB). The 2× model is kept; its gain must be judged on real
photos with the before/after preview.

Downsides: SwinIR is slower than a convolutional network and needs a new
converter (but it shares the window attention with SCUNet's); the weights
were trained on DIV2K, Flickr2K and OST (L also on WED, FFHQ, Manga109 and
SCUT-CTW1500), research datasets, as for SCUNet: the model card says so.

## Decision 2 — where it sits

Option A, **an export setting**: *Upscale: none / 2× / 4×* (and the variant)
in the export settings, used by Export and by the queue; the viewer and the
history stay at the photo's size; the preview is a before/after of a region
chosen with a click.

Option B, a step of the document: the open photo is upscaled and shown at the
new size. Sixteen times the memory of the photo for 4×, a viewer and a
history at that size, and every later step working on synthesised pixels.

Proposed: **A**.

## Decision 3 — memory: the output is streamed

The output is produced in bands (one row of tiles at a time), converted to
the file's samples (with the dither of the whole image) and handed to the
encoder:

- PNG: the encoder's stream writer takes the rows as they come;
- TIFF and JPEG: their encoders take a whole image (the `tiff` crate
  compresses only an image written in one go: strips written one by one
  come out uncompressed under a Deflate tag), so the converted samples are
  gathered: 6 bytes per pixel for 16-bit RGB (2.3 GB for 4× of 24 MP),
  3 for JPEG, never the 12 of floating point.

Peak memory: the photo's buffers at its own size, one band, and for TIFF and
JPEG the converted samples. Before any work: JPEG at most 65,535 px per
side, a TIFF under 4 GB uncompressed (classic TIFF; 32-bit float at 4× of
24 MP is refused with a hint to choose 16 bits), the gathered samples within
the free memory, a lower bound of the file size within the free disk space;
otherwise a message that says what to change.

Rejected: the whole output in memory as floating point (12 bytes per output
pixel, 4.6 GB for 4× of 24 MP).

## Decision 4 — colour, alpha, metadata

- The network sees sRGB, as SCUNet does. For a photo in another space only
  the network's correction comes back: result = classical upscale of the
  photo + (network output − classical upscale of the network's input), both
  taken back to the photo's space, so colours outside sRGB are kept.
- Grey photos go through the network as RGB and are written as grey.
- Alpha is upscaled with Catmull-Rom and clamped.
- The resolution (DPI) is kept, so the print size grows with the pixels, as
  in Photoshop's Image Size with resampling. The EXIF pixel dimensions
  (`PixelXDimension`, `PixelYDimension`) still give the original size:
  readers use the image's own size, and updating them is left for later.
- The name says what was done: `photo_dn_2x.tif`, `photo_dn_4x-gen.tif`.

## Decision 5 — tiles

- The graph has a fixed square input (the masks of the shifted windows are
  baked in, as in SCUNet's graph). Two input sizes per model: 640 px (2×)
  and 448 px (4×) for TensorRT for RTX; 320 px and 256 px for DirectML.
  Windows resets a GPU that works more than about 2 s on one submission
  (error 887A0006, "device hung"), and DirectML submits a whole tile at
  once: on the RTX 3050 laptop a 2× tile of 320 px takes 1.5 s and one of
  384 px stalls, a 4× tile of 256 px 1.6 s and one of 320 px stalls. The
  engine takes the large file on TensorRT for RTX, the small one on
  DirectML (run twice before use: the stall shows on the second run), and
  the CPU last, saying why.
- The attention runs on 4-D tensors (DirectML refuses some 5-D matrix
  products); SCUNet's graph gives the same output to the last bit.
- In the FP16 variant the convolutions that close each residual group stay
  in FP32: SwinIR-M's residual stream reaches 474,000, beyond FP16's 65,504
  (TensorRT returned non-finite values before).
- 64 input pixels along each edge of a tile are discarded. Measured against
  the whole image (with the same mirrored edges) on fine stripes: with 32 px,
  the overlap SwinIR's own tiled test uses, the tiles differ by up to 76
  levels of 255 (a visible seam); with 48 px by up to 15 (2×) and 39 (4×);
  with 64 px by up to 10, 0.05–0.09 on average.
- `TileModel` gains a scale factor (1 for the denoiser); the tile plan stays
  in input coordinates and an output rectangle is the input one times the
  factor. Every edge mirrors the photo, as now.
- Test: tiled result = whole result within a declared tolerance, on a
  synthetic image; preview = export, pixel for pixel, for the same region
  (the same function makes both).

## Decision 6 — models on disk

- `tools/swinir_graph.py` builds the graph with the operators SCUNet's
  builder uses; the window attention, layer norm and GELU move first to a
  module shared by both builders (a `tidy:` commit).
- The converter (`tools/export_upscaler_model.py`) accepts only the four
  weights files whose SHA-256 it knows and checks the result with two
  oracles: a direct NumPy forward pass on a small input (the graph matches
  it within 6·10⁻⁶) and, for the faithful models, the test image blurred
  like a camera's, reduced and enlarged again, with a PSNR above
  Catmull-Rom's. The FP16 variant differs from FP32 by 72 dB of PSNR.
- Files `swinir_real_x2_<psnr|gan>_640.onnx` and
  `swinir_real_x4_<psnr|gan>_448.onnx` (53 and 121 MB) and their `_fp16`
  variants (27 and 61 MB); each model's card (source, licence, training data, faithful or
  generative) is shown in the program.
- Distribution (owner's decision, 5 November): the installer does not carry
  them. After the program starts, an invitation says what they are for and
  to press "Download the models" (192 MB for the faithful 2× and 4×; the
  generated-detail ones, another 192 MB, are offered apart in the export
  settings). Only the FP16 files are downloaded: the GPU runs them, the CPU
  too, about twice as slowly as FP32. They come from the project's release
  `models-1`, built by `.github/workflows/models.yml`; the conversion is
  deterministic (two runs give the same bytes), so the program can pin the
  size and SHA-256 of each file (`models/upscalers.txt`) and refuse any
  other. Without the files the choice is disabled and the download offered
  in its place.

## Decision 7 — the queue with enlargement (9 November)

With the enlargement on, the queue loads SCUNet and then SwinIR for every
file: one network at a time in the GPU's memory. Measured on the RTX 3050
laptop (TensorRT for RTX, saved engines): a load takes 4–5 s, so the two
loads of a file cost 8–10 s, against about 13 s of noise reduction and
5 minutes (2×) to 10 minutes (4×) of enlargement for 24 MP: 2–3 % of the
file's time; with DirectML and on the CPU, below 1 %.

Grouping (first all the noise reductions, then all the enlargements) would
save most of those loads, but the denoised photos must wait somewhere, in
floating point (12 bytes per pixel, 288 MB for 24 MP: a coarser copy would
make the queue's files differ from the export of the same photo):

- **A: all of them on disk**, in the local data folder: 29 GB for 100
  photos of 24 MP, written and read again (seconds per photo on an SSD,
  more on a hard disk, which eats part of the saving), temporary files to
  clean up after a crash, and a queue that writes its first file only
  after denoising all of them.
- **B: groups held in memory** under a budget (about 2 GB, 6 photos of
  24 MP): saves 5 loads out of 6, about 7 s per photo, but takes that
  memory while SwinIR runs, beside the samples a 4× TIFF gathers (up to
  2.3 GB for 24 MP); a cancel or a crash loses the denoised photos of the
  group not written yet, and each file's progress becomes two phases.

Decided: **the queue stays file by file.** Both alternatives cost memory or
disk in the gigabytes to gain 2–3 % of the time, and B adds lost work on
cancel. To reconsider if the enlargement becomes much faster (then the
loads weigh more) or if the engines stop being cached between loads.

## Consequences and how they are checked

- Unit tests with a fake ×2 model (nearest neighbour) for the tile plan, the
  bands and the writers; the tiled-equals-whole and preview-equals-export
  tests above; the hostile-input tests stay as they are (no new parser).
- The self-test also times the upscaler; the measured times and the memory
  peak go in `claude/status.md`.
- The threat model gains the new model files, treated like SCUNet's
  (generated locally from pinned weights; not verified by the program).

## Measured on the RTX 3050 laptop

- Test image of the engine test (100 × 80, camera blur, reduced): 4× 29.9 dB
  against Catmull-Rom's 28.6; 2× 33.3 against 31.6; the same on TensorRT for
  RTX and DirectML.
- The self-test enlarges 1024 × 768 through the whole export path on each
  GPU and leaves the results next to its report; its times are in
  `claude/status.md`.

## Increments

1. Converter and its checks (Python); models generated locally.
2. Engine: the upscaler model, the scale factor in the tiling, tests with
   the fake model.
3. Output: banded writing for TIFF and PNG, JPEG in memory, EXIF pixel
   dimensions, limits and disk space checked first.
4. Program: export settings, worker command with progress, estimate and
   Cancel, queue, before/after preview on a region.
5. Release workflow, models README, threat model, self-test.
