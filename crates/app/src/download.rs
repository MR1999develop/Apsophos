//! Download of the upscaling models, only when the user presses the button
//! for it: from the project's release over HTTPS, with Windows' own TLS and
//! certificates; every file checked against the size and SHA-256 compiled
//! into the program (`models/upscalers.txt`), written next to its final
//! name and renamed durably. Nothing else is sent than the request for the
//! file, nothing else is received.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use denoise_core::{product, JobControl};
use sha2::{Digest, Sha256};

/// The list of the models, with where they are published.
const MANIFEST: &str = include_str!("../../../models/upscalers.txt");
/// Bytes read at a time; cancellation is checked between reads.
const CHUNK: usize = 1 << 20;
/// The slowest transfer still waited for: below it the download stops
/// with a time-out instead of hanging.
const SLOWEST_BYTES_PER_SEC: u64 = 20_000;

/// One file of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub sha256: [u8; 32],
    pub size: u64,
    pub name: String,
}

struct Manifest {
    base_url: String,
    entries: Vec<Entry>,
}

/// The models the user can download, as two choices: those trained for
/// fidelity, and those that generate detail (offered apart, never together).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pack {
    Faithful,
    Generated,
}

impl Pack {
    fn holds(self, name: &str) -> bool {
        match self {
            Pack::Faithful => name.contains("_psnr_"),
            Pack::Generated => name.contains("_gan_"),
        }
    }
}

/// Why a download ended without the files.
#[derive(Debug)]
pub enum DownloadError {
    Cancelled,
    /// In English, for the log and inside the interface's sentence.
    Failed(String),
}

fn manifest() -> &'static Manifest {
    static M: OnceLock<Manifest> = OnceLock::new();
    M.get_or_init(|| parse(MANIFEST).unwrap_or(Manifest { base_url: String::new(), entries: Vec::new() }))
}

/// The list as written in `models/upscalers.txt`; `None` if a line is not
/// understood (a test checks the one compiled in).
fn parse(text: &str) -> Option<Manifest> {
    let mut base_url = None;
    let mut entries = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["url", url] if url.starts_with("https://") && url.ends_with('/') => base_url = Some((*url).to_owned()),
            [hash, size, name] if denoise_engine::upscaler_file_kind(Path::new(name)).is_some() => {
                entries.push(Entry { sha256: hex32(hash)?, size: size.parse().ok()?, name: (*name).to_owned() });
            }
            _ => return None,
        }
    }
    Some(Manifest { base_url: base_url?, entries })
}

fn hex32(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (o, pair) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
        *o = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

/// The files of `pack` that are not among `installed` (by name).
pub fn missing(pack: Pack, installed: &[PathBuf]) -> Vec<Entry> {
    let have = |name: &str| installed.iter().any(|p| p.file_name().is_some_and(|n| n == name));
    manifest().entries.iter().filter(|e| pack.holds(&e.name) && !have(&e.name)).cloned().collect()
}

/// `true` if none of the files of `pack` is among `installed`: then the
/// program offers them at start.
pub fn none_installed(pack: Pack, installed: &[PathBuf]) -> bool {
    missing(pack, installed).len() == manifest().entries.iter().filter(|e| pack.holds(&e.name)).count()
}

/// The size of all of `pack`, for the button's label.
pub fn pack_bytes(pack: Pack) -> u64 {
    manifest().entries.iter().filter(|e| pack.holds(&e.name)).map(|e| e.size).sum()
}

/// Where downloaded models go: the models folder in the user's local data
/// (on Windows `%LOCALAPPDATA%`, never synchronised with a roaming profile),
/// next to the logs and the TensorRT cache.
pub fn download_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(product::DATA_DIR).join("models"))
}

/// Downloads `entries` into `dir` (created if needed). `progress` gets the
/// bytes done and in all.
///
/// # Errors
/// [`DownloadError::Cancelled`] if `control` is cancelled; otherwise what
/// went wrong. Files already completed stay; the one in progress is removed.
pub fn download(
    entries: &[Entry],
    dir: &Path,
    progress: &mut dyn FnMut(u64, u64),
    control: &JobControl,
) -> Result<(), DownloadError> {
    let failed = |e: &dyn std::fmt::Display| DownloadError::Failed(e.to_string());
    std::fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    let agent = agent();
    let total: u64 = entries.iter().map(|e| e.size).sum();
    let mut before = 0;
    for entry in entries {
        let url = format!("{}{}", manifest().base_url, entry.name);
        let mut on_bytes = |n: u64| progress(before + n, total);
        fetch(&agent, &url, entry, dir, &mut on_bytes, control)?;
        before += entry.size;
        tracing::info!("downloaded {}", entry.name);
    }
    Ok(())
}

fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .user_agent(format!("{}/{}", product::NAME, env!("CARGO_PKG_VERSION")))
        .tls_config(tls)
        .build()
        .into()
}

/// One file: read into a temporary file beside its name while hashing it,
/// then renamed if size and hash are the expected ones.
fn fetch(
    agent: &ureq::Agent,
    url: &str,
    entry: &Entry,
    dir: &Path,
    on_bytes: &mut dyn FnMut(u64),
    control: &JobControl,
) -> Result<(), DownloadError> {
    let failed = |e: &dyn std::fmt::Display| DownloadError::Failed(e.to_string());
    let limit = Duration::from_secs(60 + entry.size / SLOWEST_BYTES_PER_SEC);
    let response = agent.get(url).config().timeout_recv_body(Some(limit)).build().call().map_err(|e| match e {
        ureq::Error::StatusCode(404) => DownloadError::Failed(format!("{} is not published yet", entry.name)),
        other => failed(&other),
    })?;
    let reader = response.into_body().into_with_config().limit(entry.size.saturating_add(1)).reader();
    store(reader, entry, dir, on_bytes, control)
}

