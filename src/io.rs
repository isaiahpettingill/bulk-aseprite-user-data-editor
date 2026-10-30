#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_FILE: usize = 128 * 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
pub fn supported(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        ext.eq_ignore_ascii_case("ase") || ext.eq_ignore_ascii_case("aseprite")
    })
}
#[cfg(not(target_arch = "wasm32"))]
pub fn load(paths: Vec<PathBuf>) -> Result<Vec<(String, PathBuf, Vec<u8>)>, String> {
    fn walk(
        p: &Path,
        root: &Path,
        out: &mut Vec<(String, PathBuf, Vec<u8>)>,
    ) -> Result<(), String> {
        if out.len() >= 10_000 {
            return Err("Too many files (limit 10,000)".into());
        }
        let meta = std::fs::symlink_metadata(p).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        if meta.is_dir() {
            for child in std::fs::read_dir(p).map_err(|e| e.to_string())? {
                walk(&child.map_err(|e| e.to_string())?.path(), root, out)?;
            }
        } else if supported(&p.to_string_lossy()) {
            if meta.len() > MAX_FILE as u64 {
                return Err(format!("{} exceeds 128 MiB", p.display()));
            }
            if out.iter().map(|(_, _, bytes)| bytes.len()).sum::<usize>() + meta.len() as usize
                > 256 * 1024 * 1024
            {
                return Err("Folder exceeds 256 MiB; open a smaller folder".into());
            }
            out.push((
                p.strip_prefix(root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .into_owned(),
                p.to_owned(),
                std::fs::read(p).map_err(|e| e.to_string())?,
            ));
        }
        Ok(())
    }
    let mut out = Vec::new();
    for p in paths {
        let root = if p.is_dir() {
            p.clone()
        } else {
            p.parent().unwrap_or(Path::new("")).to_owned()
        };
        walk(&p, &root, &mut out)?;
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}
#[cfg(not(target_arch = "wasm32"))]
pub fn save(path: &Path, expected: &[u8], bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if std::fs::read(path).map_err(|e| e.to_string())? != expected {
        return Err("File changed on disk; reopen before saving".into());
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("Save target is no longer a regular file".into());
    }
    let permissions = metadata.permissions();
    let parent = path.parent().ok_or("Missing parent directory")?;
    // Each save gets a unique recovery copy; never overwrite an earlier backup.
    let mut backup = tempfile::Builder::new()
        .prefix(&format!(
            "{}.backup-",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
        .suffix(".bak")
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?;
    backup
        .write_all(expected)
        .and_then(|_| backup.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    backup.keep().map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temp.write_all(bytes).map_err(|e| e.to_string())?;
    temp.as_file()
        .set_permissions(permissions)
        .map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    // Recheck immediately before the atomic replacement.
    if std::fs::read(path).map_err(|e| e.to_string())? != expected {
        return Err("File changed while saving".into());
    }
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
/// Publish a newly merged copy without ever replacing another file.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Missing parent directory")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temp.write_all(bytes).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist_noclobber(path).map_err(|e| {
        format!("Cannot save merged copy (existing files are never overwritten): {e}")
    })?;
    Ok(())
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = editorReport)]
    pub fn report(files: usize, selected: usize);
    #[wasm_bindgen(js_name = editorDirty)]
    pub fn dirty(dirty: bool);
    #[wasm_bindgen(js_name = editorOpen)]
    pub fn open(directory: bool);
    #[wasm_bindgen(js_name = editorPoll)]
    pub fn poll() -> js_sys::Array;
    #[wasm_bindgen(js_name = editorDownload)]
    pub fn download(name: &str, data: &[u8]);
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn merged_copy_save_never_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("merged.aseprite");
        save_new(&path, b"merged").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"merged");
        assert!(save_new(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"merged");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn merged_copy_save_refuses_symlink_target() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original.aseprite");
        std::fs::write(&original, b"original").unwrap();
        let path = dir.path().join("merged.aseprite");
        std::os::unix::fs::symlink(&original, &path).unwrap();
        assert!(save_new(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"original");
    }
    #[test]
    fn save_preserves_recovery_copy_and_refuses_external_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.aseprite");
        std::fs::write(&path, b"original").unwrap();
        save(&path, b"original", b"edited").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"edited");
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "bak"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), b"original");
        std::fs::write(&path, b"external").unwrap();
        assert!(save(&path, b"edited", b"new").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"external");
    }
    #[cfg(unix)]
    #[test]
    fn replacement_preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.aseprite");
        std::fs::write(&path, b"before").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        save(&path, b"before", b"after").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod additional_tests {
    use super::*;
    #[test]
    fn atomic_save_retains_backup_and_rejects_external_change() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sprite.aseprite");
        std::fs::write(&file, b"original").unwrap();
        save(&file, b"original", b"edited").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"edited");
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|s| s == "bak"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(backups[0].path()).unwrap(), b"original");
        assert!(save(&file, b"original", b"wrong").is_err());
        assert_eq!(std::fs::read(file).unwrap(), b"edited");
    }
    #[test]
    fn scan_recurses_and_ignores_backups() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/art.ASEPRITE"), b"bytes").unwrap();
        std::fs::write(dir.path().join("art.aseprite.backup-x.bak"), b"bytes").unwrap();
        let files = load(vec![dir.path().to_owned()]).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].0.ends_with("art.ASEPRITE"));
    }
}
