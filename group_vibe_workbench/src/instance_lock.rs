//! Single instance lock mechanism
//!
//! Ensures only one instance of the application can run in release mode.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::os::unix::fs::OpenOptionsExt;

/// Single instance lock
pub struct InstanceLock {
    lock_file_path: PathBuf,
    _lock_file: Option<File>,
}

impl InstanceLock {
    /// Try to acquire the instance lock
    ///
    /// Returns Ok(InstanceLock) if successful, Err if another instance is running
    pub fn try_acquire() -> io::Result<Self> {
        // In debug mode, always allow multiple instances
        if cfg!(debug_assertions) {
            return Ok(Self {
                lock_file_path: PathBuf::new(),
                _lock_file: None,
            });
        }

        // In release mode, enforce single instance
        let lock_file_path = Self::get_lock_file_path();

        // Ensure parent directory exists
        if let Some(parent) = lock_file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Try to create and lock the file
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::io::AsRawFd;

            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&lock_file_path)?;

            // Try to acquire exclusive lock (non-blocking)
            let fd = file.as_raw_fd();
            let result = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };

            if result != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Another instance is already running",
                ));
            }

            // Write PID to lock file
            let mut file = file;
            writeln!(file, "{}", std::process::id())?;
            file.flush()?;

            Ok(Self {
                lock_file_path,
                _lock_file: Some(file),
            })
        }

        #[cfg(target_os = "windows")]
        {
            // Windows implementation using file creation
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_file_path)
            {
                Ok(mut file) => {
                    writeln!(file, "{}", std::process::id())?;
                    file.flush()?;
                    Ok(Self {
                        lock_file_path,
                        _lock_file: Some(file),
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "Another instance is already running",
                    ))
                }
                Err(e) => Err(e),
            }
        }

        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;

            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&lock_file_path)?;

            // Try to acquire exclusive lock (non-blocking)
            let fd = file.as_raw_fd();
            let result = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };

            if result != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Another instance is already running",
                ));
            }

            // Write PID to lock file
            let mut file = file;
            writeln!(file, "{}", std::process::id())?;
            file.flush()?;

            Ok(Self {
                lock_file_path,
                _lock_file: Some(file),
            })
        }
    }

    /// Get the lock file path
    fn get_lock_file_path() -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            // Use ~/Library/Application Support/GroupVibeWorkbench/
            if let Some(home) = dirs::home_dir() {
                home.join("Library")
                    .join("Application Support")
                    .join("GroupVibeWorkbench")
                    .join(".lock")
            } else {
                PathBuf::from("/tmp/group_vibe_workbench.lock")
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Use %APPDATA%\GroupVibeWorkbench\
            if let Some(appdata) = dirs::data_dir() {
                appdata.join("GroupVibeWorkbench").join(".lock")
            } else {
                PathBuf::from("C:\\Temp\\group_vibe_workbench.lock")
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Use ~/.local/share/group-vibe-workbench/
            if let Some(data_dir) = dirs::data_dir() {
                data_dir.join("group-vibe-workbench").join(".lock")
            } else {
                PathBuf::from("/tmp/group_vibe_workbench.lock")
            }
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // In debug mode, no lock file to clean up
        if cfg!(debug_assertions) {
            return;
        }

        // Clean up lock file
        if self.lock_file_path.exists() {
            let _ = std::fs::remove_file(&self.lock_file_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_file_path() {
        let path = InstanceLock::get_lock_file_path();
        assert!(path.to_string_lossy().contains("group"));
    }

    #[test]
    fn test_acquire_lock_in_debug() {
        // In debug mode, should always succeed
        if cfg!(debug_assertions) {
            let lock1 = InstanceLock::try_acquire();
            assert!(lock1.is_ok());

            let lock2 = InstanceLock::try_acquire();
            assert!(lock2.is_ok());
        }
    }
}
