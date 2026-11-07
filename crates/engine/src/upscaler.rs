//! The SwinIR upscaling models (real-world super-resolution).
//!
//! Files `swinir_real_x<2|4>_<psnr|gan>_<tile>.onnx`, made by
//! `tools/export_upscaler_model.py`, and their `_fp16` variants; either
//! precision may be missing (the program downloads only the FP16 files: the
//! GPU runs them, the CPU too, more slowly). Each model comes in two tile
//! sizes.
//! TensorRT for RTX takes the large one. DirectML takes the small one:
//! Windows resets a GPU that works more than about 2 s on one submission,
//! and DirectML submits a whole tile at once (a 2× tile of 384 px or a 4×
//! tile of 320 px already stalls an RTX 3050 laptop). The program asks for an
//! [`Upscale`] and never for a file name; [`card`] says what the model is.

use std::path::{Path, PathBuf};

use denoise_core::upscale::{Detail, Factor, Upscale};
use denoise_core::{ModelError, TileModel};

use crate::{Device, EngineError, EngineOptions, Gpu, ModelFiles, Network};

/// Input pixels discarded along each edge of an upscaler tile. Measured on
/// fine stripes, against the whole image: with 32 px the tiles differ from it
/// by up to 76 levels of 255 (a visible seam), with 48 px by up to 39, with
/// 64 px by up to 10 (0.05 on average).
pub const UPSCALE_MARGIN_PX: usize = 64;

/// What an upscaling model file holds, read from its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpscalerFile {
    pub upscale: Upscale,
    /// Side of the input tile.
    pub tile: usize,
    /// The half-precision (FP16) variant.
    pub half: bool,
}

/// What a model file holds, read from its name; `None` for any other file.
pub fn upscaler_file_kind(path: &Path) -> Option<UpscalerFile> {
    if path.extension()? != "onnx" {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let (stem, half) = match stem.strip_suffix("_fp16") {
        Some(s) => (s, true),
        None => (stem, false),
    };
    let rest = stem.strip_prefix("swinir_real_x")?;
    let (factor, rest) = rest.split_once('_')?;
    let factor = match factor {
        "2" => Factor::X2,
        "4" => Factor::X4,
        _ => return None,
    };
    let (detail, tile) = rest.split_once('_')?;
    let detail = match detail {
        "psnr" => Detail::Faithful,
        "gan" => Detail::Generated,
        _ => return None,
    };
    let tile = tile.parse::<usize>().ok().filter(|&t| t.is_multiple_of(8) && t > 2 * UPSCALE_MARGIN_PX)?;
    Some(UpscalerFile { upscale: Upscale { factor, detail }, tile, half })
}

/// The files of one tile size of an upscaling model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpscalerTile {
    pub tile: usize,
    pub files: ModelFiles,
}

/// The tile sizes of the model for `upscale` found among `files` (the first
/// file of each name wins), largest first.
pub fn upscaler_files(files: &[PathBuf], upscale: Upscale) -> Vec<UpscalerTile> {
    let mut found: Vec<UpscalerTile> = Vec::new();
    for path in files {
        let Some(kind) = upscaler_file_kind(path).filter(|k| k.upscale == upscale) else { continue };
        let i = match found.iter().position(|t| t.tile == kind.tile) {
            Some(i) => i,
            None => {
                found.push(UpscalerTile { tile: kind.tile, files: ModelFiles::default() });
                found.len() - 1
            }
        };
        let slot = if kind.half { &mut found[i].files.fp16 } else { &mut found[i].files.fp32 };
        slot.get_or_insert_with(|| path.clone());
    }
    found.sort_by_key(|t| std::cmp::Reverse(t.tile));
    found
}

/// What the interface says about a model: the network, where it comes
/// from, its licence and the data it was trained on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelCard {
    pub network: &'static str,
    pub source: &'static str,
    pub licence: &'static str,
    pub training_data: &'static str,
}

/// The card of the model for `upscale`.
pub const fn card(upscale: Upscale) -> ModelCard {
    const SOURCE: &str = "github.com/JingyunLiang/SwinIR, release v0.0";
    match upscale.factor {
        Factor::X2 => ModelCard {
            network: "SwinIR-M, real-world super-resolution",
            source: SOURCE,
            licence: "Apache-2.0",
            training_data: "DIV2K, Flickr2K, OST",
        },
        Factor::X4 => ModelCard {
            network: "SwinIR-L, real-world super-resolution",
            source: SOURCE,
            licence: "Apache-2.0",
            training_data: "DIV2K, Flickr2K, OST, WED, FFHQ, Manga109, SCUT-CTW1500",
        },
    }
}

/// A SwinIR upscaling model, loaded and ready.
pub struct Upscaler {
    net: Network,
    upscale: Upscale,
}

