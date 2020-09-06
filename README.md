win32-remove-dir-all
====================

A [`std::fs::remove_dir_all`] replacement using the Windows Shell and Property System APIs on
Windows.

[![Latest Version](https://img.shields.io/crates/v/win32-remove-dir-all.svg)](https://crates.io/crates/win32-remove-dir-all)
[![Released API docs](https://docs.rs/win32-remove-dir-all/badge.svg)](https://docs.rs/win32-remove-dir-all)
![MIT/Apache-2.0 licensed](https://img.shields.io/crates/l/win32-remove-dir-all.svg)
[![Rustc Version 1.37+](https://img.shields.io/badge/rustc-1.37+-lightgray.svg)](https://blog.rust-lang.org/2019/08/15/Rust-1.37.0.html)
[![Build Status](https://github.com/okready/win32-remove-dir-all/workflows/CI/badge.svg)](https://github.com/okready/win32-remove-dir-all/actions?query=workflow%3ACI)

- [Documentation](https://docs.rs/win32-remove-dir-all)
- [Release notes](https://github.com/okready/win32-remove-dir-all/releases)

The current Windows implementation of `remove_dir_all` in the Rust standard library has a
[long-standing issue] with consistently deleting directories. The [Windows Shell] and [Windows
Property System] APIs both provide methods for recursively deleting directories with consistent
results by way of [`SHFileOperationW`] and [`IFileOperation`], respectively, although a stable
solution compatible with UWP apps has not been settled on in the context of the associated
GitHub issue.

This crate provides a `remove_dir_all` implementation based on both [`SHFileOperationW`] and
[`IFileOperation`], with the former used as a fallback if the latter is not supported
([`IFileOperation`] is recommended over [`SHFileOperationW`], but it is only supported on
Windows Vista and later). For non-Windows platforms, the standard library `remove_dir_all`
function is re-exported for convenience.

Due to the lack of Shell and Property System API support for UWP apps, UWP app developers are
recommended to use the [`remove_dir_all` crate] instead, as it provides an alternative
implementation that does not rely on the Shell or Property System APIs.

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
win32-remove-dir-all = "0.1"
```

## Rust Version Support

The minimum supported Rust version is 1.37. This is largely due to language features used by
dependencies and may be subject to change in later releases.

## Examples

The `remove_dir_all` function provided by this crate can be used as a drop-in replacement for
[`std::fs::remove_dir_all`], even in code targeting multiple platforms; [`std::fs::remove_dir_all`]
will be used automatically on non-Windows targets.

```rust
use std::{error::Error, fs, path::Path};
use win32_remove_dir_all::remove_dir_all;

fn main() -> Result<(), Box<dyn Error>> {
    // Create a directory with a couple files residing in it.
    fs::create_dir("foo")?;
    fs::OpenOptions::new().create(true).write(true).open("foo/bar")?;
    fs::OpenOptions::new().create(true).write(true).open("foo/baz")?;

    // Delete the directory and all its contents as you would with `std::fs::remove_dir_all`.
    remove_dir_all("foo")?;

    Ok(())
}
```

## Disabling Property System ([`IFileOperation`]) Support

Support for [`IFileOperation`] is gated behind the `property_system_api` crate feature, which is
enabled by default. It is not necessary to disable this feature in order to support Windows
versions prior to Windows Vista, as [`SHFileOperationW`] will be used instead if
[`IFileOperation`] is unavailable. It can still be disabled if desired, such as if build times
or sizes are of concern, in which case [`SHFileOperationW`] will always be used regardless of
the Windows version.

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

[`std::fs::remove_dir_all`]: https://doc.rust-lang.org/std/fs/fn.remove_dir_all.html
[long-standing issue]: https://github.com/rust-lang/rust/issues/29497
[Windows Shell]: https://docs.microsoft.com/en-us/previous-versions/windows/desktop/legacy/bb773177(v=vs.85)
[Windows Property System]: https://docs.microsoft.com/en-us/windows/win32/properties/windows-properties-system
[`SHFileOperationW`]: https://docs.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shfileoperationw
[`IFileOperation`]: https://docs.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ifileoperation
[`remove_dir_all` crate]: https://crates.io/crates/remove_dir_all
