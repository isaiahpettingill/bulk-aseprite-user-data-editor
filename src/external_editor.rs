//! Bounded native editor discovery. Nothing is launched during discovery.
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug)]
pub struct EditorCommand {
    executable: PathBuf,
    name: &'static str,
}

impl EditorCommand {
    #[cfg(test)]
    pub fn for_test(executable: PathBuf) -> Self {
        Self {
            executable,
            name: "Aseprite",
        }
    }

    pub fn label(&self) -> &'static str {
        self.name
    }

    fn command(&self, path: &Path) -> Result<Command, String> {
        if !is_executable(&self.executable) {
            return Err(format!(
                "{} is no longer available. Restart to detect installed editors.",
                self.name
            ));
        }
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !extension.eq_ignore_ascii_case("ase") && !extension.eq_ignore_ascii_case("aseprite") {
            return Err("Only .ase and .aseprite files can be opened in the sprite editor.".into());
        }
        let absolute = path
            .canonicalize()
            .map_err(|e| format!("Cannot open file: {e}"))?;
        if !absolute.is_file() {
            return Err("Select an existing sprite file, not a directory.".into());
        }
        // An absolute path cannot be mistaken for a command-line option. Each
        // path is exactly one argument; shell metacharacters are never evaluated.
        let mut command = Command::new(&self.executable);
        command.arg(absolute);
        Ok(command)
    }

    pub fn launch(&self, path: &Path) -> Result<(), String> {
        let mut command = self.command(path)?;
        restore_loader_environment(
            &mut command,
            std::env::var_os("ASEPRITE_METADATA_APPIMAGE").as_deref(),
            std::env::var_os("ASEPRITE_METADATA_ORIGINAL_LD_LIBRARY_PATH").as_deref(),
        );
        let mut child = command
            .spawn()
            .map_err(|e| format!("Could not start {}: {e}", self.name))?;
        // Reap without blocking the UI or closing the editor when our app exits.
        let _ = std::thread::Builder::new()
            .name("external-editor".into())
            .spawn(move || {
                let _ = child.wait();
            });
        Ok(())
    }
}

// Only undo our own AppRun modification, never a user's ordinary environment.
// Arguments make the behavior testable without mutating process-global state.
fn restore_loader_environment(
    command: &mut Command,
    appimage_marker: Option<&std::ffi::OsStr>,
    original: Option<&std::ffi::OsStr>,
) {
    if appimage_marker != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    match original {
        Some(value) => {
            command.env("LD_LIBRARY_PATH", value);
        }
        None => {
            command.env_remove("LD_LIBRARY_PATH");
        }
    }
    command.env_remove("ASEPRITE_METADATA_APPIMAGE");
    command.env_remove("ASEPRITE_METADATA_ORIGINAL_LD_LIBRARY_PATH");
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn first_available(
    name: &'static str,
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Option<EditorCommand> {
    candidates
        .into_iter()
        .filter(|p| is_executable(p))
        .find_map(|p| p.canonicalize().ok())
        .map(|executable| EditorCommand { executable, name })
}

fn candidates(binary: &str, application: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(search) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&search) {
            // Do not implicitly execute a same-named file in a project folder.
            if !directory.is_absolute() {
                continue;
            }
            #[cfg(windows)]
            {
                paths.push(directory.join(format!("{binary}.exe")));
                paths.push(directory.join(format!("{application}.exe")));
            }
            #[cfg(not(windows))]
            paths.push(directory.join(binary));
        }
    }
    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/bin").join(binary));
        paths.push(PathBuf::from("/usr/local/bin").join(binary));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            paths.push(home.join(".local/bin").join(binary));
            for library in [
                ".steam/steam/steamapps/common",
                ".local/share/Steam/steamapps/common",
            ] {
                paths.push(home.join(library).join(application).join(binary));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let suffix = format!("{application}.app/Contents/MacOS/{binary}");
        paths.push(PathBuf::from("/Applications").join(&suffix));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            paths.push(home.join("Applications").join(&suffix));
            paths.push(
                home.join("Library/Application Support/Steam/steamapps/common")
                    .join(application)
                    .join(&suffix),
            );
        }
    }
    #[cfg(windows)]
    {
        for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(variable).map(PathBuf::from) {
                paths.push(base.join(application).join(format!("{application}.exe")));
                paths.push(
                    base.join("Programs")
                        .join(application)
                        .join(format!("{application}.exe")),
                );
                paths.push(
                    base.join("Steam/steamapps/common")
                        .join(application)
                        .join(format!("{application}.exe")),
                );
            }
        }
    }
    paths
}

