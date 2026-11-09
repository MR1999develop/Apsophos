//! Which lines of the tools' output go into the summary: errors and warnings
//! with the `-->` line that says where they are, failed tests, panics, test
//! totals, format differences and cargo-deny's verdicts (as in the
//! summary of the PowerShell check before it).

/// `error: …`, `warning: …`, `error[E0308]: …`, `warning[E0170]: …`.
fn is_diagnostic(line: &str, kind: &str) -> bool {
    let Some(rest) = line.strip_prefix(kind) else { return false };
    let rest = match rest.strip_prefix('[') {
        Some(code) => match code.split_once(']') {
            Some((name, after)) if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                after
            }
            _ => return false,
        },
        None => rest,
    };
    rest.starts_with(':')
}

/// A warning of the compiler or of Clippy, not the "generated N warnings"
/// total that follows them.
pub fn is_warning(line: &str) -> bool {
    is_diagnostic(line, "warning") && !(line.contains(" generated ") && line.contains(" warning"))
}

pub fn is_error(line: &str) -> bool {
    is_diagnostic(line, "error")
}

pub fn is_important(line: &str) -> bool {
    let trimmed = line.trim_start();
    let indented = trimmed.len() < line.len();
    is_error(line)
        || is_diagnostic(line, "warning")
        || (indented && trimmed.starts_with("--> "))
        || line.contains("panicked at")
        || (line.starts_with("test ") && line.ends_with(" ... FAILED"))
        || (line.starts_with("---- ") && line.ends_with(" stdout ----"))
        || line.starts_with("test result: ")
        || line.starts_with("Diff in ")
        || (indented && trimmed.starts_with("Finished "))
        || ["failed ", "rejected ", "advisories ", "bans ", "licenses ", "sources "]
            .iter()
            .any(|word| trimmed.starts_with(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_with_and_without_a_code() {
        assert!(is_error("error: could not compile `apsophos`"));
        assert!(is_error("error[E0308]: mismatched types"));
        assert!(is_warning("warning: unused variable: `x`"));
        assert!(is_warning("warning[E0170]: x"));
        assert!(!is_warning("warning[clippy::x]: not a code"));
        assert!(!is_error("error[]: x"));
        assert!(!is_error("error[E 1]: x"));
        assert!(!is_error("errors: 3"));
        assert!(!is_error("  error: indented, part of a message"));
    }

    #[test]
    fn the_warning_total_is_not_a_warning_but_is_kept() {
        let total = "warning: `apsophos` (bin \"apsophos\") generated 2 warnings";
        assert!(!is_warning(total));
        assert!(is_important(total));
    }

    #[test]
    fn lines_kept_for_the_summary() {
        for line in [
            "   --> crates/app/src/main.rs:10:5",
            "thread 'x' panicked at crates/core/src/lib.rs:3:1:",
            "test tiles::seams ... FAILED",
            "---- tiles::seams stdout ----",
            "test result: ok. 12 passed; 0 failed; 0 ignored",
            "Diff in \\\\?\\D:\\x\\src\\lib.rs:3:",
            "    Finished `release` profile [optimized] target(s) in 1.2s",
            "advisories ok, bans ok, licenses ok, sources ok",
            "    advisories ok",
        ] {
            assert!(is_important(line), "{line}");
        }
        for line in ["   Compiling denoise_core v0.1.0", "test tiles::seams ... ok", "running 3 tests", "", "Finished"]
        {
            assert!(!is_important(line), "{line}");
        }
    }
}
