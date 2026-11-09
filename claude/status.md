# Apsophos — status of the work

Updated on November 5, 2026 (language and workflow rows on November 7,
release rows on November 8, the blocks of November 9). It is where work
resumes: last checked commit, work in progress, open points, decisions.
Small postponed things are in `TIDY.md`.

## Last checked commit

- Branch `independent-steps-2026-11-09` (November 9, after `0aa13a9`):
  noise reduction and enlargement are independent. A *Reduce the noise*
  switch in the right panel (`ExportSettings::reduce_noise`, on by
  default); off, the view, the preview, Export and the queue enlarge the
  photo as it was opened, without denoising it first, the sliders are
  disabled and the file name carries only `_2x`/`_4x`. With neither step,
  Export and the queue say there is nothing to do. Checked with the real
  networks by `enlarged_view_without_noise_reduction` (ignored test,
  screenshot `target/screenshots/enlarged-only.png`). Before it, on the
  same day: `b02082b` (the enlargement chosen in the right panel) and
  `0aa13a9` (the viewer shows the enlargement of the area in view,
  decision 2 of `docs/decisions/0001-ai-upscaling.md`).
- `main` @ `101b71a` (November 9): four blocks, each made by a separate
  agent in its own worktree, then rebased on `main` one at a time, checked
  with `scripts\check.bat` (all green: tests in release and in debug,
  Clippy, cargo-deny) and with a green CI of its branch before merging:
  - **Dependencies** (`d0918ad`): the nine Dependabot updates (sysinfo
    0.40, sha2 0.11, md-5 0.11, dirs 7, opener 0.9; checkout 7.0.1,
    setup-python 7.1.0, upload-artifact 7.0.2, action-gh-release 3.0.3).
    sysinfo is wrapped in `crates/app/src/resources.rs`: unknown memory or
    disk space is `None`, the check is skipped and the log says so.
  - **Release** (`400a4d1`, `1581090`): `cargo xtask check` is the one
    command of the checks; `check.bat` and the CI run it (tests in release
    and in debug, Clippy, cargo-deny, required). Chosen over running
    `check.ps1` with `pwsh` in the CI, because locally it runs on Windows
    PowerShell 5.1. The release builds with `cargo auditable` and attaches
    `SHA256SUMS.txt` and the provenance bundle.
  - **Enlargement** (`3fc5244` … `e412f5a`): time estimate in the export
    settings from the measured speed (per model and device, in the
    settings); the model download resumes with HTTP `Range` (`.<name>.part`,
    hash over the whole file, restart when the server does not resume);
    the queue stays file by file (decision 7 of
    `docs/decisions/0001-ai-upscaling.md`); exported files carry their own
    size in the EXIF pixel dimensions.
  - **Interface** (`99b7c05` … `101b71a`): F6 / Shift+F6 between queue,
    photo and parameters; a discreet notice under an estimated JPEG quality
    of 50 (IJG scale, from the luminance quantisation table); a *Models*
    section in the About window (SCUNet and SwinIR cards, installed files);
    the point of the enlargement preview chosen with a click (the centre of
    the view stays the default).
  To try by hand: the estimate before and after an enlargement, a download
  interrupted and resumed, EXIF dimensions of an enlarged JPEG, F6, the
  JPEG notice at quality 30 and 90, Information ▸ Models, the preview
  point.
- Before: the merge of branch `release-package-2026-11-08` (November 8):
  the first release run (tag `v0.1.0`) failed at the winget step because
  `package.bat nonvidia` asked for `onnxruntime_providers_shared.dll`,
  which only the TensorRT for RTX build makes, and then exited with 0
  without a package. Now the DLL is required only with NVIDIA and the
  script exits with 1 when it makes nothing or only part. Checked by
  building the DirectML-only package locally and running its self-test
  (DirectML on the GPU, then the CPU). The repository was published on
  November 7 as one signed commit; `models-1` was built by the Models
  workflow on November 8 (draft, published by the owner).
- Before: the merge of branch `docs-english` (November 7: status and TIDY
  in English, documents only): `scripts\check.bat` all green (156 tests,
  cargo-deny). Merged without branch CI, with the owner's consent: GitHub
  did not start the jobs (account billing, see *Open points*).
- Before: the merge of branch `model-download-2026-11-05` (block 25,
  November 5): `scripts\check.bat` all green (fmt, build, 156 tests,
  Clippy, cargo-deny).
- Before: the merge of branch `upscaler-2026-11-05` (block 24, November 5):
  `check.bat` green (151 tests), full self-test, enlargement included.
- Before: the merge of branch `windows-calls-2026-11-05` (block 23,
  November 5): `check.bat` green (133 tests), full self-test.
- Before: the merge of branch `rules-review-2026-11-05` (block 22,
  November 5): `check.bat` green (128 tests).
