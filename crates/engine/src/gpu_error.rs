//! GPU errors in words. ONNX Runtime passes on the text of DirectML and
//! TensorRT for RTX, which is long and names only a Windows error code
//! (`887A0001`, …); the self-test and the tile-size search show what it means
//! instead, keeping the code for whoever needs to look it up.

/// How TensorRT for RTX and CUDA say that memory ran out (lower case).
const MEMORY_WORDS: [&str; 4] = ["out of memory", "outofmemory", "memoryallocation", "failed to allocate"];

/// What a GPU error means for the user, as far as its text tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuProblem {
    /// The tile does not fit in the GPU memory: smaller tiles still work.
    OutOfMemory,
    /// The GPU was reset or stopped responding (driver crash, update, sleep).
    DeviceLost,
    /// DirectML cannot work: its DLL next to the program is missing or old,
    /// and Windows' own copy is too old for the model.
    Unsupported,
}

impl GpuProblem {
    pub const ALL: [Self; 3] = [Self::OutOfMemory, Self::DeviceLost, Self::Unsupported];

    /// Reads the problem from an error text; `None` if it is not recognised.
    pub fn from_text(raw: &str) -> Option<Self> {
        let lower = raw.to_ascii_lowercase();
        let code = windows_code(raw);
        match code.as_deref() {
            // DXGI_ERROR_INVALID_CALL: what DirectML answers when a tensor
            // cannot be allocated; E_OUTOFMEMORY
            Some("887A0001" | "8007000E") => return Some(Self::OutOfMemory),
            // DXGI_ERROR_DEVICE_REMOVED, _HUNG, _RESET
            Some("887A0005" | "887A0006" | "887A0007") => return Some(Self::DeviceLost),
            // DXGI_ERROR_UNSUPPORTED
            Some("887A0004") => return Some(Self::Unsupported),
            _ => {}
        }
        MEMORY_WORDS.iter().any(|k| lower.contains(k)).then_some(Self::OutOfMemory)
    }

    /// The problem in words, in English: [`plain`] puts it at the start of
    /// the message, where a program can find it and say it in its own
    /// language.
    pub fn words(self) -> &'static str {
        match self {
            Self::OutOfMemory => "not enough GPU memory for this tile size",
            Self::DeviceLost => {
                "the GPU stopped responding or was reset (driver update, crash or sleep); restarting the program usually helps"
            }
            Self::Unsupported => {
                "DirectML cannot run the model: the DirectML.dll next to the program is missing or out of date"
            }
        }
    }
}

/// The error in words when it is recognised, followed by its Windows code;
/// otherwise the text as it is.
pub fn plain(raw: &str) -> String {
    match GpuProblem::from_text(raw) {
        Some(p) => match windows_code(raw) {
            Some(code) => format!("{} (error {code})", p.words()),
            None => p.words().to_owned(),
        },
        None => raw.to_owned(),
    }
}

/// The first Windows HRESULT of the DXGI (`887A…`) or general (`8007…`)
/// families in a text, in upper case, with or without `0x`.
fn windows_code(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    (0..bytes.len().saturating_sub(7)).find_map(|i| {
        let word = raw.get(i..i + 8)?;
        let starts_word = i == 0 || !bytes[i - 1].is_ascii_alphanumeric() || bytes[..i].ends_with(b"0x");
        let ends_word = bytes.get(i + 8).is_none_or(|b| !b.is_ascii_alphanumeric());
        let upper = word.to_ascii_uppercase();
        (starts_word
            && ends_word
            && (upper.starts_with("887A") || upper.starts_with("8007"))
            && upper.bytes().all(|b| b.is_ascii_hexdigit()))
        .then_some(upper)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directml_out_of_memory_in_words() {
        let raw = "Non-zero status code returned while running Conv node. Name:'/b/conv' Status Message: \
                   D:\\a\\_work\\1\\s\\onnxruntime\\core\\providers\\dml\\DmlExecutionProvider\\src\\\
                   MLOperatorAuthorImpl.cpp(2557)\\onnxruntime.dll!00007FF8: (caller: 00007FF8) Exception(3) \
                   tid(1a2c) 887A0001 The application made a call that is invalid.";
        assert_eq!(plain(raw), "not enough GPU memory for this tile size (error 887A0001)");
    }

    #[test]
    fn codes_with_prefix_and_lower_case() {
        assert_eq!(GpuProblem::from_text("HRESULT 0x887a0005"), Some(GpuProblem::DeviceLost));
        assert_eq!(GpuProblem::from_text("failed: 887A0004"), Some(GpuProblem::Unsupported));
        assert_eq!(GpuProblem::from_text("E_OUTOFMEMORY 8007000e"), Some(GpuProblem::OutOfMemory));
        assert!(plain("error 0x887A0006 hung").ends_with("(error 887A0006)"));
    }

    #[test]
    fn tensorrt_out_of_memory_without_code() {
        assert_eq!(
            plain("[TensorRT-RTX] Error: cudaErrorMemoryAllocation: out of memory"),
            "not enough GPU memory for this tile size"
        );
    }

    #[test]
    fn other_texts_stay_as_they_are() {
        let raw = "output has 3 values instead of 12: this is not the expected model";
        assert_eq!(plain(raw), raw);
        // part of a longer hexadecimal number is not a code
        assert_eq!(GpuProblem::from_text("address 00007FF8887A00011"), None);
        assert_eq!(plain(""), "");
    }
}
