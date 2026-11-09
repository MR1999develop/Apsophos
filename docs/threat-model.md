# Threat model

Apsophos is a desktop program for Windows (and Linux, without GPU back
ends) that opens photos, runs a neural network on them and writes new
files next to them. It has no network connection, no accounts, no server
and no plug-ins. This document answers the four questions for what crosses
a trust boundary, and is reviewed whenever one of the parts below changes.

## 1. What are we building?

| Asset | Why it matters |
|---|---|
| The user's photos | Unique; the program must never damage or overwrite them |
| The exported files | Must be complete and faithful (pixels, profile, metadata) |
| The user's computer | The program runs with the user's rights and must not become a way in |
| The settings | Lose them and the user redoes the setup, nothing worse |

Trust boundaries, from the least trusted:

1. **Image files** (TIFF, JPEG, PNG) chosen or dropped by the user, or found
   in a folder added to the queue: anyone can have made them.
2. **Model files** (`.onnx`) from the `models` folder or chosen by hand:
   parsed by ONNX Runtime (C++) inside the process. The upscaling models
   (`swinir_real_*.onnx`) are found in the same folder or in the user's
   data folder, never chosen by hand.
3. **The network**, only for the upscaling models, only when the user
   presses the button that downloads them: an HTTPS answer from GitHub.
4. **DLLs** loaded at run time: ONNX Runtime, DirectML, TensorRT for RTX.
5. **The settings file** written by eframe in the user's data folder.
6. **The release pipeline** (GitHub Actions): what reaches the users.

## 2. What can go wrong? (STRIDE, CIA-T)

| Boundary | Threat | Kind |
|---|---|---|
| Image | Huge dimensions or decompression bombs exhaust memory | Denial of service |
| Image | Malformed TIFF/EXIF/XMP/JPEG segments crash or hang a parser of ours (`meta.rs`, `jpeg_app.rs`, `jpeg_quality.rs`, `icc.rs`) | Denial of service |
| Image | A malformed ICC profile exploits Little CMS (C, in process) | Elevation, tampering |
| Image | A crafted name or metadata makes the export write outside its folder or over the original | Tampering |
| Image | Metadata shown in the interface as text that looks like a program message | Spoofing |
| Model | A crafted `.onnx` exploits the ONNX Runtime parser | Elevation |
| Network | A forged or altered model arrives instead of the published one (attacker on the network, compromised release) | Tampering, elevation |
| Network | The download never ends, or fills the disk | Denial of service |
| Network | The request tells someone what the user does | Information disclosure |
| DLL | A planted `DirectML.dll` or `onnxruntime*.dll` in a searched folder is loaded instead | Elevation |
| Settings | A damaged settings file stops the program from starting | Denial of service |
| Release | A compromised action or dependency ships malicious code | Tampering |
| Logs | Paths and errors in the log reveal the user's folders to whoever reads it | Information disclosure |

## 3. What are we doing about it?

- **Size before decoding:** the dimensions are checked against
  `ReadLimits` (memory available, a megapixel cap) from the header, before
  any pixel is decoded; metadata values are capped (`MAX_VALUE_BYTES`, 4 MB;
  at most 1,000 entries per IFD; the header segments of a JPEG, APP and
  quantisation tables, at most 8 MB in all).
- **Our parsers return errors, never panic:** they read through bounded
  slices with `get`, and property tests feed them random bytes
  (`denoise_io`); a panic in the worker becomes a failed job, not a crash.
- **Third-party decoders in safe Rust** (`image`, `tiff`, `zune-jpeg`,
  `png`); the only C code that reads file data is Little CMS (ICC
  profiles), fuzzed by OSS-Fuzz upstream and kept up to date.
- **Writing:** the output name is built by the program from the source
  name, a suffix and an extension, in a folder the user chose; a temporary
  file created exclusively (it must not exist) is synced and renamed at the
  end with a write-through rename, so a power cut leaves either the old
  state or the complete file; the original is never the target
  (`output_path` steps around it).
- **Format from content**, not from the extension.
- **Models:** only files the user put in the program's `models` folder or
  chose by hand, and the upscaling models the user downloaded.
- **Download of the upscaling models:** only when the user presses the
  button (an invitation at start says so; it can be turned off); from one
  fixed HTTPS address (the project's release `models-1`), HTTPS only,
  through Windows' own TLS and certificate store; each file must have the
  size and SHA-256 compiled into the program (`models/upscalers.txt`),
  otherwise it is deleted and refused, so a forged file never reaches ONNX
  Runtime; no more bytes than expected are read; time limits on connection,
  answer and transfer; Cancel at any time; the file is written beside its
  name and renamed durably. An interrupted download resumes from its
  partial file (`.<name>.part`, locked while in use, so two windows never
  write it together) with an HTTP `Range` request; the SHA-256 covers the
  whole file, the part already on disk included, so a partial file altered
  on disk is refused like a forged download; a partial file larger than the
  model, a server that does not answer with exactly the rest of the file,
  or a wrong hash make it start again from the first byte (a wrong hash
  once, then the file is refused and deleted). The request carries the
  program's name and version and, when resuming, the byte it resumes from;
  nothing about the user. No other network access.
- **Weights for the models** are converted by the tools in `tools/`, which
  read only tensors from a checkpoint (any other pickled object is
  refused). The upscaler's converter accepts only the four SwinIR files
  whose SHA-256 it knows (`tools/export_upscaler_model.py`).
- **Large outputs:** an enlarged export checks the format's limits, the
  memory its writer needs and the free disk space before any work, and is
  written through the same temporary file and durable rename as the rest.
- **DLLs:** the first thing `main` does is restrict the search for DLLs
  loaded by name (by ONNX Runtime and its providers too) to the program's
  folder and System32 (`SetDefaultDllDirectories`): the current folder and
  the `PATH` are never searched. The DLLs ship next to the executable.
- **Settings:** an unreadable file falls back to the defaults.
- **Release:** the downloadable models are built by a workflow run by
  hand (`models.yml`), which checks them against `models/upscalers.txt`
  before attaching them to a draft release published by hand;
  read-only workflow tokens, actions pinned to commit hashes,
  `cargo deny` (advisories, yanked crates, licences, sources) in every
  check, Dependabot with a week's cooldown, draft releases published by
  hand, optional code signing. The local check and the CI run the same
  command (`cargo xtask check`), so what is checked before a merge is
  what the CI checks. The program's release (`release.yml`) is built with
  `cargo auditable` (the dependency list inside the executable, for
  `cargo audit bin`); the draft release carries `SHA256SUMS.txt` with the
  SHA-256 of the zip, of the installer and of every SCUNet model in the
  package, and a build provenance attestation covering the three files,
  also attached as a file.
- **Executables** built with Control Flow Guard and CET shadow stack
  compatibility (`.cargo/config.toml`), without local paths inside.

## 4. Accepted risks

- **Parsers in process:** decoders are not isolated in a separate process;
  the safe-Rust decoders make memory corruption unlikely, and the program
  holds no secrets or privileges beyond the user's. Revisit if a decoder in
  C or C++ is added (RAW support, for instance).
- **Model files are not verified by hash:** a model chosen by hand is
  trusted like a document the user opens; the official models are made
  locally from weights whose SHA-256 is checked
  (`tools/export_onnx_model.py`, `tools/export_upscaler_model.py`). The
  program does not check the models of its own folder either: their
  SHA-256 is in the release's `SHA256SUMS.txt`, for a check by hand.
- **Logs hold file paths:** they stay on the user's computer, in the user's
  data folder, one file a day, the last seven kept.
