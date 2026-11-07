//! What differs between operating systems in the look of the window: the
//! interface and icon fonts. The only place with `cfg(windows)` for the
//! interface.

/// The two faces of the system interface font, and its icon font.
pub struct UiFonts {
    pub regular: Vec<u8>,
    pub semibold: Vec<u8>,
    pub icons: Option<Vec<u8>>,
}

/// Segoe UI and the icon font, read from the Windows fonts folder (never
/// shipped: their licence does not allow it); `None` elsewhere or when
/// Segoe UI is missing, and the Inter shipped with the program is used.
#[cfg(windows)]
pub fn ui_fonts() -> Option<UiFonts> {
    let dir = std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from).join("Fonts");
    let regular = std::fs::read(dir.join("segoeui.ttf")).ok()?;
    let semibold = std::fs::read(dir.join("seguisb.ttf")).ok()?;
    // Windows 11 also has Segoe Fluent Icons, whose glyphs differ at some
    // code points: one font everywhere keeps the icons as checked.
    let icons = std::fs::read(dir.join("segmdl2.ttf")).ok();
    Some(UiFonts { regular, semibold, icons })
}

#[cfg(not(windows))]
pub fn ui_fonts() -> Option<UiFonts> {
    None
}