/// Writes what `reader` gives into `dir`, under the entry's name if its size
/// and SHA-256 are the expected ones; nothing is left otherwise.
fn store(
    mut reader: impl Read,
    entry: &Entry,
    dir: &Path,
    on_bytes: &mut dyn FnMut(u64),
    control: &JobControl,
) -> Result<(), DownloadError> {
    let failed = |e: &dyn std::fmt::Display| DownloadError::Failed(e.to_string());
    let (tmp, file) = temporary(dir, &entry.name).map_err(|e| failed(&e))?;
    let result = (|| {
        let mut out = BufWriter::new(file);
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; CHUNK];
        let mut got = 0u64;
        loop {
            if control.is_cancelled() {
                return Err(DownloadError::Cancelled);
            }
            let n = reader.read(&mut buf).map_err(|e| failed(&e))?;
            if n == 0 {
                break;
            }
            got += n as u64;
            if got > entry.size {
                return Err(DownloadError::Failed(format!("{} is larger than expected", entry.name)));
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| failed(&e))?;
            on_bytes(got);
        }
        if got != entry.size || hasher.finalize().as_slice() != entry.sha256 {
            return Err(DownloadError::Failed(format!("{} differs from the expected file", entry.name)));
        }
        out.flush().map_err(|e| failed(&e))?;
        out.get_ref().sync_all().map_err(|e| failed(&e))?;
        drop(out);
        denoise_platform::rename_durably(&tmp, &dir.join(&entry.name)).map_err(|e| failed(&e))
    })();
    if result.is_err() {
        // best effort: the error returned is the one that matters
        std::fs::remove_file(&tmp).ok();
    }
    result
}

/// A new file `.<name>.<process>-<n>.part` in `dir`, created exclusively.
fn temporary(dir: &Path, name: &str) -> std::io::Result<(PathBuf, File)> {
    for n in 0..100 {
        let tmp = dir.join(format!(".{name}.{}-{n}.part", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "no free name for a temporary file"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compiled_list_is_understood() {
        let m = parse(MANIFEST).expect("models/upscalers.txt");
        assert!(m.base_url.starts_with("https://github.com/"));
        assert_eq!(m.entries.len(), 8);
        // both packs, each with 2× and 4× in two tile sizes, FP16 only
        for pack in [Pack::Faithful, Pack::Generated] {
            assert_eq!(m.entries.iter().filter(|e| pack.holds(&e.name)).count(), 4);
            assert!(pack_bytes(pack) > 100_000_000);
        }
        assert!(m.entries.iter().all(|e| e.name.ends_with("_fp16.onnx")));
    }

    #[test]
    fn bad_lists_are_refused() {
        assert!(parse("url http://example.org/\n").is_none(), "not https");
        assert!(parse("url https://example.org/\nabc 12 swinir_real_x2_psnr_320_fp16.onnx").is_none());
        let hash = "00".repeat(32);
        assert!(parse(&format!("url https://e.org/\n{hash} 12 other.onnx")).is_none());
        assert!(parse(&format!("url https://e.org/\n{hash} 12 swinir_real_x2_psnr_320_fp16.onnx")).is_some());
    }

    #[test]
    fn only_what_is_missing_is_downloaded() {
        let installed = vec![PathBuf::from("m/swinir_real_x2_psnr_320_fp16.onnx")];
        let need = missing(Pack::Faithful, &installed);
        assert_eq!(need.len(), 3);
        assert!(need.iter().all(|e| e.name.contains("_psnr_")));
        // the offer at start: only when none is there
        assert!(!none_installed(Pack::Faithful, &installed));
        assert!(none_installed(Pack::Faithful, &[PathBuf::from("m/swinir_real_x2_gan_320_fp16.onnx")]));
        assert!(none_installed(Pack::Generated, &installed));
    }

    fn entry_for(data: &[u8]) -> Entry {
        let sha256 = Sha256::digest(data).into();
        Entry { sha256, size: data.len() as u64, name: "swinir_real_x2_psnr_320_fp16.onnx".into() }
    }

    #[test]
    fn only_the_expected_file_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let data = b"model bytes".repeat(1000);
        let entry = entry_for(&data);
        let ctrl = JobControl::default();
        let mut seen = 0;
        store(&data[..], &entry, dir.path(), &mut |n| seen = n, &ctrl).unwrap();
        assert_eq!(std::fs::read(dir.path().join(&entry.name)).unwrap(), data);
        assert_eq!(seen, data.len() as u64);
        // a different file, a shorter one, a longer one: refused, nothing left
        let empty = tempfile::tempdir().unwrap();
        let mut other = data.clone();
        other[5] ^= 1;
        for bad in [&other[..], &data[..10], &[data.as_slice(), b"x"].concat()[..]] {
            assert!(matches!(store(bad, &entry, empty.path(), &mut |_| {}, &ctrl), Err(DownloadError::Failed(_))));
            assert_eq!(std::fs::read_dir(empty.path()).unwrap().count(), 0);
        }
        // cancelled: nothing left either
        ctrl.cancel();
        assert!(matches!(store(&data[..], &entry, empty.path(), &mut |_| {}, &ctrl), Err(DownloadError::Cancelled)));
        assert_eq!(std::fs::read_dir(empty.path()).unwrap().count(), 0);
    }

    #[test]
    fn the_network_fails_cleanly() {
        // nothing listens there: an error, and no file
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_for(b"x");
        let r = fetch(&agent(), "https://127.0.0.1:9/none", &entry, dir.path(), &mut |_| {}, &JobControl::default());
        assert!(matches!(r, Err(DownloadError::Failed(_))));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
