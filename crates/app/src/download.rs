//! Download of the upscaling models, only when the user presses the button
//! for it: from the project's release over HTTPS, with Windows' own TLS and
//! certificates; every file checked against the size and SHA-256 compiled
//! into the program (`models/upscalers.txt`), written next to its final
//! name and renamed durably. Nothing else is sent than the request for the
//! file, nothing else is received.
//!
//! An interrupted download resumes: what arrived stays in `.<name>.part`,
//! and the next download asks the server only for the rest (HTTP `Range`).
//! The hash covers the whole file, the part already on disk included, so a
//! damaged partial file is caught like a damaged download.

use std::fs::{File, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
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
/// Longest `Content-Range` header read: `bytes <start>-<end>/<size>` with
/// numbers of 20 digits fits in it.
const MAX_CONTENT_RANGE: usize = 80;

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
/// went wrong. Files already completed stay; of the one in progress, what
/// arrived stays for the next download if the connection failed, and goes
/// otherwise (cancelled, larger or different than expected).
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

/// What the server answered to a request for a file from a byte on.
enum Reply {
    /// 206: the file from byte `start` to its end.
    Rest { start: u64, body: Box<dyn Read> },
    /// 200: the whole file (a server that ignores `Range` answers so).
    Whole(Box<dyn Read>),
    /// The range cannot be had (416, or a 206 that does not say which part
    /// of the expected file it holds): the file must be asked for again.
    NoRange,
}

/// One file over HTTPS, resuming what an earlier download left.
fn fetch(
    agent: &ureq::Agent,
    url: &str,
    entry: &Entry,
    dir: &Path,
    on_bytes: &mut dyn FnMut(u64),
    control: &JobControl,
) -> Result<(), DownloadError> {
    let failed = |e: &dyn std::fmt::Display| DownloadError::Failed(e.to_string());
    let mut request = |from: u64| -> Result<Reply, DownloadError> {
        let rest = entry.size.saturating_sub(from);
        let limit = Duration::from_secs(60 + rest / SLOWEST_BYTES_PER_SEC);
        let mut builder = agent.get(url);
        if from > 0 {
            builder = builder.header("Range", format!("bytes={from}-"));
        }
        let response = match builder.config().timeout_recv_body(Some(limit)).build().call() {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(416)) => return Ok(Reply::NoRange),
            Err(ureq::Error::StatusCode(404)) => {
                return Err(DownloadError::Failed(format!("{} is not published yet", entry.name)))
            }
            Err(other) => return Err(failed(&other)),
        };
        let partial = response.status().as_u16() == 206;
        let start = response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| content_range_start(v, entry.size));
        // one byte more than expected is read, so that a longer file is caught
        let body = |max: u64| -> Box<dyn Read> {
            Box::new(response.into_body().into_with_config().limit(max.saturating_add(1)).reader())
        };
        Ok(match (partial, start) {
            (false, _) => Reply::Whole(body(entry.size)),
            (true, Some(start)) => Reply::Rest { start, body: body(entry.size - start) },
            (true, None) => Reply::NoRange,
        })
    };
    resume(entry, dir, &mut request, on_bytes, control)
}

/// The first byte of a `Content-Range` answer (`bytes 100-199/200`) for a
/// file of `size` bytes, if the range runs to the end of exactly that file;
/// `None` for anything else.
fn content_range_start(header: &str, size: u64) -> Option<u64> {
    if header.len() > MAX_CONTENT_RANGE {
        return None;
    }
    let (range, total) = header.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let number = |s: &str| s.bytes().all(|b| b.is_ascii_digit()).then(|| s.parse::<u64>().ok()).flatten();
    let (start, end, total) = (number(start)?, number(end)?, number(total)?);
    (total == size && end.checked_add(1) == Some(size) && start <= end).then_some(start)
}

/// How a download that did not complete leaves its partial file.
enum Stopped {
    /// The connection failed: what arrived is kept for the next download.
    Keep(DownloadError),
    /// Cancelled, or the file is not the expected one: nothing is kept.
    Discard(DownloadError),
}

