# To fix later

Small issues found in the reviews of October 2026 and postponed until that
code changes again. Every item says why it waits. The overall state of the
work (done, checked, open) is in `claude/status.md`.

Updated on November 2, 2026 (translated into English on November 7).

## Code

- **Symbols in the text catalogue**: the star of *Medium ★* and the arrows
  of the shortcuts (↔) come from Inter, not from the icon font: they are
  fine, but if they become real icons they must move to `symbol.rs`.
- **Buttons of the shared Windows 11 style not used yet**: the style also
  has underlined tabs and the "dangerous" (red) button; Apsophos has no
  tabs or delete confirmations yet. Add them when needed, with the grey
  accent.
- **Viewer drawing details** (comparison handle, outlines) use
  `Color32::WHITE` and hand-written widths. They are part of drawing a
  single glyph, not of the theme: move them to `theme.rs` if the handle is
  redrawn.
- **Hand-written format lists in the catalogue**: `empty_formats` ("TIFF,
  JPEG and PNG up to 32 bit…") and `not_an_image` ("not a TIFF, JPEG or PNG
  image"). The dialogs' filter is now built from `FileFormat::ALL`; these
  two sentences must be reviewed by hand when a format is added (the list
  with "and"/"or" depends on the language).
- **The product name is in `denoise_core::product`**, that is in an
  algorithms crate: it is the only point shared by app and engine (which
  uses it for the TensorRT for RTX cache folder). If the engine becomes a
  library used by other programs, the cache folder must be passed in the
  options.

- **Grey photos with a grey profile** go to the network without
  conversion: if the profile's curve is very different from sRGB's (for
  example a linear grey) the network removes less noise. Convert them like
  the RGB ones if it happens.
- **The sRGB conversion redoes the work for every tile**, also on the
  overlapping margins (about 20–40 % more pixels): it costs milliseconds
  against the network's tenths of a second, so converting the whole image
  once, which would take more memory, is not worth it.

- **Texts still in English in the Italian interface** (since block 14 the
  libraries' errors go through the catalogue, `crate::errors`): the notes
  `denoise_io` writes while reading a file (for example the conversion from
  CMYK or the ignored pages of a TIFF), the decoders' details inside errors
  ("the file is damaged (…)"), the tile-search notes in the *Advanced*
  panel and the details of the worker's internal errors. To translate the
  notes, `denoise_io` should return variants instead of sentences.
- **Plurals with the thousands of the catalogue in use**: `plural` takes
  the separators from `tr()`, so calling another language's catalogue by
  hand (as in the tests) gives the thousands of the language in use. In the
  app it is always the same; to fix if one day two languages are needed
  together.

- **A single model per precision** instead of one per tile size: the files
  repeat the same weights and only the graph's shape changes. It would need
  a graph with dynamic height and width (`tools/scunet_graph.py`) and in
  ONNX Runtime the sizes fixed when the session is created
  (`add_free_dimension_override_by_name` for DirectML, shape profiles for
  TensorRT for RTX). The package would go from about 357 to about 120 MB of
  zip. To try with the self-test on every GPU before adopting it.

## Metadata

- **Extended XMP merged by hand in TIFF and PNG**: these formats keep XMP
  in one piece, so the descriptions of the extended part are inserted into
  the main packet's `rdf:RDF` with a simple text search
  (`jpeg_app::merged`), not with an XML parser. It is enough for Lightroom
  and Photoshop packets; a packet with several `rdf:RDF` or written in an
  unusual way keeps only the main part.
- **The maker note is lost** when EXIF goes into a new file: its internal
  offsets refer to the original file. Lens and exposure are also in the
  standard EXIF tags, which stay.
- **The resolution of JPEGs** is read from the EXIF tags and, if they are
  missing, from the JFIF density; when the two differ EXIF wins, as in most
  programs.

## From the review with the skills (November 5)

- **Real fuzzing** of our parsers (`meta.rs`, `jpeg_app.rs`, `icc.rs`) with
  `cargo-fuzz`: it needs the nightly compiler. Today there are the
  `proptest` tests; fuzzing would find more in less time.
- **"`<decode error>`" in the ONNX Runtime log:** some ONNX Runtime
  messages (for example when DirectML runs out of memory) reach the log as
  unreadable text, probably in Windows' code page and not in UTF-8. It was
  there before; the real error reaches the app anyway.
- **Model hashes:** the models in `models\` are not verified with a hash. A
  manifest with the SHA-256 of the models made by the scripts could warn if
  a model does not match.
- **Heavily compressed JPEGs:** a discreet notice when quantisation is
  strong (block noise is not what the network is trained for).
- **F6 between panels** (Windows convention) and a command to move the
  focus between queue, photo and parameters.
- **Settings:** the file is the serialisation of `Settings`; fields a new
  version does not know are lost on saving. To fix if the settings grow.
- **CI and local check:** the CI calls the cargo commands one by one, the
  local check calls them from `check.ps1`; a shared `cargo xtask check`
  would guarantee they are the same. The release workflow lacks
  `cargo auditable build` (dependencies readable from the binary).
  They also differ in the build: `check.bat` tests in release, the CI in
  debug, where egui's debug assertions run. On November 7 two font tests
  passed locally and failed in the CI (an undrained `TexturesDelta`);
  until the shared command exists, run `cargo test --workspace` (debug)
  too before publishing a branch that touches egui code.

## Enlargement (November 5)

- **EXIF pixel dimensions** (`PixelXDimension`, `PixelYDimension`): the
  enlarged file keeps the original's. Programs use the image's real
  dimensions; updating them requires rewriting a value in the EXIF sub-IFD,
  which today is copied as it is.
- **TIFF and JPEG collected, not streamed:** the `tiff` crate compresses
  only an image written in one go (strips written one at a time come out
  uncompressed under a Deflate tag), so the converted samples are collected
  in memory (6 bytes per pixel in 16 bit). When the crate compresses
  strips, write them in bands; BigTIFF for files over 4 GB (today rejected
  before the work, with the 16-bit suggestion).
- **Preview at the centre of the view:** darktable chooses the point with a
  click; here the view is moved. A picker on the viewer is still to do.
- **DirectML on slower GPUs:** the small tiles (320 px for 2×, 256 for 4×)
  stay below the about 2 s Windows grants a submission on a laptop RTX
  3050; integrated graphics may exceed them and reset the GPU (887A0006).
  Measure the time of the first pass and, if it is close to the limit,
  switch to the CPU saying so, or generate even smaller tiles.
- **Time estimate before starting:** the remaining time appears after the
  first tile; the export settings could show an estimate from the measured
  speed (as for the SCUNet tiles).
- **Queue with enlargement:** for every file SCUNet is loaded first and then
  SwinIR (one network at a time in GPU memory, a few seconds per load with
  the saved engines). If it weighs, group them: first all the noise
  reductions, then all the enlargements.
- **The faithful 2× gains little** over Catmull-Rom in the converter's
  synthetic test (0.04 dB with camera blur; 1.8 dB in the engine test):
  judge it on real photos with the preview. Alternative: SwinIR "classical"
  2× (trained on bicubic downscaling), more faithful on sharp images and
  less on blurred ones.
- **Download not resumed:** if it is interrupted, the complete files stay
  and the half one is deleted; it restarts from that file (HTTP Range not
  used).
- **Proxy:** `ureq` uses only the proxy from the environment variables
  (`HTTPS_PROXY`), not the one set in Windows; with a corporate proxy the
  download fails with a message, and the models can be copied by hand into
  `%LOCALAPPDATA%\Apsophos\models`.
- **Model card** shown only in the export settings; a "Models" item in the
  About window would gather it for all of them.

## Release

- **The installer script has not been compiled yet**
  (`packaging/apsophos.iss`): Inno Setup is missing on this PC, so the first
  try will be the *Release* workflow started by hand from the *Actions* tab
  (it only makes the artifacts). If Inno Setup 6 is installed,
  `scripts\package.bat` also makes the installer locally.
- **Signing**: it needs a code-signing certificate (OV or EV) in the secrets
  `SIGN_PFX` and `SIGN_PASSWORD`; without it, the workflow skips signing and
  SmartScreen warns at the first start. An EV certificate removes the
  warning at once, an OV one after a number of downloads.
- **winget**: the workflow writes the manifests (`dist/winget/` in the
  artifacts) with the installer's address in the release; they must be
  proposed by hand to the `microsoft/winget-pkgs` repository after
  publishing the release, with the identifier `MR.Apsophos`.
- **The CI package has no TensorRT for RTX**: the DLLs are an NVIDIA
  download that requires an account. The NVIDIA package is made locally with
  `scripts\package.bat`.

## Checks

- **Action hashes in the workflows**: Dependabot updates them with a weekly
  pull request; before merging it, look at what changes in the action (the
  release diff), as for a dependency.

- **The CI on GitHub has not given a result yet** (first push on October
  27). The Linux job builds without DirectML and TensorRT for RTX
  (`--no-default-features`); there too Little CMS is compiled from C.
- **The interface crate does not build in cloud sessions** (egui missing):
  the real check after every change is `scripts\check.bat` on Windows. The
  app modules without egui (`color`, `files`, `format`, `inspect`,
  `logging`) and the core, io and engine crates build and are tested there
  too.
- **cargo-deny** runs in CI (`deny.toml`) and in `scripts\check.bat` when
  installed (`cargo install cargo-deny`). If it reports a licence not on the
  list, check that it is compatible with GPL-3.0 before adding it.
  Installed and green since November 2.
- **Four advisories ignored in `deny.toml`**: crates no longer maintained,
  without known vulnerabilities or a successor, all in the Linux-only part
  of eframe 0.37 (`ttf-parser` for winit's Wayland decorations; `zvariant`,
  `zbus_names`, `zvariant_derive` for accessibility over D-Bus). Check again
  when eframe is updated.
