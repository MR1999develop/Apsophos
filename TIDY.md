# To fix later

Small issues found in the reviews of October 2026 and postponed until that
code changes again. Every item says why it waits. The overall state of the
work (done, checked, open) is in `claude/status.md`.

Updated on November 2, 2026 (translated into English on November 7; the
items done on November 9 and 10 removed, new ones added).

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
- **Model hashes in the program:** the release lists the SHA-256 of the
  package and of its SCUNet models (`SHA256SUMS.txt`), but the program
  does not check the models in `models\` against them.
- **Settings:** the file is the serialisation of `Settings`; fields a new
  version does not know are lost on saving. To fix if the settings grow.

## Writing files (November 10)

- **The writers still take an image in bands** (`BandSource` in
  `denoise_io`): the bands came from the AI enlargement, removed on
  November 10; now every file is written from one image in memory, as a
  single band. Simplify the TIFF, JPEG and PNG writers to take an `Image`
  when they change again.
- **The model cards are in English** in the Italian interface (they come
  from the engine).

## Interface (November 9)

- **JPEG quality threshold**: the notice appears below an estimated
  quality of 50 (IJG scale); judge it on real photos.
- **F6 with the queue running** goes to "Add files…", which is disabled, so
  the focus does not arrive; it should go to the first enabled control.
- **The slider of the preview window** has no accessible name.
- **Unknown memory or disk space** (the system does not answer) is only in
  the log: the checks are skipped. Say it in the interface too.

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

- **Pinned tool versions outside Dependabot**: cargo-deny 0.20.2 (CI) and
  cargo-auditable 0.7.7 (release) are installed at a fixed version that
  Dependabot does not update: check them by hand from time to time.
- **No daily dependency scan** and no `cargo audit bin` on the released
  program; `SHA256SUMS.txt` is covered only by the provenance attestation,
  not signed on its own.
- **Shared build folder between worktrees gives false results**: cargo
  names the workspace crates by their relative path, so two worktrees with
  one `CARGO_TARGET_DIR` reuse each other's builds. One folder per worktree.

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
- **cargo-deny** runs in `cargo xtask check` (`deny.toml`), so in CI and in
  `scripts\check.bat`; it is required (`cargo install --locked cargo-deny`). If it reports a licence not on the
  list, check that it is compatible with GPL-3.0 before adding it.
  Installed and green since November 2.
- **Four advisories ignored in `deny.toml`**: crates no longer maintained,
  without known vulnerabilities or a successor, all in the Linux-only part
  of eframe 0.37 (`ttf-parser` for winit's Wayland decorations; `zvariant`,
  `zbus_names`, `zvariant_derive` for accessibility over D-Bus). Check again
  when eframe is updated.
