//! What `cargo xtask check` runs, as values: the list of tasks and the cargo
//! arguments of each. Pure, so the tests can pin the commands down.

/// A step the user can ask for alone (`--step`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Fmt,
    Build,
    Test,
    Clippy,
    Deny,
}

impl Step {
    pub const NAMES: &'static str = "fmt, build, test, clippy, deny";

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "fmt" => Some(Self::Fmt),
            "build" => Some(Self::Build),
            "test" => Some(Self::Test),
            "clippy" => Some(Self::Clippy),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// Cargo build profile. The tests run in both: release is what the user
/// runs (and the model tests are fast enough only there), debug has the
/// debug assertions of egui and of the standard library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Release,
    Debug,
}

impl Profile {
    /// Folder of the profile inside the target directory.
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Debug => "debug",
        }
    }
}

/// The platform the checks build for. DirectML and TensorRT for RTX are
/// Windows builds: elsewhere the workspace builds without them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Other,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Other
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Fmt,
    Build(Profile),
    /// Puts the DLLs next to the program into `deps\` too, where the test
    /// executables are (Windows only: they load DLLs from their own folder).
    CopyDlls(Profile),
    /// Runs only if the build of the same profile succeeded.
    Test(Profile),
    Clippy,
    Deny,
}

impl Task {
    /// Name of the task in the output and in the summary.
    pub fn label(self) -> String {
        match self {
            Self::Fmt => "fmt".to_owned(),
            Self::Build(p) => format!("build {}", p.dir_name()),
            Self::CopyDlls(p) => format!("dlls {}", p.dir_name()),
            Self::Test(p) => format!("test {}", p.dir_name()),
            Self::Clippy => "clippy".to_owned(),
            Self::Deny => "deny".to_owned(),
        }
    }

    /// Arguments of the cargo command of the task; `None` for the tasks that
    /// are not a cargo command.
    pub fn cargo_args(self, platform: Platform) -> Option<Vec<&'static str>> {
        let features: &[&'static str] = match platform {
            Platform::Windows => &[],
            Platform::Other => &["--no-default-features"],
        };
        let profile = |p: Profile| -> &'static [&'static str] {
            match p {
                Profile::Release => &["--release"],
                Profile::Debug => &[],
            }
        };
        let args: Vec<&'static str> = match self {
            Self::Fmt => vec!["fmt", "--all", "--check"],
            Self::Build(p) => {
                [&["build"], profile(p), &["--workspace", "--all-targets", "--locked"], features].concat()
            }
            // the model tests share one GPU: one at a time
            Self::Test(p) => [
                &["test"],
                profile(p),
                &["--workspace", "--locked", "--no-fail-fast"],
                features,
                &["--", "--test-threads=1"],
            ]
            .concat(),
            Self::Clippy => [
                &["clippy", "--workspace", "--all-targets", "--release", "--locked"],
                features,
                &["--", "-D", "warnings"],
            ]
            .concat(),
            Self::Deny => vec!["deny", "check"],
            Self::CopyDlls(_) => return None,
        };
        Some(args)
    }
}

/// The tasks of a check, in order: everything, or only `only` (a test step
/// builds first, so that the DLLs can be put next to the test executables).
pub fn plan(only: Option<Step>, platform: Platform) -> Vec<Task> {
    let wants = |step: Step| only.is_none_or(|o| o == step);
    let mut tasks = Vec::new();
    if wants(Step::Fmt) {
        tasks.push(Task::Fmt);
    }
    for profile in [Profile::Release, Profile::Debug] {
        if wants(Step::Build) || wants(Step::Test) {
            tasks.push(Task::Build(profile));
            if platform == Platform::Windows {
                tasks.push(Task::CopyDlls(profile));
            }
        }
        if wants(Step::Test) {
            tasks.push(Task::Test(profile));
        }
    }
    if wants(Step::Clippy) {
        tasks.push(Task::Clippy);
    }
    if wants(Step::Deny) {
        tasks.push(Task::Deny);
    }
    tasks
}

#[cfg(test)]
mod tests {
    use super::*;
    use Profile::{Debug, Release};

    #[test]
    fn full_check_on_windows_tests_both_profiles_with_the_dlls() {
        assert_eq!(
            plan(None, Platform::Windows),
            vec![
                Task::Fmt,
                Task::Build(Release),
                Task::CopyDlls(Release),
                Task::Test(Release),
                Task::Build(Debug),
                Task::CopyDlls(Debug),
                Task::Test(Debug),
                Task::Clippy,
                Task::Deny,
            ]
        );
    }

    #[test]
    fn elsewhere_there_are_no_dlls_to_copy() {
        assert!(!plan(None, Platform::Other).iter().any(|t| matches!(t, Task::CopyDlls(_))));
    }

    #[test]
    fn one_step_runs_alone_and_a_test_step_builds_first() {
        assert_eq!(plan(Some(Step::Fmt), Platform::Windows), vec![Task::Fmt]);
        assert_eq!(plan(Some(Step::Deny), Platform::Other), vec![Task::Deny]);
        assert_eq!(
            plan(Some(Step::Test), Platform::Other),
            vec![Task::Build(Release), Task::Test(Release), Task::Build(Debug), Task::Test(Debug)]
        );
        assert_eq!(plan(Some(Step::Build), Platform::Other), vec![Task::Build(Release), Task::Build(Debug)]);
    }

    #[test]
    fn step_names_parse_and_unknown_ones_are_refused() {
        for (name, step) in [
            ("fmt", Step::Fmt),
            ("build", Step::Build),
            ("test", Step::Test),
            ("clippy", Step::Clippy),
            ("deny", Step::Deny),
        ] {
            assert_eq!(Step::parse(name), Some(step));
            assert!(Step::NAMES.contains(name));
        }
        assert_eq!(Step::parse("Test"), None);
        assert_eq!(Step::parse(""), None);
    }

    #[test]
    fn commands_are_locked_and_warnings_are_errors() {
        let joined = |t: Task, p| t.cargo_args(p).map(|a| a.join(" "));
        assert_eq!(
            joined(Task::Test(Release), Platform::Windows).as_deref(),
            Some("test --release --workspace --locked --no-fail-fast -- --test-threads=1")
        );
        assert_eq!(
            joined(Task::Test(Debug), Platform::Other).as_deref(),
            Some("test --workspace --locked --no-fail-fast --no-default-features -- --test-threads=1")
        );
        assert_eq!(
            joined(Task::Build(Debug), Platform::Windows).as_deref(),
            Some("build --workspace --all-targets --locked")
        );
        assert_eq!(
            joined(Task::Clippy, Platform::Other).as_deref(),
            Some("clippy --workspace --all-targets --release --locked --no-default-features -- -D warnings")
        );
        assert_eq!(joined(Task::Fmt, Platform::Windows).as_deref(), Some("fmt --all --check"));
        assert_eq!(joined(Task::Deny, Platform::Windows).as_deref(), Some("deny check"));
        assert_eq!(Task::CopyDlls(Release).cargo_args(Platform::Windows), None);
    }
}