- Before: the merge of branch `repo-hardening-2026-11-05` (November 5):
  `scripts\check.bat` all green. Preparation for the public repository:
  - workflows with a read-only token (`permissions: contents: read`; the
    release job has write access for itself only), checkout without kept
    credentials, external actions pinned to the commit hash with the
    version in a comment;
  - `.github/dependabot.yml`: weekly updates of actions and crates, with a
    7-day wait on new versions;
  - `SECURITY.md`: private reports through GitHub (private vulnerability
    reporting), no personal address.
- Before: the merge of branch `lock-update-2026-11-05` (November 5): the
  yanked `async-executor` updated to 1.14.1; `dist\` and the build
  cleaned.
- Before: the merge of branch `ui-style-2026-11-02` (block 21, November
  2): `check.bat` green (112 tests).
- Before: the merge of branch `rust-1.99-2026-11-02` (block 20): `check.bat`
  green with Rust 1.99.1 (110 tests).
- Before: the merge of branch `release-2026-11-02` (block 19): `check.bat`
  green (110 tests); NVIDIA and DirectML packages rebuilt.
- Before: the merge of branch `metadata-2026-11-02` (block 18): `check.bat`
  green (110 tests).
- Before: the merge of branch `pipeline-2026-11-02` (block 17): `check.bat`
  green (104 tests) and full self-test without hangs.
- Before: the merge of branch `quality-test-2026-11-02` (block 16):
  `check.bat` green (104 tests).
- Before: the merge of branch `package-size-2026-11-02` (block 15):
  `check.bat` green (103 tests).
- Before: the merge of branch `italian-2026-11-02` (block 14): `check.bat`
  green (103 tests).
- Before: the merge of branch `ui-tests-2026-11-02` (block 13): `check.bat`
  green (99 tests).
- Before: the merge of branch `filter-repo-2026-11-02` (block 12, history
  rewritten): `check.bat` green (91 tests).
- Before: the merge of branch `eframe-037-2026-11-02` (block 10, November
  2): `check.bat` green with Rust 1.97 (90 tests); package rebuilt.
- Before: the merge of branch `eframe-update-2026-11-02` (block 9, November
  2): `check.bat` green (90 tests, cargo-deny included).
- Before: the merge of branch `planned-2026-11-02` (block 8, November 2):
  `check.bat` green (90 tests, cargo-deny included); package rebuilt.
- Before: the merge of branch `copyright-mr-2026-11-02` (block 7, November
  2): `check.bat` green (90 tests).
- Before: the merge of branch `texts-ui-2026-10-31` (blocks 4–6, November
  2): `check.bat` green (90 tests).
- Before: `6c4c671`, merge of branch `review-2026-10-31` (blocks 1–3,
  October 31): `check.bat` green (82 tests) and full self-test on a laptop
  RTX 3050.

## How the work is done

| What | Where / how |
|---|---|
| Repository | GitHub, repository `Apsophos`, branch `main` (formerly `AIdenoiserRust`, renamed on October 31) |
| Branches | one at a time, from a checked `main`; Claude Code works directly in the working folder |
| After every block | Claude runs `scripts\check.bat`, reads `target\check-summary.txt` and fixes until everything is green |
| Branch CI | the agent publishes the work branch with the command of the general rules and waits for its CI (since November 7) |
| Merge | into `main` only with `scripts\check.bat` all green and the branch CI green |
| Push of `main` and releases | by hand, by the owner |
| Languages | interface, code, comments, commits, status and TIDY in English; Claude's instructions (`CLAUDE.md`) in Italian |
| Models | `models\` of the working folder (FP32 and FP16, tiles 448–1600), not in the repository |
| TensorRT for RTX | 1.4.0.76 in `C:\TensorRT-RTX-1.4.0.76`; `scripts\check.bat` copies its two DLLs next to the program |
| Toolchain | Rust 1.99.1 (`rust-toolchain.toml`) |

## Work in progress

Review with the design skills and alignment with the project instructions,
in blocks. Blocks 1–3 are in branch `review-2026-10-31`, blocks 4–6 in
branch `texts-ui-2026-10-31`, block 7 in branch `licence-cleanup-2026-11-02`
(copyright in `copyright-mr-2026-11-02`), block 8 in branch
`planned-2026-11-02`, block 9 in branch `eframe-update-2026-11-02`, block 10
in branch `eframe-037-2026-11-02`, block 11 in branch `jpeg-xmp-2026-11-02`,
blocks 13–19 in a branch each (`ui-tests-2026-11-02`, `italian-2026-11-02`,
…): all merged into `main`. The working rules in `CLAUDE.md` were rewritten
on November 2 (branch `instructions-2026-11-02`).

1. **Scripts and documents** (this block): `scripts\` with `check`,
   `prepare`, `self-test`, `run`, `package`, `pull-claude`, modelled on
   another project; `tools\build.bat` removed; status and TIDY in
   `claude/stato.md` and `TIDY.md` (in Italian until November 7, now
   `claude/status.md` in English).
   **Name: Apsophos** (ἄψοφος, "without noise"), replacing Rust Denoiser in
   the whole program; the name and what derives from it are in a single
   place, `denoise_core::product`. *Checked: `scripts\check.bat` all green
   on October 31 at `7be8f65` (fmt, build, 74 tests, Clippy; cargo-deny not
   installed).*
2. **Clearer messages**: GPU errors said in words, with the Windows code in
   brackets (`denoise_engine::gpu_error`): out of memory (887A0001,
   8007000E, TensorRT for RTX "out of memory"), GPU reset (887A0005–7),
   DirectML unusable (887A0004). They apply to the tile search ("1216 px
   tiles skipped: not enough GPU memory…"), to the self-test and to the
   reason for falling back to the CPU. `scripts\check.bat` green at
   `c8a5633` (78 tests). The self-test of October 31 froze the PC: DirectML
   at 1216 px gave 887A0001 (new message correct), then the 1600 px attempt
   also filled the shared memory and the PC stopped. Now the self-test, on
   every GPU, stops at the first size that fails and marks the larger ones
   as skipped (the automatic search already did). Checked on October 31:
   `check.bat` green at `6587084`; full self-test without hangs (DirectML:
   1216 px "not enough GPU memory … (error 887A0001)", 1600 px skipped;
   chosen 960 px on DirectML, 1216 px on TensorRT for RTX).
3. **Network in sRGB** (`network_space.rs`): for photos with an RGB profile
   other than sRGB every tile goes to sRGB (Little CMS, in floating point)
   before the network; values outside sRGB are clipped only for the
   network. Only the correction is brought back into the photo's space:
   output = source + (network − network input), both converted back the
   same way, so rounding cancels out and out-of-gamut colours stay. Without
   a profile, with an sRGB profile (recognised by comparing 125 colours,
   tolerance 0.001), grey or unusable: no conversion. It applies to the
   preview and to the queue. `check.bat` green at `46bc75f` (82 tests).
   *To try by hand with an Adobe RGB or ProPhoto photo.*
4. **Text catalogue** (`text.rs`, branch `texts-ui-2026-10-31`): all
   interface texts in a `Texts` structure with one value per language
   (`EN`); fixed texts as `&'static str`, texts with numbers or names as
   functions, so every language chooses order and plurals; `tr()` gives the
   language in use. Adding a language = writing another value, and the
   compiler asks for every text. `format.rs` keeps only numbers and
   durations, with separators and units from the catalogue. Left out:
   library messages, names of formats and devices, self-test, logs,
   details of internal errors. *Checked: `check.bat` green on November 2 at
   `ff12de7` (90 tests).*
