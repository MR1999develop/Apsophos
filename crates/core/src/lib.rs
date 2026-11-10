//! Core of Apsophos.
//!
//! Pure algorithms only, with no user interface and no inference engine:
//!
//! * [`Image`]: floating-point RGB image (nominal values 0..=1, encoded as in
//!   the file: no colour-space conversion);
//! * [`TilePlan`] and [`run_tiles`]: split into overlapping tiles and apply a
//!   neural network seen through the [`TileModel`] trait;
//! * [`Mixer`] and [`blend`]: how much of the correction to apply, separately
//!   for luminance (grain) and colour (colour blotches);
//! * [`denoise`]: the whole procedure in one call.
//!
//! SCUNet is a "blind" network: it estimates the noise level by itself. The
//! network output is therefore computed once, and the parameters only act on
//! the blending, which is instantaneous.
#![cfg_attr(not(test), warn(clippy::cast_possible_truncation))]

mod blend;
mod error;
mod image;
pub mod num;
mod params;
pub mod product;
pub mod quant;
mod tiling;

pub use blend::{blend, blend_region, luma, Mixer};
pub use error::{DenoiseError, ModelError};
pub use image::{Image, PixelRect};
pub use params::{Params, Percent, Preset};
pub use tiling::{run_tiles, JobControl, TileModel, TileOutput, TilePlan};

/// Reduces the noise of `img` with `model` and applies `params`.
///
/// Tile size and overlap are decided by the model; tiles are processed in
/// reading order. `progress` receives `(done, total)` after every tile.
///
/// # Errors
/// [`DenoiseError::Cancelled`] if `control` is cancelled,
/// [`DenoiseError::Model`] if the network fails.
pub fn denoise(
    img: &Image,
    params: &Params,
    model: &mut dyn TileModel,
    progress: &mut dyn FnMut(usize, usize),
    control: &JobControl,
) -> Result<Image, DenoiseError> {
    let raw = network_output(img, model, progress, control)?;
    Ok(blend(img, &raw, params))
}

/// Raw network output for the whole image, without blending.
///
/// # Errors
/// As [`denoise`].
pub fn network_output(
    img: &Image,
    model: &mut dyn TileModel,
    progress: &mut dyn FnMut(usize, usize),
    control: &JobControl,
) -> Result<Image, DenoiseError> {
    let plan = TilePlan::for_model(img.width(), img.height(), model)?;
    let order: Vec<usize> = (0..plan.len()).collect();
    let mut raw = img.clone();
    run_tiles(
        img,
        model,
        &plan,
        &order,
        &mut |t: TileOutput<'_>| {
            raw.paste(t.pixels, t.rect.x, t.rect.y);
            progress(t.done, t.total);
        },
        control,
    )?;
    Ok(raw)
}
