//! The network the program uses, with its card and its files found on
//! this computer, for the About window. Pure: the caller reads the
//! folders (once per opening of the window, never on every frame).

use std::path::{Path, PathBuf};

use denoise_engine::ModelCard;

/// One network: its card, and its files grouped by folder (names sorted);
/// empty when it is not installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedModel {
    pub card: ModelCard,
    pub folders: Vec<(PathBuf, Vec<String>)>,
}

/// The denoising network, with its files among `denoiser`.
pub fn list(denoiser: &[PathBuf]) -> Vec<ListedModel> {
    let denoiser: Vec<&PathBuf> = denoiser.iter().collect();
    vec![ListedModel { card: denoise_engine::DENOISER_CARD, folders: by_folder(&denoiser) }]
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
    fn files_go_to_their_folder() {
        let denoiser = [
            PathBuf::from("app/models/scunet_color_real_960.onnx"),
            "app/models/scunet_color_real_448.onnx".into(),
            "data/models/scunet_color_real_448_fp16.onnx".into(),
        ];
        let listed = list(&denoiser);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].card, denoise_engine::DENOISER_CARD);
        assert_eq!(
            listed[0].folders,
            vec![
                (
                    PathBuf::from("app/models"),
                    vec!["scunet_color_real_448.onnx".to_owned(), "scunet_color_real_960.onnx".to_owned()]
                ),
                (PathBuf::from("data/models"), vec!["scunet_color_real_448_fp16.onnx".to_owned()]),
            ]
        );
    }

    #[test]
    fn nothing_installed() {
        assert!(list(&[]).iter().all(|m| m.folders.is_empty()));
        // the same file found twice is listed once
        let twice = [PathBuf::from("m/scunet_color_real_448.onnx"), "m/scunet_color_real_448.onnx".into()];
        assert_eq!(list(&twice)[0].folders[0].1.len(), 1);
    }
}