/// Downloads `entry` into `dir`, asking `request` for the file from a byte
/// on and resuming the partial file of an earlier download; renamed to its
/// name only if its size and SHA-256 are the expected ones.
fn resume(
    entry: &Entry,
    dir: &Path,
    request: &mut dyn FnMut(u64) -> Result<Reply, DownloadError>,
    on_bytes: &mut dyn FnMut(u64),
    control: &JobControl,
) -> Result<(), DownloadError> {
    let failed = |e: &dyn std::fmt::Display| DownloadError::Failed(e.to_string());
    let part = dir.join(format!(".{}.part", entry.name));
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&part)
        .map_err(|e| failed(&e))?;
    // a second window downloading the same file would mix the two
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(DownloadError::Failed(format!("{} is being downloaded by another window", entry.name)))
        }
        Err(TryLockError::Error(e)) => return Err(failed(&e)),
    }
    match complete(&mut file, entry, request, on_bytes, control) {
        Ok(()) => {
            drop(file);
            denoise_platform::rename_durably(&part, &dir.join(&entry.name)).map_err(|e| failed(&e))
        }
        Err(Stopped::Keep(e)) if file.metadata().is_ok_and(|m| m.len() > 0) => Err(e),
        Err(Stopped::Keep(e) | Stopped::Discard(e)) => {
            // emptied while still locked, so that no other window takes it
            // up between closing and removing; best effort: the error
            // returned is the one that matters
            file.set_len(0).ok();
            drop(file);
            std::fs::remove_file(&part).ok();
            Err(e)
        }
    }
}

/// Brings `file`, the partial download of `entry`, to the whole file with
/// the expected hash, written to the disk. Starts again from the first byte
/// when the partial file is longer than the file, when the server does not
/// send the part asked for, and once when the file has the wrong hash (the
/// damage may be in the part an earlier download left).
fn complete(
    file: &mut File,
    entry: &Entry,
    request: &mut dyn FnMut(u64) -> Result<Reply, DownloadError>,
    on_bytes: &mut dyn FnMut(u64),
    control: &JobControl,
) -> Result<(), Stopped> {
    let local = |e: std::io::Error| Stopped::Discard(DownloadError::Failed(e.to_string()));
    let mut have = file.metadata().map_err(local)?.len();
    if have > entry.size {
        tracing::info!("{}: partial file larger than the model, downloading again", entry.name);
        have = 0;
    }
    file.set_len(have).map_err(local)?;
    let mut hasher = Sha256::new();
    if have > 0 {
        tracing::info!("{}: resuming after {have} bytes", entry.name);
        hash_prefix(file, have, &mut hasher, control)?;
        on_bytes(have);
    }
    let mut may_restart = true;
    loop {
        if have < entry.size {
            let body = match request(have).map_err(Stopped::Keep)? {
                Reply::Rest { start, body } if start == have => body,
                Reply::Whole(body) => {
                    (have, hasher) = (0, Sha256::new());
                    body
                }
                Reply::Rest { .. } | Reply::NoRange if have > 0 => {
                    tracing::info!("{}: the server did not resume, downloading again", entry.name);
                    (have, hasher) = (0, Sha256::new());
                    file.set_len(0).map_err(local)?;
                    continue;
                }
                Reply::Rest { .. } | Reply::NoRange => {
                    return Err(Stopped::Discard(DownloadError::Failed(format!(
                        "the server did not send {}",
                        entry.name
                    ))))
                }
            };
            file.set_len(have).map_err(local)?;
            file.seek(SeekFrom::Start(have)).map_err(local)?;
            have = append(file, body, entry, have, &mut hasher, on_bytes, control)?;
        }
        if have == entry.size && hasher.clone().finalize().as_slice() == entry.sha256 {
            file.sync_all().map_err(local)?;
            return Ok(());
        }
        if have < entry.size {
            // the connection ended before the end of the file
            return Err(Stopped::Keep(DownloadError::Failed(format!(
                "the download of {} was interrupted",
                entry.name
            ))));
        }
        if !std::mem::take(&mut may_restart) {
            return Err(Stopped::Discard(DownloadError::Failed(format!(
                "{} differs from the expected file",
                entry.name
            ))));
        }
        tracing::info!("{}: the file differs from the expected one, downloading again", entry.name);
        (have, hasher) = (0, Sha256::new());
        file.set_len(0).map_err(local)?;
    }
}

