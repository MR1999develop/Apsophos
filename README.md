# Apsophos

*Apsophos* (ἄψοφος) is ancient Greek for "noiseless".

Photo noise reduction with the **SCUNet** neural network, trained on real camera and phone noise (compressed JPEGs included). A desktop application for **Windows** and **Linux**, written in Rust with egui.

- **GPU acceleration**: on Windows the network runs on the GPU. On NVIDIA RTX 30xx and later cards with TensorRT for RTX installed it runs in half precision on the Tensor Cores (on an RTX 3050 laptop: about 0.5 s per megapixel, 4 times faster than DirectML, 40 times faster than the CPU); otherwise through DirectML, which works on any DirectX 12 card with nothing extra to install, also in half precision from the FP16 models (about 1.7 times faster than FP32, with half the GPU memory; a card that cannot run them gets the FP32 model). If the GPU cannot be used, processing falls back to the CPU and the program says so.
- **Tile size chosen for your GPU and your image**: larger tiles waste less work on the overlap but need more memory, and larger is not always faster; tiles do not shrink at the edges, so very large ones also waste work there. The first time, the sizes are tried from the smallest up (448 → 704 → 960 → 1216 → 1600 px), each timed on a blank tile, until one does not fit or is not faster. The timings are remembered, and for each image the size that processes it fastest is used, switching size only when that saves more time than loading the other model (*Advanced ▸ Re-detect best tile size* measures again). On the CPU 448 px is used, the fastest there.
- **Progressive preview**: the network processes 320 px tiles, starting from the area you are looking at; the result appears as it goes.
- **Instant adjustments**: the network estimates the noise by itself, so it runs only once. *Grain (luminance)* and *Colour blotches* decide how much of its correction to apply and act immediately, without recomputing.
- **Before/after comparison** always on, with a draggable divider; holding `\` or Space shows the whole original. Zoom up to 3200 %, navigator for large images.
- **Faithful to the file**: 8/16/32-bit TIFF (floating point too), JPEG and PNG; ICC colour profile, EXIF and GPS data, XMP (also the "extended XMP" of JPEGs larger than one segment), print resolution (dpi), alpha channel and bit depth are kept. The EXIF orientation is applied to the pixels. Deeper images exported to 8 bits are dithered, so smooth skies do not band.
- **English or Italian**: the interface follows the Windows language, or the one chosen in *Preferences*.
- **Wide-gamut photos denoised as well as sRGB ones**: the network was trained on sRGB photos, so tiles of a photo in Adobe RGB, ProPhoto or a linear space are converted to sRGB for it and only its correction is brought back. The file keeps its own colour space and profile, and colours outside sRGB are not clipped.
- **Colour-managed viewer**: images with their own profile (Adobe RGB, ProPhoto…) are shown converted to sRGB with Little CMS, so they do not look dull. A **removed noise** view (`N`) and a luma **histogram** of original and result help judge the settings.
- **Accessible**: the image, the navigator and the histogram have names and descriptions for screen readers (zoom, view, tiles done, median brightness); the single-key shortcuts (F, 1, N, +, −, \\) can be turned off in the Preferences for speech input.
- **Processing queue** with pause, stop and a final summary; an error on one file does not stop the others.
- **Never overwrites**: results are named `photo_dn.tif`, then `photo_dn 2.tif`; writing goes through a temporary file.
- Undo/Redo of the settings, remembered settings and folders, a dark greyscale interface so that the only colour on screen is the photo.

## Building and running

You need Rust (the version is pinned in `rust-toolchain.toml`; `rustup` installs it by itself).

```bash
cargo run --release
```

The first build downloads ONNX Runtime, DirectML and the TensorRT for RTX provider (Internet access and a few minutes needed). On Windows the scripts in `scripts\` do the rest:

- `scripts\prepare.bat`, once: makes the missing FP16 models, says where TensorRT for RTX is (`-RepairDlls` fetches ONNX Runtime's DLLs again if a build lost them);
- `scripts\check.bat`, after every change: `cargo xtask check` (format, build and tests in release and in debug, Clippy with `-D warnings`, cargo-deny, which must be installed: `cargo install --locked cargo-deny`), the same command the CI runs on Windows and Ubuntu, plus the TensorRT for RTX DLLs next to the program; the full output goes to `target\check.log` and a summary to `target\check-summary.txt`; a branch goes into `main` only when everything is green;
- `scripts\self-test.bat`: speed of every back end, in `target\self-test.txt`;
- `scripts\run.bat`: starts the program.

On Linux build with `--no-default-features` (DirectML is Windows-only; the network then runs on the CPU). The window also needs these system libraries:

```bash
sudo apt install libxkbcommon-dev libgl1-mesa-dev libwayland-dev
```

### GPU

*Advanced ▸ Compute device* is set to **Auto**: the fastest available back end. The normal build has two, and needs nothing installed beyond the graphics driver:

- **TensorRT for RTX** (feature `nvrtx`), NVIDIA's TensorRT for GeForce RTX 30xx and later cards. It needs two DLLs of **TensorRT for RTX 1.4** (the version the ONNX Runtime build expects), `tensorrt_rtx_1_4.dll` and `tensorrt_onnxparser_rtx_1_4.dll`, next to the program: `scripts\check.bat` copies them there from the NVIDIA zip (extracted to `C:\TensorRT-RTX-1.4…`). CUDA and cuDNN are not needed. It runs the network in the precision stored in the model, so for half precision it uses the FP16 models `scunet_color_real_<N>_fp16.onnx` (see below) next to the normal ones. Building the engine for a tile size takes about 15 s; the first time it is saved in `%LOCALAPPDATA%\Apsophos\tensorrt-rtx-cache`, so later loads take about half a second.
- **DirectML** (feature `directml`): any DirectX 12 card. Used when TensorRT for RTX is not there or not supported by the card.

On an RTX 3050 laptop GPU a 24 MP photo takes about 13 s with TensorRT for RTX, 55 s with DirectML and 8 minutes on the CPU. (A CUDA / TensorRT 10 back end was tried and dropped: it needed several GB of NVIDIA libraries and was slower.)

Any build can be timed with `apsophos --self-test report.txt`, which runs the network on every compiled back end and on the CPU without opening the window, and reports the tile size it would use for typical photo sizes.

### Package to distribute

`scripts\package.bat` (after `check.bat`) makes `dist\Apsophos-<version>-windows-x64\` and its zip: the program, its DLLs, the models the tile search picks in practice (448, 960 and 1216 px, plus FP16 960 and 1216 for the GPU; `scripts\package.bat full` adds 704, 1600 and the other FP16 files), an end-user README, `LICENSE.txt` and the third-party licences (`packaging\`). It needs nothing installed beyond the graphics driver. `scripts\package.bat nonvidia` leaves out the NVIDIA DLLs (DirectML and CPU only). The scripts that build (`check`, `prepare`) replace the local paths of the sources with neutral prefixes (`--remap-path-prefix`), so the program carries no user name or local folder; a plain `cargo build` without them rebuilds everything and keeps the local paths.

With Inno Setup 6 installed, `package.bat` also makes `dist\Apsophos-<version>-windows-x64-setup.exe` (`packaging\apsophos.iss`): a per-user installer, no administrator rights, in `%LOCALAPPDATA%\Programs\Apsophos`, with a Start menu entry and an uninstaller, in English or Italian.

### Release

Pushing a tag `v<version>` (the version in `Cargo.toml`) runs `.github/workflows/release.yml` on GitHub: it makes the models from the original KAIR weights (checked by SHA-256), builds the DirectML and CPU program (the TensorRT for RTX DLLs are an NVIDIA download), makes the package, the installer and the winget manifest (`scripts\winget-manifest.ps1`), and attaches the package, the installer, `SHA256SUMS.txt` (SHA-256 of the zip, the installer and the models in the package) and the build provenance attestation to a **draft** release, published by hand. The program is built with `cargo auditable`, so `cargo audit bin apsophos.exe` checks the released executable against the security advisories. To check a download: `sha256sum -c --ignore-missing SHA256SUMS.txt` (or compare with `Get-FileHash` in PowerShell) and `gh attestation verify <file> --repo <owner>/<repository>`. With the repository secrets `SIGN_PFX` (a code-signing certificate, `.pfx` in base64) and `SIGN_PASSWORD` it also signs the program and the installer (`scripts\sign.ps1`, with a timestamp). Run by hand from the *Actions* tab it only builds, as artifacts.

## The model

The program looks for `scunet_color_real_448.onnx` (about 77 MB), in this order:

1. the file chosen in *Edit ▸ Preferences*;
2. the `APSOPHOS_MODEL` environment variable;
3. a `models` folder next to the executable (or up to three folders above it, handy during development);
4. `%APPDATA%\Apsophos\models` (Windows) or `~/.local/share/Apsophos/models` (Linux).

The model is not in the repository (it is excluded by `.gitignore`). The larger-tile variants (`scunet_color_real_704.onnx`, `_960`, `_1216`, `_1600`) are optional; put them in the same folder. They are all generated from the original [KAIR](https://github.com/cszn/KAIR/releases/tag/v1.0) weights with Python and NumPy only (no PyTorch, no onnx package): `tools/pth_weights.py` reads the checkpoint safely (only tensors are accepted), `tools/scunet_graph.py` builds the network and `tools/onnxproto.py` writes the ONNX file.

```bash
python tools/export_onnx_model.py --weights scunet_color_real_psnr.pth \
    --output models/scunet_color_real_448.onnx --verify
