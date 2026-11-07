use thiserror::Error;

/// Read and write errors, worded for the user.
#[derive(Debug, Error)]
pub enum IoError {
    #[error("unrecognised format: TIFF, JPEG and PNG files can be opened")]
    UnknownFormat,
    #[error("{0} is not supported")]
    Unsupported(String),
    #[error("the file is damaged or unreadable ({0})")]
    Decode(String),
    #[error("encoding the file failed ({0})")]
    Encode(String),
    #[error("image too large to open: {width} × {height} px; the limit on this computer is {max_mpx} megapixels")]
    TooManyPixels { width: u32, height: u32, max_mpx: u64 },
    #[error("image too large for the {format} format: at most {max} px per side")]
    TooLarge { format: &'static str, max: u32 },
    #[error("image too large for a {format} file: at most {max_gb} GB")]
    FileTooLarge { format: &'static str, max_gb: u32 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
