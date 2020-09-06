// Copyright 2020 Theodore Cipicchio
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! `remove_dir_all` implementation using `SHFileOperationW` from the Shell API.

use super::{resolve_absolute_path_utf16, strip_extended_length_path_prefix};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::{convert::TryFrom, io, path::Path, ptr};
use winapi::{
    shared::minwindef::FALSE,
    um::{
        fileapi::{GetFileAttributesW, INVALID_FILE_ATTRIBUTES},
        shellapi::{SHFileOperationW, FOF_NO_UI, FO_DELETE, SHFILEOPSTRUCTW},
        winnt::FILE_ATTRIBUTE_DIRECTORY,
    },
};

/// Non-standard `SHFileOperation` error codes, as described at
/// https://docs.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shfileoperationw.
///
/// These take precedence over any `Winerror.h` codes returned from `SHFileOperation`.
#[allow(non_camel_case_types)]
#[derive(
    Clone, Copy, Debug, Eq, Hash, IntoPrimitive, Ord, PartialEq, PartialOrd, TryFromPrimitive,
)]
#[repr(i32)]
enum ErrorCode {
    DE_SAMEFILE = 0x71,
    DE_MANYSRC1DEST = 0x72,
    DE_DIFFDIR = 0x73,
    DE_ROOTDIR = 0x74,
    DE_OPCANCELLED = 0x75,
    DE_DESTSUBTREE = 0x76,
    DE_ACCESSDENIEDSRC = 0x78,
    DE_PATHTOODEEP = 0x79,
    DE_MANYDEST = 0x7A,
    DE_INVALIDFILES = 0x7C,
    DE_DESTSAMETREE = 0x7D,
    DE_FLDDESTISFILE = 0x7E,
    DE_FILEDESTISFLD = 0x80,
    DE_FILENAMETOOLONG = 0x81,
    DE_DEST_IS_CDROM = 0x82,
    DE_DEST_IS_DVD = 0x83,
    DE_DEST_IS_CDRECORD = 0x84,
    DE_FILE_TOO_LARGE = 0x85,
    DE_SRC_IS_CDROM = 0x86,
    DE_SRC_IS_DVD = 0x87,
    DE_SRC_IS_CDRECORD = 0x88,
    DE_ERROR_MAX = 0xB7,
    UNKNOWN = 0x402,
    ERRORONDEST = 0x10000,
    DE_ROOTDIR_ERRORONDEST = 0x10074,
}

