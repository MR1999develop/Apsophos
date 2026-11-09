//! The networks the program uses, with their cards and the files of each
//! found on this computer, for the About window. Pure: the caller reads the
//! folders (once per opening of the window, never on every frame).

use std::path::{Path, PathBuf};

use denoise_core::upscale::{Detail, Factor, Upscale};
use denoise_engine::ModelCard;

/// One network: its card, and its files grouped by folder (names sorted);
/// empty when it is not installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedModel {
    pub card: ModelCard,
    pub folders: Vec<(PathBuf, Vec<String>)>,
}

/// The denoising network, then the 2× and 4× enlargement, each with the
/// files among `denoiser` and `upscalers` that belong to it.
pub fn list(denoiser: &[PathBuf], upscalers: &[PathBuf]) -> Vec<ListedModel> {
    let upscaler = |factor: Factor| {
        let files: Vec<&PathBuf> = upscalers
            .iter()
            .filter(|p| denoise_engine::upscaler_file_kind(p).is_some_and(|k| k.upscale.factor == factor))
            .collect();
        let card = denoise_engine::card(Upscale { factor, detail: Detail::Faithful });
        ListedModel { card, folders: by_folder(&files) }
    };
    let denoiser: Vec<&PathBuf> = denoiser.iter().collect();
    vec![
        ListedModel { card: denoise_engine::DENOISER_CARD, folders: by_folder(&denoiser) },
        upscaler(Factor::X2),
        upscaler(Factor::X4),
    ]
}

/// File names grouped by their folder, folders in the order first met.
fn by_folder(files: &[&PathBuf]) -> Vec<(PathBuf, Vec<String>)> {
    let mut out: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for path in files {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match out.iter_mut().find(|(d, _)| *d == dir) {
            Some((_, names)) if !names.contains(&name) => names.push(name),
            Some(_) => {}
            None => out.push((dir, vec![name])),
        }
    }
    for (_, names) in &mut out {
        names.sort();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_go_to_their_network_and_folder() {
        let denoiser =
            [PathBuf::from("app/models/scunet_color_real_960.onnx"), "app/models/scunet_color_real_448.onnx".into()];
        let upscalers = [
            PathBuf::from("data/models/swinir_real_x4_psnr_256_fp16.onnx"),
            "data/models/swinir_real_x2_psnr_320_fp16.onnx".into(),
            "app/models/swinir_real_x2_gan_320_fp16.onnx".into(),
            "data/models/notes.txt".into(),
        ];
        let listed = list(&denoiser, &upscalers);
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].card, denoise_engine::DENOISER_CARD);
        assert_eq!(
            listed[0].folders,
            vec![(
                PathBuf::from("app/models"),
                vec!["scunet_color_real_448.onnx".to_owned(), "scunet_color_real_960.onnx".to_owned()]
            )]
        );
        // 2×: faithful and generated detail, in two folders
        assert_eq!(
            listed[1].card.network,
            denoise_engine::card(Upscale { factor: Factor::X2, detail: Detail::Faithful }).network
        );
        assert_eq!(listed[1].folders.len(), 2);
        assert_eq!(
            listed[2].folders,
            vec![(PathBuf::from("data/models"), vec!["swinir_real_x4_psnr_256_fp16.onnx".to_owned()])]
        );
    }

    #[test]
    fn nothing_installed() {
        assert!(list(&[], &[]).iter().all(|m| m.folders.is_empty()));
        // the same file found twice is listed once
        let twice = [PathBuf::from("m/scunet_color_real_448.onnx"), "m/scunet_color_real_448.onnx".into()];
        assert_eq!(list(&twice, &[])[0].folders[0].1.len(), 1);
    }
}
