//! The operating-system calls the program needs that the standard library
//! does not offer, behind safe functions. The only crate of the workspace
//! with `unsafe` code: one block per Windows call, each with its `SAFETY`
//! comment. Miri cannot check these blocks (it does not run foreign calls);
//! the tests below run the real calls.

use std::io;
use std::path::Path;

/// Restricts where Windows looks for DLLs loaded by name — by the program,
/// by ONNX Runtime (which loads `DirectML.dll` and the TensorRT for RTX
/// libraries that way) or by their dependencies — to the program's folder
/// and System32. Without it the current folder and the `PATH` are searched
/// too, and a DLL planted there would run inside the program.
///
/// Call it first thing in `main`, before any window or library is created;
/// it cannot be undone. Does nothing on other systems.
pub fn restrict_dll_search() -> io::Result<()> {
    #[cfg(windows)]
    {
        windows::restrict_dll_search()
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

/// Renames `from` to `to`, replacing `to`, so that the new name survives a
/// power cut: on Windows with `MOVEFILE_WRITE_THROUGH`, elsewhere followed by
/// a sync of the folder. Both must be on the same volume (the rename is
/// then atomic); the data of `from` must already be synced.
pub fn rename_durably(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows::rename_durably(from, to)
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(from, to)?;
        let dir = to.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
        std::fs::File::open(dir)?.sync_all()
    }
}

#[cfg(windows)]
mod windows {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH};
    use windows_sys::Win32::System::LibraryLoader::{
        SetDefaultDllDirectories, LOAD_LIBRARY_SEARCH_APPLICATION_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
    };

    #[expect(unsafe_code, reason = "a Win32 call with no safe equivalent")]
    pub fn restrict_dll_search() -> io::Result<()> {
        let flags = LOAD_LIBRARY_SEARCH_APPLICATION_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;
        // SAFETY: the call takes one flags value and no pointers; any value
        // is valid input, and a failure is reported through the result.
        let ok = unsafe { SetDefaultDllDirectories(flags) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    #[expect(unsafe_code, reason = "a Win32 call with no safe equivalent")]
    pub fn rename_durably(from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (verbatim(from)?, verbatim(to)?);
        // SAFETY: both pointers point to NUL-terminated UTF-16 strings
        // (without other NULs, checked by `verbatim`) owned by this frame,
        // alive until the call returns; MoveFileExW only reads them.
        let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// `path` made absolute, in the `\\?\` form (no 260-character limit, no
    /// reinterpretation of the name), as a NUL-terminated wide string. A
    /// path with a NUL inside is refused: Windows would cut it there.
    fn verbatim(path: &Path) -> io::Result<Vec<u16>> {
        let absolute = std::path::absolute(path)?;
        let wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "path with a NUL character"));
        }
        let bs = u16::from(b'\\');
        let starts = |prefix: &str| wide.starts_with(&prefix.encode_utf16().collect::<Vec<_>>());
        let mut out: Vec<u16> = if starts(r"\\?\") || starts(r"\\.\") {
            wide
        } else if wide.len() > 2 && wide[0] == bs && wide[1] == bs {
            // \\server\share\… → \\?\UNC\server\share\…
            r"\\?\UNC\".encode_utf16().chain(wide[2..].iter().copied()).collect()
        } else {
            r"\\?\".encode_utf16().chain(wide).collect()
        };
        out.push(0);
        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn paths_become_verbatim() {
            let text = |v: Vec<u16>| String::from_utf16_lossy(&v[..v.len() - 1]);
            assert_eq!(text(verbatim(Path::new(r"C:\a\..\b.txt")).unwrap()), r"\\?\C:\b.txt");
            assert_eq!(text(verbatim(Path::new(r"\\srv\share\x.tif")).unwrap()), r"\\?\UNC\srv\share\x.tif");
            assert_eq!(text(verbatim(Path::new(r"\\?\C:\x")).unwrap()), r"\\?\C:\x");
            assert!(verbatim(Path::new("C:\\a\0b")).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_replaces_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("new.tmp"), dir.path().join("photo.tif"));
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&to, b"old").unwrap();
        rename_durably(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"new");
        assert!(!from.exists());
    }

    #[test]
    fn rename_works_beyond_260_characters() {
        let dir = tempfile::tempdir().unwrap();
        let mut deep = dir.path().to_path_buf();
        for _ in 0..6 {
            deep.push("a-folder-with-a-rather-long-name-to-pass-the-old-limit");
        }
        std::fs::create_dir_all(&deep).unwrap();
        let (from, to) = (deep.join("x.tmp"), deep.join("photo-with-a-long-name.tif"));
        assert!(to.as_os_str().len() > 300);
        std::fs::write(&from, b"data").unwrap();
        rename_durably(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"data");
    }

    #[test]
    fn a_missing_source_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(rename_durably(&dir.path().join("none"), &dir.path().join("x")).is_err());
    }

    #[test]
    fn dll_search_can_be_restricted() {
        // per process and irreversible: harmless in a test program
        restrict_dll_search().unwrap();
    }
}