5. **`ui.rs` split** by panel into `ui/` (`menus`, `params`, `queue`,
   `status`, `central`, `windows`; common helpers in `ui/mod.rs`), a
   separate `tidy:` commit without behaviour changes; `check.bat` green
   together with the catalogue on October 31 (84 tests). Then
   `ui/present.rs`: from a few facts (`Facts`: image open, complete,
   denoise running, queue, export) it says for every command whether it can
   be used and why not; menus, toolbar, right panel and queue use it, so
   the same command has the same rule everywhere (before, "Add files" was
   enabled in the menu but not in the queue panel during processing). 5
   tests. *Checked: `check.bat` green on November 2 at `ff12de7`.*
6. **Accessibility**: viewer, navigator and histogram have a name and a
   description for screen readers (AccessKit: zoom, view, tiles done;
   median brightness of original and result); the X of the notices has a
   name. Single-key shortcuts (F, 1, N, +, −, \\) can be turned off in the
   Preferences (*Keyboard* section, setting `single_keys_off`): they help
   people using dictation and people who press them by mistake (WCAG
   2.1.4); when off, the menus do not show them and the shortcuts window
   marks them "(off)". *`check.bat` green on November 2 at `ff12de7`; to
   try with Narrator (Win+Ctrl+Enter).*
