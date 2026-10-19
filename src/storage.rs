//! File publication and shared-storage group access for Mod files.
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

pub fn allow_file_access(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut permissions = file.metadata()?.permissions();
        // Android assigns external app files to ext_data_rw, also held by adb/Shizuku.
        permissions.set_mode(permissions.mode() | 0o660);
        file.set_permissions(permissions)?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

pub fn allow_group_access(path: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        let metadata = fs::symlink_metadata(path)?;
        let required = if metadata.is_dir() {
            0o770
        } else if metadata.is_file() {
            0o660
        } else {
            return Ok(false);
        };
        let mut permissions = metadata.permissions();
        let mode = permissions.mode();
        if mode & required == required {
            return Ok(false);
        }
        permissions.set_mode(mode | required);
        fs::set_permissions(path, permissions)?;
        Ok(true)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(false)
    }
}

pub fn repair_permissions(base: &Path) {
    let mut repaired = 0;
    for entry in walkdir::WalkDir::new(base).follow_links(false) {
        match entry {
            Ok(entry) => match allow_group_access(entry.path()) {
                Ok(true) => repaired += 1,
                Ok(false) => {}
                Err(error) => log::warn!("permissions {}: {error}", entry.path().display()),
            },
            Err(error) => log::warn!("Mod directory: {error}"),
        }
    }
    if repaired > 0 {
        log::info!("repaired permissions for {repaired} Mod entries");
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("file has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|()| allow_file_access(file.as_file()))
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_publishes_complete_group_accessible_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chaldea-login.json");
        for bytes in [b"previous response".as_slice(), b"new"] {
            atomic_write(&path, bytes).unwrap();
            assert_eq!(fs::read(&path).unwrap(), bytes);
            #[cfg(unix)]
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o660,
                0o660
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn repair_restores_legacy_permissions_without_following_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("Mod");
        let exports = base.join("exports");
        fs::create_dir_all(&exports).unwrap();
        let file = exports.join("chaldea-login.json");
        fs::write(&file, b"response").unwrap();
        let outside = dir.path().join("private");
        fs::write(&outside, b"outside").unwrap();
        for path in [&file, &outside] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::set_permissions(&exports, fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(&outside, base.join("link")).unwrap();
        repair_permissions(&base);
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o660
        );
        assert_eq!(
            fs::metadata(&exports).unwrap().permissions().mode() & 0o777,
            0o770
        );
        assert_eq!(
            fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read(&file).unwrap(), b"response");
    }
}
