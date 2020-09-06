// Copyright 2020 Theodore Cipicchio
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Windows-only tests.

use std::{
    fs, io,
    path::{Path, PathBuf},
};
use tempfile::{NamedTempFile, TempDir};

#[cfg(feature = "symlink_tests")]
use std::os::windows::fs::{symlink_dir, symlink_file};

/// Creates an empty file at the specified path.
fn create_empty_file(path: &Path) -> io::Result<()> {
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .map(|_| ())
}

/// Creates a temporary non-empty directory.
fn create_temp_non_empty_dir() -> io::Result<PathBuf> {
    let dir_path = TempDir::new()?.into_path();

    create_empty_file(&dir_path.join("foo"))?;
    create_empty_file(&dir_path.join("bar"))?;

    let baz_path = dir_path.join("baz");
    fs::create_dir(&baz_path)?;
    create_empty_file(&baz_path.join("qux"))?;

    Ok(dir_path)
}

/// Generates a path name for temporary symbolic links.
#[cfg(feature = "symlink_tests")]
fn create_temp_symlink_path() -> io::Result<PathBuf> {
    Ok(NamedTempFile::new()?.path().into())
}

/// Tests whether `remove_dir_all` works on a non-empty directory.
#[test]
fn non_empty_directory_works() {
    let dir_path = create_temp_non_empty_dir().unwrap();
    assert!(fs::metadata(&dir_path).unwrap().is_dir());

    super::remove_dir_all(&dir_path).unwrap();
    assert_eq!(
        fs::metadata(&dir_path).err().map(|error| error.kind()),
        Some(io::ErrorKind::NotFound)
    );
}

/// Tests whether `remove_dir_all` rejects regular files.
#[test]
fn file_fails() {
    let (_, file_path) = NamedTempFile::new().unwrap().keep().unwrap();
    assert!(fs::metadata(&file_path).unwrap().is_file());

    assert!(super::remove_dir_all(&file_path).is_err());
    assert!(fs::metadata(&file_path).unwrap().is_file());

    fs::remove_file(&file_path).unwrap();
    assert_eq!(
        fs::metadata(&file_path).err().map(|error| error.kind()),
        Some(io::ErrorKind::NotFound)
    );
}

/// Tests whether `remove_dir_all` rejects targets that don't exist.
#[test]
fn missing_target_fails() {
    let missing_path: PathBuf = NamedTempFile::new().unwrap().path().into();
    assert_eq!(
        fs::metadata(&missing_path).err().map(|error| error.kind()),
        Some(io::ErrorKind::NotFound)
    );

    assert_eq!(
        super::remove_dir_all(&missing_path)
            .err()
            .map(|error| error.kind()),
        Some(io::ErrorKind::NotFound)
    );
}

/// Tests whether `remove_dir_all` works with input paths containing forward slashes.
#[test]
fn unix_path_delimiters_work() {
    let base_dir = TempDir::new().unwrap();

    let mut dir_path = base_dir.path().as_os_str().to_os_string();
    dir_path.push("/foo");
    let dir_path = PathBuf::from(dir_path);

    fs::create_dir(&dir_path).unwrap();
    create_empty_file(&dir_path.join("foo")).unwrap();

    super::remove_dir_all(&dir_path).unwrap();
    assert_eq!(
        fs::metadata(&dir_path).err().map(|error| error.kind()),
        Some(io::ErrorKind::NotFound)
    );
}

/// Tests whether `remove_dir_all` works on a non-empty directory symlink without deleting the
/// target directory itself.
#[test]
#[cfg(feature = "symlink_tests")]
fn non_empty_directory_symlink_works() {
    let dir_path = create_temp_non_empty_dir().unwrap();
    assert!(fs::metadata(&dir_path).unwrap().is_dir());

    // Obvious race condition here as it's possible for another process to create a file with the
    // same name between the point at which the temp file is deleted and the symlink is created, but
    // this should be unlikely enough that it should still work for our tests.
    let symlink_path = create_temp_symlink_path().unwrap();
    symlink_dir(&dir_path, &symlink_path).unwrap();
    assert!(fs::symlink_metadata(&symlink_path)
        .unwrap()
        .file_type()
        .is_symlink());

    super::remove_dir_all(&symlink_path).unwrap();
    assert_eq!(
        fs::metadata(&symlink_path).err().map(|error| error.kind()),
        Some(io::ErrorKind::NotFound)
    );
    assert!(fs::metadata(&dir_path).unwrap().is_dir());

    super::remove_dir_all(&dir_path).unwrap();
    assert_eq!(
        fs::metadata(&dir_path).err().map(|error| error.kind()),
        Some(io::ErrorKind::NotFound)
    );
}

/// Tests whether `remove_dir_all` rejects file symlinks.
#[test]
#[cfg(feature = "symlink_tests")]
fn file_symlink_fails() {
    let (_, file_path) = NamedTempFile::new().unwrap().keep().unwrap();
    assert!(fs::metadata(&file_path).unwrap().is_file());

    // Obvious race condition here as it's possible for another process to create a file with the
    // same name between the point at which the temp file is deleted and the symlink is created, but
    // this should be unlikely enough that it should still work for our tests.
    let symlink_path = create_temp_symlink_path().unwrap();
    symlink_file(&file_path, &symlink_path).unwrap();
    assert!(fs::symlink_metadata(&symlink_path)
        .unwrap()
        .file_type()
        .is_symlink());

    assert_eq!(
        super::remove_dir_all(&symlink_path)
            .err()
            .map(|error| error.kind()),
        Some(io::ErrorKind::InvalidData)
    );

    fs::remove_file(&symlink_path).unwrap();
    assert_eq!(
        fs::symlink_metadata(&symlink_path)
            .err()
            .map(|error| error.kind()),
        Some(io::ErrorKind::NotFound)
    );

    fs::remove_file(&file_path).unwrap();
    assert_eq!(
        fs::metadata(&file_path).err().map(|error| error.kind()),
        Some(io::ErrorKind::NotFound)
    );
}