7. **Licence and personal references** (branch
   `licence-cleanup-2026-11-02`): licence `GPL-3.0-only` instead of
   `GPL-3.0-or-later` in `Cargo.toml`, `LICENSE-EXCEPTION`, README, package,
   `deny.toml` and the *About* window; copyright holder **MR**
   (`Copyright (C) 2026 MR` in `LICENSE-EXCEPTION`, README, package and
   *About*; `authors = ["MR"]`, branch `copyright-mr-2026-11-02`); removed
   from `CLAUDE.md` and from this document name, GitHub user name, local
   path and names of other projects. Deleted outside the repository:
   `dist\` (old RustDenoiser package), `_log\` and the bundles in
   `target\branches\`; removed the already merged branch
   `texts-ui-2026-10-31` and the orphan ref `claude/main`. The old commits
   were then cleaned with the history rewrite (block 12). *`check.bat` green
   on November 2 (90 tests).*
8. **Planned work** (branch `planned-2026-11-02`):
   - **cargo-deny** installed and green: workspace crates `publish = false`
     (private: no licence check on themselves, path dependencies accepted);
     added `Ubuntu-font-1.0` (new SPDX name of the licence of egui's fonts,
     already allowed as `LicenseRef-UFL-1.0`); seven advisories of the
     eframe 0.31 tree ignored with the reason in `deny.toml` (see
     `TIDY.md`).
   - **Two `TIDY.md` items**: the name of the dialogs' filter is built from
     `FileFormat::ALL`; the "Open an image first" notice takes the shortcut
     from `SC_OPEN` (`shortcut_text`, same text as `format_shortcut` outside
     macOS).
   - **Personal paths in the executable**: rustc wrote into it the paths of
     the dependencies' sources (`%USERPROFILE%\.cargo\registry\src`) and of
     the project folder. `scripts\common.ps1` sets `--remap-path-prefix`
     (prefixes `crates` and `apsophos`) for every script that compiles;
     verified: no file of the package contains the user name or a local
     path.
   - **Package rebuilt** (`Apsophos-0.1.0-windows-x64`, 24 files, 576 MB
     zip) with the Little CMS licence and the new copyright notices.
   *`check.bat` green on November 2 (90 tests, cargo-deny included).*
9. **eframe 0.31 → 0.35** and rfd 0.15 → 0.17 (branch
   `eframe-update-2026-11-02`). 0.35 is the last version for Rust 1.92
   (0.37 wants 1.95). Changes: `App::update` split into `logic` (worker
   messages, keys, dropped files, title) and `ui` (which calls `draw` with
   the root `Ui`); panels are `egui::Panel::top/bottom/left/right` and are
   shown inside the root `Ui` (`default_size`, `size_range`); menus with
   `MenuBar`, `ui.close()` instead of `close_menu`;
   `egui_wants_keyboard_input`; zoom with the wheel uses
   `smooth_scroll_delta` (`raw_scroll_delta` no longer exists). The window
   now draws with wgpu (before, OpenGL with glow). The two vulnerabilities
   of quick-xml and `paste` are gone; four unmaintained, Linux-only crates
   remain, ignored with the reason. *`check.bat` green on November 2 (90
   tests, cargo-deny included); the program starts without errors in the
   log. To try the whole interface by hand.*
10. **Rust 1.97 and eframe 0.37** (branch `eframe-037-2026-11-02`):
    `rust-toolchain.toml` and `rust-version` go from 1.92 to 1.97 (eframe
    0.37 wants at least 1.95). egui changes: `DroppedFile::path()` always
    gives a path (empty ones are discarded), `WidgetType` → `egui::Role`,
    `Id::new` → `Id::unique` for the zoom animation, `Ui::id` →
    `Ui::scope_id`. The four exceptions in `deny.toml` are still needed (the
    same Linux-only crates). Package rebuilt (24 files, 580 MB zip), without
    user name or local paths.
    *`check.bat` green on November 2 (90 tests, cargo-deny included); the
    program starts without errors in the log. To try the whole interface by
    hand.*
11. **XMP from JPEG and PNG** (branch `jpeg-xmp-2026-11-02`): `image`
    0.25.10 (already in the lockfile) exposes `xmp_metadata()`;
    `raster::read` reads the packet, removes the
    `http://ns.adobe.com/xap/1.0/` header if the decoder leaves it and
    discards empty packets or packets over 4 MB (the same limit as TIFF
    tags). So XMP goes JPEG → JPEG, JPEG → TIFF and PNG → JPEG/TIFF; the
    information panel marks it as kept. Tests: round trip TIFF → JPEG →
    JPEG and `xmp_packet`. *`check.bat` green on November 2 (91 tests,
    cargo-deny included). To try by hand with a JPEG from Lightroom or from
    a camera with a rating or keywords.*
12. **History rewritten** (branches `history-rewrite-2026-11-02` and
    `filter-repo-2026-11-02`): see *Open points*.