python tools/export_onnx_model.py --weights scunet_color_real_psnr.pth --tile 960 \
    --output models/scunet_color_real_960.onnx
```

`--verify` needs `onnxruntime` for Python and checks that the new model reduces the noise of a test tile.

The FP16 variants, for TensorRT for RTX and DirectML, are made from those, with Python and NumPy only:

```bash
python tools/make_fp16_model.py models/scunet_color_real_*.onnx
```

Weights, convolutions and matrix products become FP16; the layer-normalisation statistics and the attention softmax stay in FP32, and input and output stay FP32. On a photo and on extreme test patterns the result differs from the FP32 model by at most 0.4 of an 8-bit level (67–81 dB PSNR), and no FP16 value comes near the overflow limit.

## Usage

1. **Open** an image (`Ctrl+O`, or drop it onto the window).
2. **Denoise** (`Ctrl+R`). Zoom to 100 % (`1`) where you want to check: the network starts there.
3. Adjust **Grain** and **Colour blotches**, or pick *Light / Medium / Strong*.
4. **Export** (`Ctrl+E`) with the settings of *File ▸ Export settings*, or *Export as…* (`Ctrl+Shift+E`).

`N` switches the right side of the comparison to the noise that was removed (×4, around mid grey): with good settings it looks like even grain; recognisable edges or texture mean detail is being taken away.

For many files: drop them (or a folder) onto the window, then **Process queue** (`Ctrl+B`). It uses the current settings and export settings.

The full list of keys is in *Help ▸ Keyboard shortcuts*. What the program does (model loading, devices, files, errors) is written to a log, one file per day, the last seven kept: *Help ▸ Open log folder* (`%LOCALAPPDATA%\Apsophos\logs`).

## Structure

```
crates/core     pure algorithms: Image, overlapping tiles, luminance/colour blending
crates/io       TIFF/JPEG/PNG with ICC, EXIF/GPS/XMP, alpha, 8/16/32 bit; size limits; atomic writing
crates/engine   SCUNet on ONNX Runtime (CPU, DirectML, TensorRT for RTX, CUDA, TensorRT) behind the core's TileModel trait
crates/app      egui interface: viewer (colour-managed), panels, queue, background worker, log
tools/          model conversion (.pth → ONNX, FP32 → FP16), in Python
scripts/        Windows scripts: preparation, checks, self-test, package, bundles
claude/         state of the work (status.md)
```

The core knows neither egui nor ONNX: the network is seen through the `TileModel` trait, and the tests use a fake "network" to check that tiled processing gives the same result as the whole image.

## Tests

```bash
cargo test --workspace -- --nocapture
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check     # licences and advisories of the dependencies (cargo install cargo-deny)
```

When the model is present in `models/`, `crates/engine/tests/model.rs` also runs the real network on CPU and GPU and prints the timings.

## Known limitations

- CMYK and palette TIFFs are not supported; of a multi-page TIFF only the first page is processed (preview thumbnails are ignored).
- The camera maker note is not carried into the new file (its standard EXIF data is).
- Memory: about 48 bytes per pixel (24 MP ≈ 1.2 GB), plus up to about 2.5 GB for the network on the CPU. Images that would not fit in the memory available are refused before decoding, with a message.

## Security

Security problems are reported privately, as `SECURITY.md` explains.

## Licence

Copyright (C) 2026 MR. GPL-3.0-only (`LICENSE`), with an additional permission under section 7 of the GPL (`LICENSE-EXCEPTION`) that allows conveying the program together with NVIDIA TensorRT for RTX and Microsoft DirectML, which are not free software. SCUNet: K. Zhang et al., Apache-2.0. Inter font: SIL Open Font License 1.1 (`crates/app/assets/fonts/OFL.txt`). The licences of all the components shipped in the package are listed in `packaging/THIRD-PARTY-NOTICES.txt`.