impl Upscaler {
    /// Loads the model for `upscale` from `files` (the model folder's
    /// files), on the device `options` ask for: TensorRT for RTX with the
    /// largest tile, else DirectML with the smallest, else the CPU with the
    /// largest. On a GPU the model is run twice on a blank tile (a stall
    /// shows on the second run) before it is used; a GPU that fails is
    /// skipped, and [`Upscaler::gpu_fallback`] says why.
    ///
    /// # Errors
    /// [`EngineError::Missing`] if there is no file for `upscale`, or an
    /// error if the model cannot be loaded even on the CPU.
    pub fn load(files: &[PathBuf], upscale: Upscale, options: &EngineOptions) -> Result<Self, EngineError> {
        let found = upscaler_files(files, upscale);
        let (Some(large), Some(small)) = (found.first(), found.last()) else {
            return Err(EngineError::Missing(format!("upscaling model {upscale:?}")));
        };
        let scale = upscale.factor.value();
        let mut failures = Vec::new();
        for gpu in options.device.gpu_order() {
            let (model, device) = match gpu {
                Gpu::Rtx => (large, Device::TensorRtRtx),
                Gpu::DirectMl => (small, Device::DirectMl),
            };
            match Network::load_files(&model.files, model.tile, scale, &EngineOptions { device, ..*options }) {
                Ok(mut net) if net.on_gpu => match net.probe().and_then(|()| net.probe()) {
                    Ok(()) => {
                        net.gpu_fallback = (!failures.is_empty()).then(|| failures.join("; "));
                        return Ok(Self { net, upscale });
                    }
                    Err(e) => failures.push(format!("{}: {}", net.device, e.0)),
                },
                Ok(net) => failures.extend(net.gpu_fallback),
                Err(e) => failures.push(format!("{}: {e}", gpu.name())),
            }
        }
        // the CPU: the largest tile with an FP32 file, else the largest
        let cpu = found.iter().find(|t| t.files.fp32.is_some()).unwrap_or(large);
        let cpu_options = EngineOptions { device: Device::Cpu, ..*options };
        let mut net = Network::load_files(&cpu.files, cpu.tile, scale, &cpu_options)?;
        net.gpu_fallback = (!failures.is_empty()).then(|| failures.join("; "));
        Ok(Self { net, upscale })
    }

    pub fn upscale(&self) -> Upscale {
        self.upscale
    }

    /// `true` if the network runs on a GPU.
    pub fn on_gpu(&self) -> bool {
        self.net.on_gpu
    }

    /// Device in use, for the interface (e.g. "GPU (DirectML, FP16)").
    pub fn device(&self) -> &str {
        &self.net.device
    }

    /// Why the GPU was requested but not used, if that happened.
    pub fn gpu_fallback(&self) -> Option<&str> {
        self.net.gpu_fallback.as_deref()
    }

    /// Notes from loading (e.g. FP16 not usable, FP32 used).
    pub fn notes(&self) -> &[String] {
        &self.net.notes
    }
}

impl TileModel for Upscaler {
    fn input_px(&self) -> usize {
        self.net.tile_px
    }

    fn margin_px(&self) -> usize {
        UPSCALE_MARGIN_PX
    }

    fn scale(&self) -> usize {
        self.net.scale
    }

    fn run(&mut self, planar_in: &[f32], planar_out: &mut [f32]) -> Result<(), ModelError> {
        self.net.run(planar_in, planar_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const X2_FAITHFUL: Upscale = Upscale { factor: Factor::X2, detail: Detail::Faithful };
    const X4_GENERATED: Upscale = Upscale { factor: Factor::X4, detail: Detail::Generated };

    #[test]
    fn file_names_say_what_the_model_is() {
        let kind = |upscale, tile, half| Some(UpscalerFile { upscale, tile, half });
        assert_eq!(upscaler_file_kind(Path::new("m/swinir_real_x2_psnr_512.onnx")), kind(X2_FAITHFUL, 512, false));
        assert_eq!(upscaler_file_kind(Path::new("swinir_real_x4_gan_384.onnx")), kind(X4_GENERATED, 384, false));
        assert_eq!(upscaler_file_kind(Path::new("swinir_real_x4_gan_384_fp16.onnx")), kind(X4_GENERATED, 384, true));
        for other in [
            "swinir_real_x4_gan_384_fp32.onnx",
            "swinir_real_x3_psnr_512.onnx",
            "swinir_real_x2_other_512.onnx",
            "swinir_real_x2_psnr_500.onnx",
            "swinir_real_x2_psnr_128.onnx",
            "swinir_real_x2_psnr_512.onnx.tmp",
            "scunet_color_real_448.onnx",
        ] {
            assert_eq!(upscaler_file_kind(Path::new(other)), None, "{other}");
        }
    }

    #[test]
    fn the_files_are_found_by_what_they_do() {
        let files: Vec<PathBuf> = [
            "swinir_real_x2_psnr_320.onnx",
            "swinir_real_x2_psnr_640_fp16.onnx",
            "swinir_real_x2_psnr_320_fp16.onnx",
            "x.onnx",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let pair = |fp32: Option<usize>, fp16: Option<usize>| ModelFiles {
            fp32: fp32.map(|i| files[i].clone()),
            fp16: fp16.map(|i| files[i].clone()),
        };
        assert_eq!(
            upscaler_files(&files, X2_FAITHFUL),
            vec![
                UpscalerTile { tile: 640, files: pair(None, Some(1)) },
                UpscalerTile { tile: 320, files: pair(Some(0), Some(2)) },
            ]
        );
        assert!(upscaler_files(&files, X4_GENERATED).is_empty());
        assert!(matches!(
            Upscaler::load(&files, X4_GENERATED, &EngineOptions::default()),
            Err(EngineError::Missing(_))
        ));
        assert_eq!(card(X4_GENERATED).licence, "Apache-2.0");
    }
}