impl ErrorCode {
    /// Returns the description associated with this error code.
    fn description(self) -> &'static str {
        match self {
            Self::DE_SAMEFILE => "The source and destination files are the same file.",
            Self::DE_MANYSRC1DEST => "Multiple file paths were specified in the source buffer, but only one destination file path.",
            Self::DE_DIFFDIR => "Rename operation was specified but the destination path is a different directory. Use the move operation instead.",
            Self::DE_ROOTDIR => "The source is a root directory, which cannot be moved or renamed.",
            Self::DE_OPCANCELLED => "The operation was canceled.",
            Self::DE_DESTSUBTREE => "The destination is a subtree of the source.",
            Self::DE_ACCESSDENIEDSRC => "Security settings denied access to the source.",
            Self::DE_PATHTOODEEP => "The source or destination path exceeded or would exceed MAX_PATH.",
            Self::DE_MANYDEST => "The operation involved multiple destination paths, which can fail in the case of a move operation.",
            Self::DE_INVALIDFILES => "The path in the source or destination or both was invalid.",
            Self::DE_DESTSAMETREE => "The source and destination have the same parent folder.",
            Self::DE_FLDDESTISFILE => "The destination path is an existing file.",
            Self::DE_FILEDESTISFLD => "The destination path is an existing folder.",
            Self::DE_FILENAMETOOLONG => "The name of the file exceeds MAX_PATH.",
            Self::DE_DEST_IS_CDROM => "The destination is a read-only CD-ROM, possibly unformatted.",
            Self::DE_DEST_IS_DVD => "The destination is a read-only DVD, possibly unformatted.",
            Self::DE_DEST_IS_CDRECORD => "The destination is a writable CD-ROM, possibly unformatted.",
            Self::DE_FILE_TOO_LARGE => "The file involved in the operation is too large for the destination media or file system.",
            Self::DE_SRC_IS_CDROM => "The source is a read-only CD-ROM, possibly unformatted.",
            Self::DE_SRC_IS_DVD => "The source is a read-only DVD, possibly unformatted.",
            Self::DE_SRC_IS_CDRECORD => "The source is a writable CD-ROM, possibly unformatted.",
            Self::DE_ERROR_MAX => "MAX_PATH was exceeded during the operation.",
            Self::UNKNOWN => "An unknown error occurred. This is typically due to an invalid path in the source or destination.",
            Self::ERRORONDEST => "An unspecified error occurred on the destination.",
            Self::DE_ROOTDIR_ERRORONDEST => "Destination is a root directory and cannot be renamed.",
        }
    }

    /// Returns an [`io::ErrorKind`] suitable for this error code.
    ///
    /// [`io::ErrorKind`]: https://doc.rust-lang.org/std/io/enum.ErrorKind.html
    fn error_kind(self) -> io::ErrorKind {
        match self {
            Self::DE_SAMEFILE => io::ErrorKind::InvalidInput,
            Self::DE_MANYSRC1DEST => io::ErrorKind::InvalidInput,
            Self::DE_DIFFDIR => io::ErrorKind::InvalidInput,
            Self::DE_ROOTDIR => io::ErrorKind::InvalidInput,
            Self::DE_OPCANCELLED => io::ErrorKind::Interrupted,
            Self::DE_DESTSUBTREE => io::ErrorKind::InvalidInput,
            Self::DE_ACCESSDENIEDSRC => io::ErrorKind::PermissionDenied,
            Self::DE_PATHTOODEEP => io::ErrorKind::InvalidInput,
            Self::DE_MANYDEST => io::ErrorKind::InvalidInput,
            Self::DE_INVALIDFILES => io::ErrorKind::NotFound,
            Self::DE_DESTSAMETREE => io::ErrorKind::InvalidInput,
            Self::DE_FLDDESTISFILE => io::ErrorKind::AlreadyExists,
            Self::DE_FILEDESTISFLD => io::ErrorKind::AlreadyExists,
            Self::DE_FILENAMETOOLONG => io::ErrorKind::InvalidInput,
            Self::DE_DEST_IS_CDROM => io::ErrorKind::PermissionDenied,
            Self::DE_DEST_IS_DVD => io::ErrorKind::PermissionDenied,
            Self::DE_DEST_IS_CDRECORD => io::ErrorKind::PermissionDenied,
            Self::DE_FILE_TOO_LARGE => io::ErrorKind::InvalidData,
            Self::DE_SRC_IS_CDROM => io::ErrorKind::PermissionDenied,
            Self::DE_SRC_IS_DVD => io::ErrorKind::PermissionDenied,
            Self::DE_SRC_IS_CDRECORD => io::ErrorKind::PermissionDenied,
            Self::DE_ERROR_MAX => io::ErrorKind::Other,
            Self::UNKNOWN => io::ErrorKind::Other,
            Self::ERRORONDEST => io::ErrorKind::Other,
            Self::DE_ROOTDIR_ERRORONDEST => io::ErrorKind::InvalidInput,
        }
    }
}

/// Deletes a directory and all of its contenst using `SHFileOperationW`.
pub fn remove_dir_all(path: &Path) -> io::Result<()> {
    // `SHFileOperationW` requires the input string to be double nul-terminated, as single nul
    // characters are used to delimit multiple path input.
    let mut path = resolve_absolute_path_utf16(path)?;
    path.push(0);

    // Make sure the target is a directory or a directory symlink. Since Windows distinguishes
    // between file and folder symbolic links, `FILE_ATTRIBUTE_DIRECTORY` should be set regardless
    // of whether the target is a symbolic link.
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(io::Error::last_os_error());
    }

    if (attributes & FILE_ATTRIBUTE_DIRECTORY) == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Target is not a directory or directory symlink.",
        ));
    }

    let mut file_op = SHFILEOPSTRUCTW {
        hwnd: ptr::null_mut(),
        wFunc: FO_DELETE.into(),
        pFrom: strip_extended_length_path_prefix(&path).as_ptr(),
        pTo: ptr::null(),
        fFlags: FOF_NO_UI,
        fAnyOperationsAborted: FALSE,
        hNameMappings: ptr::null_mut(),
        lpszProgressTitle: ptr::null(),
    };
    let result = unsafe { SHFileOperationW(&mut file_op) };
    if result != 0 {
        return Err(if let Ok(code) = ErrorCode::try_from(result) {
            io::Error::new(code.error_kind(), code.description())
        } else {
            io::Error::from_raw_os_error(result)
        });
    }

    if file_op.fAnyOperationsAborted != FALSE {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Operation aborted before completion.",
        ));
    }

    Ok(())
}