/// Prefer Aseprite even if LibreSprite appears earlier on PATH. Discovery only
/// checks a bounded set of installation locations; it never scans directories.
pub fn discover() -> Option<EditorCommand> {
    first_available("Aseprite", candidates("aseprite", "Aseprite"))
        .or_else(|| first_available("LibreSprite", candidates("libresprite", "LibreSprite")))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn executable(root: &Path) -> PathBuf {
        let path = root.join("fake editor.exe");
        std::fs::write(&path, b"fixture; never execute").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }
    #[test]
    fn filename_is_one_argument_with_no_shell_interpolation() {
        let dir = tempfile::tempdir().unwrap();
        let executable = executable(dir.path());
        let editor = first_available("Aseprite", [executable.clone()]).unwrap();
        // These characters are legal on Windows too; no command is executed.
        let path = dir.path().join("--tile & $(echo PWNED) %PATH%.ASEPRITE");
        std::fs::write(&path, []).unwrap();
        let command = editor.command(&path).unwrap();
        let expected_program = executable.canonicalize().unwrap();
        let expected_path = path.canonicalize().unwrap();
        assert_eq!(command.get_program(), expected_program.as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![expected_path.as_os_str()]
        );
        assert_eq!(editor.label(), "Aseprite");
    }
    #[test]
    fn discovery_does_not_accept_directories_or_missing_candidates() {
        let dir = tempfile::tempdir().unwrap();
        assert!(first_available(
            "Aseprite",
            [dir.path().to_owned(), dir.path().join("missing")]
        )
        .is_none());
    }
    #[test]
    fn invalid_targets_are_rejected_without_launching() {
        let dir = tempfile::tempdir().unwrap();
        let editor = first_available("Aseprite", [executable(dir.path())]).unwrap();
        assert!(editor.command(&dir.path().join("missing.ase")).is_err());
        let wrong = dir.path().join("other.png");
        std::fs::write(&wrong, []).unwrap();
        assert!(editor.command(&wrong).is_err());
        let folder = dir.path().join("folder.ase");
        std::fs::create_dir(&folder).unwrap();
        assert!(editor.command(&folder).is_err());
    }
    #[test]
    fn removed_editor_returns_error_before_launch() {
        let dir = tempfile::tempdir().unwrap();
        let path = executable(dir.path());
        let editor = first_available("LibreSprite", [path.clone()]).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(editor.command(&dir.path().join("anything.ase")).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn nonexecutable_file_is_not_an_editor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("aseprite");
        std::fs::write(&path, []).unwrap();
        assert!(first_available("Aseprite", [path]).is_none());
    }
    #[test]
    fn external_editor_restores_original_loader_path_without_shell_expansion() {
        use std::ffi::OsStr;
        let mut command = Command::new("never-executed");
        let original = OsStr::new("/user/library with spaces:$ORIGIN:/other");
        restore_loader_environment(&mut command, Some(OsStr::new("1")), Some(original));
        let changes: std::collections::BTreeMap<_, _> = command.get_envs().collect();
        assert_eq!(
            changes.get(OsStr::new("LD_LIBRARY_PATH")),
            Some(&Some(original))
        );
        assert_eq!(
            changes.get(OsStr::new("ASEPRITE_METADATA_APPIMAGE")),
            Some(&None)
        );
        assert_eq!(
            changes.get(OsStr::new("ASEPRITE_METADATA_ORIGINAL_LD_LIBRARY_PATH")),
            Some(&None)
        );
    }
    #[test]
    fn external_editor_preserves_unset_versus_empty_loader_path() {
        use std::ffi::OsStr;
        for original in [None, Some(OsStr::new(""))] {
            let mut command = Command::new("never-executed");
            restore_loader_environment(&mut command, Some(OsStr::new("1")), original);
            let changes: std::collections::BTreeMap<_, _> = command.get_envs().collect();
            assert_eq!(changes.get(OsStr::new("LD_LIBRARY_PATH")), Some(&original));
        }
    }
    #[test]
    fn non_appimage_launch_leaves_environment_unchanged() {
        use std::ffi::OsStr;
        for marker in [None, Some(OsStr::new("unrecognized"))] {
            let mut command = Command::new("never-executed");
            restore_loader_environment(&mut command, marker, Some(OsStr::new("ignored")));
            assert_eq!(command.get_envs().count(), 0);
        }
    }
}
