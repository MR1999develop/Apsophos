//! The libraries' errors in the interface's language.
//!
//! `denoise_io`, `denoise_core` and `denoise_engine` word their errors in
//! English for the log; the interface shows them inside its own sentences,
//! so here each variant gets the catalog's wording. Details that come from
//! further down (a decoder's message, a GPU driver's text) stay as they are.

use denoise_core::DenoiseError;
use denoise_engine::gpu_error::GpuProblem;
use denoise_engine::EngineError;
use denoise_io::IoError;

use crate::format;
use crate::text::tr;

pub fn io(e: &IoError) -> String {
    let t = &tr().errors;
    match e {
        IoError::UnknownFormat => t.unknown_format.to_owned(),
        IoError::Unsupported(what) => (t.unsupported)(what),
        IoError::Decode(why) => (t.damaged)(why),
        IoError::Encode(why) => (t.encode)(why),
        IoError::TooManyPixels { width, height, max_mpx } => (t.too_many_pixels)(
            &format::int(*width as usize),
            &format::int(*height as usize),
            &format::int(usize::try_from(*max_mpx).unwrap_or(usize::MAX)),
        ),
        IoError::TooLarge { format, max } => (t.too_large)(format, &format::int(*max as usize)),
        IoError::FileTooLarge { format, max_gb } => (t.file_too_large)(format, &format::int(*max_gb as usize)),
        IoError::Io(io) => match io.kind() {
            std::io::ErrorKind::NotFound => t.not_found.to_owned(),
            std::io::ErrorKind::PermissionDenied => t.permission_denied.to_owned(),
            // Windows words these in the system's language already
            _ => io.to_string(),
        },
    }
}

pub fn denoise(e: &DenoiseError) -> String {
    let t = &tr().errors;
    match e {
        DenoiseError::Cancelled => t.cancelled.to_owned(),
        DenoiseError::InvalidSize(what) => (t.invalid_size)(what),
        DenoiseError::OutOfRange(v) => (t.out_of_range)(&format::dec(*v, 2)),
        DenoiseError::Model(m) => (t.network)(&gpu(&m.0)),
    }
}

pub fn engine(e: &EngineError) -> String {
    let t = &tr().errors;
    match e {
        EngineError::Missing(path) => (t.model_missing)(path),
        EngineError::Load(why) => (t.model_load)(&gpu(why)),
    }
}

/// A GPU error as `gpu_error::plain` words it, alone or in a longer text
/// (`DirectML: …`): the English words of a known problem become the
/// catalog's; the rest (device names, the Windows code) stays.
pub fn gpu(text: &str) -> String {
    let t = &tr().errors;
    let mut out = text.to_owned();
    for p in GpuProblem::ALL {
        if out.contains(p.words()) {
            let words = match p {
                GpuProblem::OutOfMemory => t.gpu_memory,
                GpuProblem::DeviceLost => t.gpu_lost,
                GpuProblem::Unsupported => t.gpu_unsupported,
            };
            out = out.replace(p.words(), words).replace(" (error ", &format!(" ({} ", t.error_code));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_problems_keep_their_code() {
        let raw = denoise_engine::gpu_error::plain("DirectML failed with 887A0001 somewhere");
        let said = gpu(&raw);
        assert!(said.starts_with(tr().errors.gpu_memory), "{said}");
        assert!(said.ends_with("887A0001)"), "{said}");
        // an unknown text is shown as it is
        assert_eq!(gpu("something else"), "something else");
    }

    #[test]
    fn io_errors_in_words() {
        let missing = IoError::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(io(&missing), tr().errors.not_found);
        let big = IoError::TooManyPixels { width: 40_000, height: 30_000, max_mpx: 500 };
        assert!(io(&big).contains("40,000"), "{}", io(&big));
    }
}