13. **Automatic interface tests** (branch `ui-tests-2026-11-02`,
    `crates/app/src/ui_tests.rs`): the real app in an
    `egui_kittest::Harness` (no window or GPU), which finds the controls in
    the accessibility tree and fails if an input control has no name. Eight
    tests: empty start with the real shortcut in the prompt; File menu with
    *Export as…* and *Close image* disabled without an image; menu that
    closes after a command; dropped image that opens and has a name for
    screen readers; `N` following the single-key setting; `N` typed in a
    text field that does not change the view; zoom with the wheel; queue
    context menu that opens the item and closes. Found and fixed: the queue
    rows had no name for screen readers (now `ListItem` "name, state").
    `ApsophosApp::new` receives the command-line files from `main` instead
    of reading them itself (in tests they would have been the test
    program's arguments). *`check.bat` green on November 2 (99 tests).*
14. **Interface in Italian** (branch `italian-2026-11-02`): complete `IT`
    catalogue next to `EN`; `text::set_language` chooses the language (an
    `AtomicU8`, read by `tr()`), setting `language` (`None` = Windows'
    language, read with `sys-locale`; always English in tests) and a
    *Language* item in the Preferences, which changes the whole interface
    at once. Errors of `denoise_io`, `denoise_core` and `denoise_engine` go
    through `crate::errors`, which gives every variant its catalogue
    sentence (`Errors` section), including the GPU problems recognised by
    `gpu_error::plain` (`GpuProblem::ALL`); the bit depth comes from the
    catalogue. In Italian, thousands with a dot and decimals with a comma.
    Tests: Italian sentences and plurals, language names, I/O and GPU
    errors. *`check.bat` green on November 2 (103 tests). To look at by
    hand: the longest Italian sentences in buttons and panels.*
15. **Lighter package** (branch `package-size-2026-11-02`): every model file
    contains all the weights (about 77 MB in FP32, 38 in FP16); only the
    graph's shape changes. `scripts\package.bat` now includes only the
    sizes the search really chooses: FP32 448 (CPU and small GPUs), 960 and
    1216, FP16 960 and 1216 for TensorRT for RTX; `package.bat full` puts
    back 704, 1600 and the other FP16 (1600 is about 4 % faster than 1216
    where it fits). Zip from 580 to 357 MB. Discarded for now: a single
    model with dynamic sizes (see `TIDY.md`). *`check.bat` green on
    November 2.*
16. **Denoise quality test** (branch `quality-test-2026-11-02`,
    `quality_holds_on_cpu` in `crates/engine/tests/model.rs`): on the CPU,
    deterministic, a synthetic 960 × 640 scene (3 × 2 tiles) with known
    noise. It checks a minimum PSNR with *Strong* (measured 48.1 dB from
    24.5; threshold 47), the presets in order (error *Light* > *Medium* >
    *Strong*) and the absence of seams: with the grid shifted by half a
    tile the result does not change where the images overlap (maximum
    difference per column measured 0.0013; threshold 0.003). It runs only
    if `models\` has the model, so not in CI; about 30 s more in
    `check.bat`. *`check.bat` green on November 2 (104 tests).*
17. **Speed** (branch `pipeline-2026-11-02`). Measured first with a timed
    model in the test: the network is 99–100 % of the time, preparing and
    recomposing tiles about 1 %, so overlapping them with the network is
    not worth the complexity (discarded). The real gain: **DirectML uses
    the FP16 models** like TensorRT for RTX. On the test 0.75 s instead of
    1.29 s (×1.7) with the same error (RMSE 0.0041); in the self-test, half
    the memory: 1216 px tiles fit (before, only 960) and a 24 MP photo goes
    from about 53 s to about 32 s. A DirectML GPU that does not load FP16
    retries with FP32 on the same back end (note in *Advanced*, no notice),
    not when memory is short nor on TensorRT for RTX (another engine build,
    about 15 s). The `nonvidia` package now includes FP16 960 and 1216.
    *`check.bat` green on November 2; full self-test: TensorRT for RTX
    1600 px, DirectML 1216 px, 1600 px skipped for memory (887A0001)
    without freezing the PC.*
18. **Complete metadata** (branch `metadata-2026-11-02`):
    - **Output PNG** written with the `png` crate instead of `image`'s
      encoder: EXIF (eXIf chunk, orientation reset), XMP (iTXt
      `XML:com.adobe.xmp`), ICC profile (only if suitable: grey for grey,
      RGB for RGB) and resolution (pHYs).
    - **Print resolution** (`Dpi` in `SourceInfo`): read from
      XResolution/YResolution/ResolutionUnit of TIFF or EXIF (also in
      centimetres), from the JFIF density of JPEGs and from pHYs of PNGs;
      written in TIFF tags, as JFIF density and as pHYs. In the information
      panel, row *Print resolution* / *Risoluzione di stampa*.
    - **Extended XMP of JPEGs** (`jpeg_app.rs`): read by reassembling the
      APP1 segments of the MD5 named by the main packet, written again with
      the same MD5; a packet too large for one segment (from a TIFF) goes
      behind a small main packet, with the MD5 computed (crate `md-5`).
      TIFF and PNG get a single packet with the two parts joined.
    Tests: PNG with EXIF, XMP, ICC and resolution; 120 kB XMP TIFF → JPEG →
    JPEG → TIFF; resolution in centimetres; JFIF density; missing segments.
    *`check.bat` green on November 2 (110 tests).*
19. **Release** (branch `release-2026-11-02`):
    - **`Release` workflow** (`.github/workflows/release.yml`): with a tag
      `v<version>` (checked against `Cargo.toml`) it regenerates the models
      from KAIR's original weights (SHA-256 checked by
      `export_onnx_model.py`), builds the DirectML and CPU version, makes
      package, installer and winget manifest and attaches them to a
      **draft** release, published by hand. Started by hand it only makes
      the artifacts.
    - **Signing** (`scripts/sign.ps1`): signtool with a timestamp, only if
      the repository has the secrets `SIGN_PFX` and `SIGN_PASSWORD`.
    - **Installer** (`packaging/apsophos.iss`, Inno Setup 6): per user,
      without administrator, in `%LOCALAPPDATA%\Programs\Apsophos`, Start
      menu, uninstall, English and Italian; `package.bat` makes it when it
      finds Inno Setup.
    - **winget** (`scripts/winget-manifest.ps1`): the three manifests with
      the installer's address and SHA-256, to be proposed by hand.
    Tried locally: DirectML package (278 MB zip), winget manifest, workflow
    syntax. *Not tried: building the installer (Inno Setup missing) and
    the workflow on GitHub.* `check.bat` green on November 2 (110 tests).
20. **Rust 1.99.1** (branch `rust-1.99-2026-11-02`): `rust-toolchain.toml`
    and `rust-version` from 1.97 to 1.99.1, the latest stable (October 13,
    2026). Clippy's new `chunks_exact_to_as_chunks` lint asked for
    `as_chunks::<N>()` instead of `chunks_exact(N)` in nine places
    (blending, metadata, pixel reading and writing, viewer colour): the
    items become arrays, and in `blend.rs` blending writes the pixel's
    array directly. *`check.bat` green on November 2 (110 tests, quality
    test included).*
21. **Interface aligned with another project's Windows 11 style** (branch
    `ui-style-2026-11-02`), except the colours: the dark grey theme
    stays, because the only colour on screen must be the photo (owner's
    choice, November 2). Taken over:
    - **Fonts**: Segoe UI and Segoe UI Semibold and the Segoe MDL2 Assets
      icons, read from Windows' fonts folder (`platform.rs`, never included:
      the licence does not allow it); behind them Inter (included), then
      egui's fonts.
    - **Sizes** (`theme.rs`): spacings `SPACE_XS/S/M/L` 4/8/16/24, `ROW_GAP`
      6, controls 24 high (`CONTROL_HEIGHT`, the primary button too), texts
      12/14/20, corners 4 (controls) and 8 (windows and menus), shadows of
      windows and popups; the colours have the shared names (`bg`,
      `surface`, `text`, `control`, `selected`…) with grey values.
    - **Components** (`ui/widgets.rs`): primary button, wide button with
      centred text, centred dialogs (Preferences, Export, Shortcuts,
      About), filled checkboxes, icon button (close notice), list items with
      the accent mark (queue rows; the one open in the viewer selected).
    - **Icons** (`symbol.rs`): MDL2 codes, with a Unicode fallback symbol
      when the icon font is missing (Linux). Two tests: the MDL2 font has
      every icon; the included fonts have every fallback. They found four
      symbols that with egui 0.37 would have been little squares (ℹ ✔ ✕ ◐,
      and ▸ in the text "File ▸ Export as…", now "›").
    - **Screenshot**: `cargo test -p apsophos --release screenshot --
      --ignored` draws the window with wgpu into
      `target\screenshots\window.png` (it needs a GPU, so it is outside
      `check.bat`).
    *`check.bat` green on November 2 (112 tests). To look at by hand: the
    window with a real photo, the Italian sentences in the buttons.*
22. **Review with the new rules and skills** (branch
    `rules-review-2026-11-05`, November 5). The sources' notes have no
    section for Apsophos; those written for the previous project of the
    same kind (Rust, egui, SCUNet) apply, already almost all respected:
    tiles with a margin and seam-free tests, view of the removed noise,
    strength as blending, profile kept, worker panic as an error, atomic
    writing, `opt-level = 3` for dependencies in debug. Changed:
    - **Instructions:** `CLAUDE.md` lists the skills (always and by topic)
      and three exceptions with the reason: grey interface, text catalogue
      in Rust instead of Fluent, path remapping in the scripts. This PC's
      notes in `CLAUDE.local.md`, ignored by git.
    - **Threat model** in `docs/threat-model.md`: opened files, models,
      DLLs, settings, release; accepted risks with the reason.
    - **Build:** Control Flow Guard and CET compatibility in
      `.cargo/config.toml` (verified on the executable); the remap now goes
      through the Windows target variable, which Cargo merges with those
      flags; overflow checks in release for `denoise_core` and
      `denoise_io`; cargo-deny rejects *unsound* crates everywhere.
    - **Numeric conversions:** the lints `cast_possible_truncation`,
      `cast_sign_loss`, `cast_possible_wrap` turned on; conversions go
      through `denoise_core::num` (round or truncate, then clamp, NaN to the
      lower bound) or through quantisation; removed 9 `#[allow]` of lints
      that were off, the others became `#[expect(reason)]`.
    - **ONNX Runtime:** telemetry off, threads without spinning.
    - **Writing:** the temporary file is created exclusively and never
      truncates another file (test).
    - **Zoom:** Ctrl++, Ctrl+− and Ctrl+0 act on the photo, not on the
      interface (test seen failing without the fix).
    - **Robustness:** `proptest` tests on random bytes and damaged valid
      data for our parsers (TIFF/EXIF directories, JPEG segments, ICC
      headers); a compile-time check that the worker's messages are `Send`.
    *`check.bat` green on November 5 (128 tests); the program starts.*
23. **Windows calls** (branch `windows-calls-2026-11-05`): new crate
    `denoise_platform`, the only one with `unsafe` (two blocks, each with
    its `SAFETY:`; its own lints: `unsafe` denied except where a function
    says why, `undocumented_unsafe_blocks` on).
    - **DLL search:** the first statement of `main` limits the search for
      DLLs loaded by name to the program's folder and System32
      (`SetDefaultDllDirectories`). Full self-test: TensorRT for RTX and
      DirectML load as before.
    - **Durable rename:** atomic writing renames with
      `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` on
      Windows, with paths in the `\\?\` form (also beyond 260 characters,
      test) and those with NUL rejected; on Linux rename and folder sync.
    - **Fix of block 22:** spinning turned off slowed the CPU from 27 to
      42 s/MP (self-test 44.5 against 19.8). Now it is off only for GPU
      sessions, where the CPU threads wait for the card; the CPU is back to
      25.7 s/MP, the GPU unchanged.
    *`check.bat` green on November 5 (133 tests); full self-test.*
24. **2× and 4× enlargement with a network** (branch `upscaler-2026-11-05`;
    design and measurements in `docs/decisions/0001-ai-upscaling.md`).
    Owner's choices: faithful variant plus the labelled generative one,
    SwinIR-L for 4×, weights downloaded from GitHub (SwinIR, Apache-2.0).
    - **Converter** `tools/export_upscaler_model.py`: accepts only the four
      weight files whose SHA-256 it knows; the graph matches a NumPy pass
      written like the authors' code (`swinir_reference.py`, difference
      ≤ 6·10⁻⁶) and the faithful models beat Catmull-Rom on a test image
      with camera blur. Swin blocks shared with SCUNet (`onnx_graph.py`; the
      SCUNet graph gives the same result to the last bit).
    - **Measurements that changed the design:** 64 px margin (with 32 the
      seams reach 76 levels out of 255 in textures); attention on 4-D
      tensors (DirectML rejects some 5-D products); residual convolutions
      in FP32 in the FP16 variant (SwinIR-M reaches 474,000, beyond the FP16
      maximum); two tile sizes per model, because Windows resets the GPU if
      DirectML keeps it busy for more than about 2 s with one tile.
    - **Core and writing:** `TileModel` with a scale factor, `Bands` (one
      row of tiles at a time), Catmull-Rom as the reference; `write_from`
      writes an image produced in bands (streaming PNG, TIFF and JPEG
      collect the converted samples), with the formats' limits checked
      before the work.
    - **Program:** "Enlargement" in the export settings (none, 2×, 4×;
      faithful or "Generated details", with a notice and the `-gen`
      suffix), model card, result size, "Export 2×" button, progress with
      remaining time and "Cancel export", memory and disk space checked
      first, one network at a time in GPU memory, the queue that enlarges
      after noise reduction, before/after preview of the tile at the centre
      of the view with a divider. The self-test enlarges an image on every
      GPU.
    *`check.bat` green on November 5 (151 tests); full self-test.*
25. **Enlargement models downloaded by the program** (branch
    `model-download-2026-11-05`; owner's decision: no models in the
    installer, download after start with a button).
    - **Prompt at start:** if no faithful model is installed, a notice says
      what they are for and to press «Download the models» (192 MB); there
      is also «Don't ask again». Nothing is downloaded without the button.
    - **Export settings:** in place of the missing choices, the button
      «Download the models (N MB)»; after the faithful ones, separately,
      the one for the generated-details models. Progress in bytes, «Cancel
      download», also in the status bar; at the end of the download the
      list is read again.
    - **Security:** a single fixed address (the project's `models-1`
      release), HTTPS only with TLS and Windows' certificates (`ureq` with
      SChannel); size and SHA-256 of every file in the program
      (`models/upscalers.txt`, also read by the workflow): a different file
      is deleted and rejected; never more bytes than expected; time limits;
      writing alongside and durable rename in
      `%LOCALAPPDATA%\Apsophos\models` (not in the roaming profile).
      Tried a real request: «not published yet» (404), no file.
    - **FP16 only:** the engine now accepts any subset of files (FP32, FP16
      or both); the GPU uses FP16, the CPU FP32 if present, otherwise FP16.
      The conversion is deterministic (two runs, same bytes).
    - **`models.yml` workflow** (by hand): converts the SwinIR weights,
      checks the files against `models/upscalers.txt` and attaches them to
      the `models-1` draft.
    *`check.bat` green on November 5 (156 tests).*

## Open points

- **Release 0.1.0** (November 8): the *Release* run of tag `v0.1.0`
  (`6a3fb95`) made the draft with the DirectML zip and installer; the
  *Models* run made the `models-1` draft. Both drafts are published by
  the owner; until `models-1` is published the in-app download fails. The
  blocks of November 9 come after the tag and go in the next version.
- **SCUNet regenerated with the new builder:** the graph now uses 4-D
  attention (same result to the last bit on the CPU, with fake weights).
  The models in `models\` are still the old ones; the release workflow
  regenerates them: before the next release regenerate them locally (the
  KAIR weights are needed) and run the self-test again.

- Since November 2 the work continues with Claude Code on the PC
  (instructions in `CLAUDE.md`); removed `scripts\pull-claude.bat`, which
  served only the cloud sessions.
- To try by hand after eframe 0.37 (menus, queue, keys, dragging and the
  wheel now have automatic tests): resizable panels (saved widths may
  restart from the defaults), touchpad pinch, rendering with wgpu, a real
  screen reader (Narrator).
- To try by hand: the new package on another PC, view of the removed noise
  (`N`), histogram following the sliders after *Denoise*, logs folder,
  *Display* row in the information panel, adding a large folder to the
  queue.
- Cleanup of November 2: deleted the old folders `%APPDATA%\rust-denoiser`
  and `%LOCALAPPDATA%\RustDenoiser` (about 300 MB, mostly the old TensorRT
  for RTX cache) and, in the project, the already zipped package folders in
  `dist\`. `target\release` (about 12 GB) stays: `cargo clean` empties it,
  but the next check recompiles everything.

## Measured performance (October 26, laptop RTX 3050)

- TensorRT for RTX FP16: 0.42–0.84 s/MP; a 24 MP photo in about 13 s
  (1216 px tiles).
- DirectML: 1.9–2.9 s/MP, about 53 s for 24 MP (960 px tiles; 1216 and 1600
  do not fit in GPU memory and are skipped). Since November 2 in FP16:
  1.1–2.2 s/MP, about 32 s for 24 MP (1216 px tiles; 1600 skipped).
- CPU: 17–28 s/MP.
- Enlargement (November 5, self-test, 1024 × 768 for the whole PNG export):
  - TensorRT for RTX FP16: 2× 12.3 s per input megapixel (a 12 MP photo in
    about 2.5 min), 4× 26 s (12 MP in about 5 min); model load 4–5 s with
    the saved engines, about 30 s the first time.
  - DirectML FP16 (small tiles): 2× 47 s/MP, 4× 100 s/MP (12 MP in about
    20 min).
  - CPU: not measured in the self-test (a 640 px 2× tile takes about 40 s).

## Decisions

- Work branches go to GitHub before merging (November 7): the agent
  publishes only the current work branch, with the command of the general
  rules (never `main`, never forced, no tags), and waits for its CI; it
  merges into `main` only with `scripts\check.bat` and the branch CI green.
  `main`, tags and releases are pushed by the owner.
- Status and TIDY in English (November 7), like everything that goes to
  GitHub; only Claude's instructions stay in Italian.
- Enlargement models outside the installer: the program downloads them
  after start, only when the user presses the button, and verifies them
  with the compiled SHA-256 values (November 5).
- Interface in a Windows 11 style shared with another project (fonts,
  icons, sizes, components), but always dark and in greys (November 2).
- No choice of the monitor's ICC profile (removed on October 26): the
  screen is treated as sRGB; photos with their own profile are converted to
  sRGB only for display.
- The `.pth` → ONNX converter does not depend on the original Python
  project: the two projects share nothing.
- `unsafe` code only in `denoise_platform` (block 23, exception in
  `CLAUDE.md`); the rest of the workspace has `unsafe_code = "forbid"`.
- Licence `GPL-3.0-only` (November 2), without names or personal data in
  code, documents and metadata; copyright holder the initials MR.
- 8-bit output is dithered only if the source had more bits: an 8-bit file
  rewritten without changes stays identical.
- Name **Apsophos** (October 31). Galene, the first one chosen, is already
  an open-source videoconferencing server (galene.org, package `galene` on
  openSUSE and NixOS); for Apsophos the search found no software.
  Executable `apsophos.exe`, data folders `Apsophos`, settings key and app
  id `apsophos`, variable `APSOPHOS_MODEL`. Settings saved under the old
  name are not taken over (start from the defaults, once); the GitHub
  repository was renamed `Apsophos`.
- Branch flow: commits and merges by Claude in the working folder; the
  agent publishes work branches for their CI, the owner pushes `main`.
- Checks in `scripts\check.bat` (fast); preparation and self-test in
  separate scripts.
