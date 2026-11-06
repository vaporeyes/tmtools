// ABOUTME: Writing files so a failure partway never leaves a damaged one: the bytes go to a temporary file beside the
// ABOUTME: target, are flushed to disk, and only then replace it.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes `bytes` to `path` all or nothing: a crash or full disk leaves the old file, or none, never half of the new one
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".part");
    let temp = PathBuf::from(temp);
    let result = std::fs::File::create(&temp)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| std::fs::rename(&temp, path));
    if result.is_err() {
        // Best effort: the error being returned matters more than a leftover temporary file
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_file_and_leaves_no_temporary() {
        let dir = std::env::temp_dir().join(format!("tmtools-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("map.Challenge.Gbx");
        std::fs::write(&path, b"old").unwrap();
        write_atomically(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert!(!dir.join("map.Challenge.Gbx.part").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_write_keeps_the_old_file() {
        // The target is a directory, so the rename fails after the temporary was written
        let dir = std::env::temp_dir().join(format!("tmtools-files-fail-{}", std::process::id()));
        let target = dir.join("existing");
        std::fs::create_dir_all(target.join("inside")).unwrap();
        assert!(write_atomically(&target, b"new").is_err());
        assert!(target.join("inside").is_dir());
        assert!(!dir.join("existing.part").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