/// Hashes the first `len` bytes of `file`, the part an earlier download left.
fn hash_prefix(file: &mut File, len: u64, hasher: &mut Sha256, control: &JobControl) -> Result<(), Stopped> {
    let local = |e: std::io::Error| Stopped::Discard(DownloadError::Failed(e.to_string()));
    file.seek(SeekFrom::Start(0)).map_err(local)?;
    let mut buf = vec![0u8; CHUNK];
    let mut left = len;
    while left > 0 {
        if control.is_cancelled() {
            return Err(Stopped::Discard(DownloadError::Cancelled));
        }
        let n = usize::try_from(left).map_or(CHUNK, |l| l.min(CHUNK));
        file.read_exact(&mut buf[..n]).map_err(local)?;
        hasher.update(&buf[..n]);
        left -= n as u64;
    }
    Ok(())
}

/// Appends what `body` gives to `file`, which holds `have` bytes, while
/// hashing it; returns the bytes the file holds at the end of the body.
fn append(
    file: &mut File,
    mut body: Box<dyn Read>,
    entry: &Entry,
    mut have: u64,
    hasher: &mut Sha256,
    on_bytes: &mut dyn FnMut(u64),
    control: &JobControl,
) -> Result<u64, Stopped> {
    let local = |e: std::io::Error| Stopped::Discard(DownloadError::Failed(e.to_string()));
    let mut buf = vec![0u8; CHUNK];
    loop {
        if control.is_cancelled() {
            return Err(Stopped::Discard(DownloadError::Cancelled));
        }
        let n = match body.read(&mut buf) {
            Ok(0) => return Ok(have),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Stopped::Keep(DownloadError::Failed(e.to_string()))),
        };
        have += n as u64;
        if have > entry.size {
            return Err(Stopped::Discard(DownloadError::Failed(format!("{} is larger than expected", entry.name))));
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(local)?;
        on_bytes(have);
    }
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

    #[test]
    fn content_ranges() {
        assert_eq!(content_range_start("bytes 100-199/200", 200), Some(100));
        assert_eq!(content_range_start("bytes 0-199/200", 200), Some(0));
        assert_eq!(content_range_start("bytes 199-199/200", 200), Some(199));
        for bad in [
            "bytes 100-199/300",  // another file
            "bytes 100-150/200",  // not to the end
            "bytes 150-100/200",  // backwards
            "bytes 100-199/*",    // size unknown
            "bytes */200",        // the 416 form
            "bytes -100-199/200", // signs
            "bytes +100-199/200",
            "bytes 100 -199/200", // spaces
            "items 100-199/200",  // another unit
            "bytes 100-199",      // no size
            "",
            "bytes 1-18446744073709551615/18446744073709551615", // end + 1 overflows
            "bytes 99999999999999999999999-1/2",
        ] {
            assert_eq!(content_range_start(bad, 200), None, "{bad}");
        }
        assert_eq!(content_range_start("bytes 1-18446744073709551615/18446744073709551615", u64::MAX), None);
        let long = format!("bytes {}100-199/200", "0".repeat(100));
        assert_eq!(content_range_start(&long, 200), None);
    }

    fn entry_for(data: &[u8]) -> Entry {
        let sha256 = Sha256::digest(data).into();
        Entry { sha256, size: data.len() as u64, name: "swinir_real_x2_psnr_320_fp16.onnx".into() }
    }

    fn model() -> Vec<u8> {
        (0..10_000u32).flat_map(|i| (i.wrapping_mul(2_654_435_761)).to_le_bytes()).collect()
    }

    fn part_of(dir: &Path, entry: &Entry) -> PathBuf {
        dir.join(format!(".{}.part", entry.name))
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    fn whole(data: &[u8]) -> Reply {
        Reply::Whole(Box::new(std::io::Cursor::new(data.to_vec())))
    }

    fn rest(data: &[u8], start: usize) -> Reply {
        Reply::Rest { start: start as u64, body: Box::new(std::io::Cursor::new(data[start..].to_vec())) }
    }

    /// A server that answers as `answer` from the byte asked for, and
    /// records what was asked.
    struct Server<F: FnMut(u64) -> Reply> {
        asked: Vec<u64>,
        answer: F,
    }

    impl<F: FnMut(u64) -> Reply> Server<F> {
        fn new(answer: F) -> Self {
            Self { asked: Vec::new(), answer }
        }

        fn run(&mut self, entry: &Entry, dir: &Path, control: &JobControl) -> Result<(), DownloadError> {
            let mut request = |from| {
                self.asked.push(from);
                Ok((self.answer)(from))
            };
            resume(entry, dir, &mut request, &mut |_| {}, control)
        }
    }

    /// Reads `limit` bytes of `data`, then fails as a dropped connection.
    struct Dropped {
        data: std::io::Cursor<Vec<u8>>,
        limit: u64,
    }

    impl Read for Dropped {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.data.position() >= self.limit {
                return Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "connection reset"));
            }
            let room = usize::try_from(self.limit - self.data.position()).unwrap();
            let n = room.min(buf.len());
            self.data.read(&mut buf[..n])
        }
    }

    #[test]
    fn a_fresh_download_keeps_only_the_expected_file() {
        let dir = tempfile::tempdir().unwrap();
        let data = model();
        let entry = entry_for(&data);
        let ctrl = JobControl::default();
        let mut seen = 0;
        let mut request = |_| Ok(whole(&data));
        resume(&entry, dir.path(), &mut request, &mut |n| seen = n, &ctrl).unwrap();
        assert_eq!(std::fs::read(dir.path().join(&entry.name)).unwrap(), data);
        assert_eq!(seen, data.len() as u64);
        assert_eq!(files_in(dir.path()), [entry.name.as_str()]);
    }

    #[test]
    fn a_dropped_connection_resumes_where_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let data = model();
        let entry = entry_for(&data);
        let ctrl = JobControl::default();
        let cut = 12_345;
        let mut first =
            Server::new(|_| Reply::Whole(Box::new(Dropped { data: std::io::Cursor::new(data.clone()), limit: cut })));
        assert!(matches!(first.run(&entry, dir.path(), &ctrl), Err(DownloadError::Failed(_))));
        // what arrived is kept, and only that
        assert_eq!(std::fs::read(part_of(dir.path(), &entry)).unwrap(), data[..12_345]);
        // the next download asks for the rest only, and the hash of the
        // whole file (the part on disk included) is the expected one
        let mut second = Server::new(|from| rest(&data, usize::try_from(from).unwrap()));
        second.run(&entry, dir.path(), &ctrl).unwrap();
        assert_eq!(second.asked, [cut]);
        assert_eq!(std::fs::read(dir.path().join(&entry.name)).unwrap(), data);
        assert_eq!(files_in(dir.path()), [entry.name.as_str()]);
    }

    #[test]
    fn a_server_that_ignores_the_range_starts_from_zero() {
        let dir = tempfile::tempdir().unwrap();
        let data = model();
        let entry = entry_for(&data);
        std::fs::write(part_of(dir.path(), &entry), &data[..5000]).unwrap();
        // 200 with the whole file: appended to the part, it would be too long
        let mut server = Server::new(|_| whole(&data));
        server.run(&entry, dir.path(), &JobControl::default()).unwrap();
        assert_eq!(server.asked, [5000]);
        assert_eq!(std::fs::read(dir.path().join(&entry.name)).unwrap(), data);
        assert_eq!(files_in(dir.path()), [entry.name.as_str()]);
    }

    #[test]
    fn a_range_refused_or_misplaced_starts_from_zero() {
        let data = model();
        let entry = entry_for(&data);
        // 416, then a 206 from another byte than the one asked for
        let wrongs: [fn(&[u8]) -> Reply; 2] = [|_| Reply::NoRange, |d| rest(d, 100)];
        for wrong in wrongs {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(part_of(dir.path(), &entry), &data[..5000]).unwrap();
            let mut server = Server::new(|from| if from == 0 { whole(&data) } else { wrong(&data) });
            server.run(&entry, dir.path(), &JobControl::default()).unwrap();
            assert_eq!(server.asked, [5000, 0]);
            assert_eq!(std::fs::read(dir.path().join(&entry.name)).unwrap(), data);
        }
        // a server that never sends the file: an error, nothing left
        let dir = tempfile::tempdir().unwrap();
        let mut server = Server::new(|_| Reply::NoRange);
        assert!(matches!(server.run(&entry, dir.path(), &JobControl::default()), Err(DownloadError::Failed(_))));
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn a_partial_file_larger_than_the_model_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let data = model();
        let entry = entry_for(&data);
        std::fs::write(part_of(dir.path(), &entry), [data.as_slice(), b"extra"].concat()).unwrap();
        let mut server = Server::new(|_| whole(&data));
        server.run(&entry, dir.path(), &JobControl::default()).unwrap();
        assert_eq!(server.asked, [0]);
        assert_eq!(std::fs::read(dir.path().join(&entry.name)).unwrap(), data);
        assert_eq!(files_in(dir.path()), [entry.name.as_str()]);
    }

    #[test]
    fn a_complete_partial_file_needs_no_network() {
        let dir = tempfile::tempdir().unwrap();
        let data = model();
        let entry = entry_for(&data);
        std::fs::write(part_of(dir.path(), &entry), &data).unwrap();
        let mut server = Server::new(|_| unreachable!("nothing to ask for"));
        server.run(&entry, dir.path(), &JobControl::default()).unwrap();
        assert_eq!(files_in(dir.path()), [entry.name.as_str()]);
    }

    #[test]
    fn a_damaged_partial_file_is_downloaded_again_once() {
        let data = model();
        let entry = entry_for(&data);
        let mut damaged = data[..5000].to_vec();
        damaged[10] ^= 1;
        // the damage is in the part on disk: once more from zero, then right
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(part_of(dir.path(), &entry), &damaged).unwrap();
        let mut server = Server::new(|from| if from == 0 { whole(&data) } else { rest(&data, 5000) });
        server.run(&entry, dir.path(), &JobControl::default()).unwrap();
        assert_eq!(server.asked, [5000, 0]);
        assert_eq!(std::fs::read(dir.path().join(&entry.name)).unwrap(), data);
        // a server that keeps sending another file: refused, nothing left
        let dir = tempfile::tempdir().unwrap();
        let mut other = data.clone();
        other[5] ^= 1;
        let mut server = Server::new(|_| whole(&other));
        assert!(matches!(server.run(&entry, dir.path(), &JobControl::default()), Err(DownloadError::Failed(_))));
        assert_eq!(server.asked, [0, 0]);
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn wrong_sizes_and_cancellation_leave_nothing() {
        let data = model();
        let entry = entry_for(&data);
        let shorter = data[..10].to_vec();
        let longer = [data.as_slice(), b"x"].concat();
        // shorter: the connection ended early, the part stays for later
        let dir = tempfile::tempdir().unwrap();
        let mut server = Server::new(|_| whole(&shorter));
        assert!(matches!(server.run(&entry, dir.path(), &JobControl::default()), Err(DownloadError::Failed(_))));
        assert_eq!(files_in(dir.path()), [format!(".{}.part", entry.name)]);
        // longer: not the expected file, nothing left
        let dir = tempfile::tempdir().unwrap();
        let mut server = Server::new(|_| whole(&longer));
        assert!(matches!(server.run(&entry, dir.path(), &JobControl::default()), Err(DownloadError::Failed(_))));
        assert!(files_in(dir.path()).is_empty());
        // cancelled: nothing left either
        let dir = tempfile::tempdir().unwrap();
        let ctrl = JobControl::default();
        ctrl.cancel();
        let mut server = Server::new(|_| whole(&data));
        assert!(matches!(server.run(&entry, dir.path(), &ctrl), Err(DownloadError::Cancelled)));
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn two_windows_never_write_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let data = model();
        let entry = entry_for(&data);
        let other = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(part_of(dir.path(), &entry))
            .unwrap();
        other.lock().unwrap();
        let mut server = Server::new(|_| whole(&data));
        assert!(matches!(server.run(&entry, dir.path(), &JobControl::default()), Err(DownloadError::Failed(_))));
        assert!(server.asked.is_empty());
        drop(other);
        server.run(&entry, dir.path(), &JobControl::default()).unwrap();
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
