// Copyright 2020 Theodore Cipicchio
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Windows-specific `remove_dir_all` implementation.

mod shell;

#[cfg(feature = "property_system_api")]
mod property;

#[cfg(test)]
mod tests;

use std::{io, iter, os::windows::ffi::OsStrExt, path::Path, ptr};
use winapi::um::fileapi::GetFullPathNameW;

const EXTENDED_PATH_PREFIX: [u16; 4] = [b'\\' as _, b'\\' as _, b'?' as _, b'\\' as _];

/// Resolves the absolute path for the given path string, returning a nul-terminated UTF-16 string.
///
/// `std::fs::canonicalize` is not suitable for our purposes, as it resolves symbolic links
/// automatically, so `GetFullPathNameW` is used instead. Like `canonicalize`, `\\?\` will be added
/// to the input path if not already present to allow for extended-length path names, and will
/// likely be included in the output string as well.
///
/// Note that `GetFullPathNameW` does not check whether the path actually exists, so subsequent
/// operations will need to account for any required existence checks.
fn resolve_absolute_path_utf16(path: &Path) -> io::Result<Vec<u16>> {
    // Convert the path string to UTF-16, adding a leading `\\?\` component if not found.
    let path_str = path.as_os_str();
    let path: Vec<_> = if path_str
        .encode_wide()
        .take(4)
        .eq(EXTENDED_PATH_PREFIX.iter().copied())
    {
        path_str.encode_wide().chain(iter::once(0)).collect()
    } else {
        EXTENDED_PATH_PREFIX
            .iter()
            .copied()
            .chain(path_str.encode_wide())
            .chain(iter::once(0))
            .collect()
    };

    // Call `GetFullPathNameW` once to get the buffer size needed, and again to actually resolve the
    // file name. This may require multiple attempts if the current directory is changed by another
    // thread in between calls.
    let mut absolute_path = Vec::new();
    let mut capacity = 0;
    loop {
        let result = unsafe {
            GetFullPathNameW(
                path.as_ptr(),
                capacity,
                absolute_path.as_mut_ptr(),
                ptr::null_mut(),
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }

        if result < capacity {
            unsafe {
                absolute_path.set_len(result as usize + 1);
            }
            debug_assert_eq!(absolute_path[result as usize], 0);

            return Ok(absolute_path);
        }

        absolute_path.reserve_exact(result as usize);
        capacity = result;
    }
}

/// Strips any `\\?\` prefix (used for extended-length paths) from a UTF-16 path string.
///
/// Both `SHFileOperationW` and `SHCreateItemFromParsingName` do not accept paths with a `\\?\`
/// prefix, so it must be stripped before processing.
fn strip_extended_length_path_prefix(path: &[u16]) -> &[u16] {
    if path.len() >= 4 && path[..4] == EXTENDED_PATH_PREFIX {
        &path[4..]
    } else {
        path
    }
}

/// Removes a directory at this path, after removing all its contents. Use
/// carefully!
///
/// This function does **not** follow symbolic links and it will simply remove the
/// symbolic link itself.
///
/// # Platform-specific behavior
///
/// This function currently corresponds to `opendir`, `lstat`, `rm` and `rmdir` functions on Unix,
/// and either the `SHFileOperation` function or the `IFileOperation` COM interface on Windows
/// depending on the Windows version used at runtime.
/// Note that, this [may change in the future][changes].
///
/// [changes]: ../io/index.html#platform-specific-behavior
///
/// # Errors
///
/// See [`fs::remove_file`] and [`fs::remove_dir`].
///
/// [`fs::remove_file`]:  fn.remove_file.html
/// [`fs::remove_dir`]: fn.remove_dir.html
///
/// # Examples
///
/// ```no_run
/// use std::fs;
///
/// fn main() -> std::io::Result<()> {
///     fs::remove_dir_all("/some/dir")?;
///     Ok(())
/// }
/// ```
pub fn remove_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();

    #[cfg(feature = "property_system_api")]
    {
        if property::remove_dir_all(path)?.is_some() {
            return Ok(());
        }
    }

    shell::remove_dir_all(path)
}
