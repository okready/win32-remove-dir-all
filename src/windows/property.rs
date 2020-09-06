// Copyright 2020 Theodore Cipicchio
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! `remove_dir_all` implementation using `IFileOperation` from the Property System API.

use super::{resolve_absolute_path_utf16, strip_extended_length_path_prefix};
use std::{
    cell::Cell,
    io, mem,
    ops::Deref,
    os::raw::{c_char, c_void},
    path::Path,
    ptr::{self, NonNull},
    sync::Once,
    thread,
};
use winapi::{
    shared::{
        guiddef::REFIID,
        minwindef::{BOOL, DWORD, FALSE, ULONG},
        windef::HWND,
        winerror::{E_NOINTERFACE, S_OK},
        wtypesbase::CLSCTX_INPROC_SERVER,
    },
    um::{
        combaseapi::{CoCreateInstance, CoInitializeEx, CoUninitialize},
        libloaderapi::{GetModuleHandleW, GetProcAddress},
        objbase::COINIT_APARTMENTTHREADED,
        objidl::IBindCtx,
        shellapi::FOF_NO_UI,
        shobjidl_core::{FileOperation, IShellItem},
        unknwnbase::{IUnknown, IUnknownVtbl},
        winnt::{HRESULT, LPCWSTR, PCWSTR},
    },
    Class, Interface, RIDL,
};

/// Type signature for `SHCreateItemFromParsingName`.
type SHCreateItemFromParsingNameFn =
    unsafe extern "system" fn(PCWSTR, *mut IBindCtx, REFIID, *mut *mut c_void) -> HRESULT;

type LPBC = *mut IBindCtx;

const STR_PARSE_PREFER_FOLDER_BROWSING: &[u16] = &[
    b'P' as _, b'a' as _, b'r' as _, b's' as _, b'e' as _, b' ' as _, b'P' as _, b'r' as _,
    b'e' as _, b'f' as _, b'e' as _, b'r' as _, b' ' as _, b'F' as _, b'o' as _, b'l' as _,
    b'd' as _, b'e' as _, b'r' as _, b' ' as _, b'B' as _, b'r' as _, b'o' as _, b'w' as _,
    b's' as _, b'i' as _, b'n' as _, b'g' as _, 0,
];

#[allow(non_snake_case)]
extern "system" {
    // Missing from `winapi`. There doesn't seem to be any open tickets or PRs regarding this
    // function, so we may wish to submit one ourselves when we get the chance.
    pub fn CreateBindCtx(reserved: DWORD, ppbc: *mut LPBC) -> HRESULT;
}

// COM types that we don't use but are part of the `IFileOperation` interface.
pub enum IFileOperationProgressSink {}
pub enum IOperationsProgressDialog {}
pub enum IPropertyChangeArray {}

// Attribute flags that can be retrieved on an item (file or folder) or set of items.
type SFGAOF = ULONG;
const SFGAO_FOLDER: SFGAOF = 0x2000_0000;
const SFGAO_FILESYSTEM: SFGAOF = 0x4000_0000;

#[allow(non_snake_case)]
mod file_operation {
    use super::*;

    RIDL! {
        // `IFileOperation` COM interface (currently missing from the `winapi` crate; see
        // https://github.com/retep998/winapi-rs/pull/834).
        #[uuid(0x947aab5f, 0x0a5c, 0x4c13, 0xb4, 0xd6, 0x4b, 0xf7, 0x83, 0x6f, 0xc9, 0xf8)]
        interface IFileOperation(IFileOperationVtbl): IUnknown(IUnknownVtbl) {
            fn Advise(
                pfops: *mut IFileOperationProgressSink,
                pdwCookie: *mut DWORD,
            ) -> HRESULT,
            fn Unadvise(
                dwCookie: DWORD,
            ) -> HRESULT,
            fn SetOperationFlags(
                dwOperationFlags: DWORD,
            ) -> HRESULT,
            fn SetProgressMessage(
                pszMessage: LPCWSTR,
            ) -> HRESULT,
            fn SetProgressDialog(
                popd: *mut IOperationsProgressDialog,
            ) -> HRESULT,
            fn SetProperties(
                pproparray: *mut IPropertyChangeArray,
            ) -> HRESULT,
            fn SetOwnerWindow(
                hwndOwner: HWND,
            ) -> HRESULT,
            fn ApplyPropertiesToItem(
                psiItem: *mut IShellItem,
            ) -> HRESULT,
            fn ApplyPropertiesToItems(
                punkItems: *mut IUnknown,
            ) -> HRESULT,
            fn RenameItem(
                psiItem: *mut IShellItem,
                pszNewName: LPCWSTR,
                pfopsItem: *mut IFileOperationProgressSink,
            ) -> HRESULT,
            fn RenameItems(
                pUnkItems: *mut IUnknown,
                pszNewName: LPCWSTR,
            ) -> HRESULT,
            fn MoveItem(
                psiItem: *mut IShellItem,
                psiDestinationFolder: *mut IShellItem,
                pszNewName: LPCWSTR,
                pfopsItem: *mut IFileOperationProgressSink,
            ) -> HRESULT,
            fn MoveItems(
                punkItems: *mut IUnknown,
                psiDestinationFolder: *mut IShellItem,
            ) -> HRESULT,
            fn CopyItem(
                psiItem: *mut IShellItem,
                psiDestinationFolder: *mut IShellItem,
                pszCopyName: LPCWSTR,
                pfopsItem: *mut IFileOperationProgressSink,
            ) -> HRESULT,
            fn CopyItems(
                punkItems: *mut IUnknown,
                psiDestinationFolder: *mut IShellItem,
            ) -> HRESULT,
            fn DeleteItem(
                psiItem: *mut IShellItem,
                pfopsItem: *mut IFileOperationProgressSink,
            ) -> HRESULT,
            fn DeleteItems(
                punkItems: *mut IUnknown,
            ) -> HRESULT,
            fn NewItem(
                psiDestinationFolder: *mut IShellItem,
                dw_file_attributes: DWORD,
                pszName: LPCWSTR,
                pszTemplateName: LPCWSTR,
                pfopsItem: *mut IFileOperationProgressSink,
            ) -> HRESULT,
            fn PerformOperations() -> HRESULT,
            fn GetAnyOperationsAborted(
                pfAnyOperationsAborted: *mut BOOL,
            ) -> HRESULT,
        }
    }
}

use file_operation::IFileOperation;

/// Trait for generic `IUnknown::Release` call support in `ComRef`.
trait IUnknownRelease {
    unsafe fn release(&self) -> ULONG;
}

macro_rules! impl_i_unknown_release {
    ($target:ty) => {
        impl IUnknownRelease for $target {
            unsafe fn release(&self) -> ULONG {
                self.Release()
            }
        }
    };
}

impl_i_unknown_release!(IUnknown);
impl_i_unknown_release!(IBindCtx);
impl_i_unknown_release!(IFileOperation);
impl_i_unknown_release!(IShellItem);

/// Container for automatically releasing a `winapi` COM object upon dropping.
struct ComRef<T>(NonNull<T>)
where
    T: IUnknownRelease;

impl<T> ComRef<T>
where
    T: IUnknownRelease,
{
    unsafe fn new(ptr: *mut T) -> Option<Self> {
        NonNull::new(ptr).map(Self)
    }

    fn as_ptr(&self) -> *mut T {
        self.0.as_ptr()
    }
}

impl<T> Drop for ComRef<T>
where
    T: IUnknownRelease,
{
    fn drop(&mut self) {
        unsafe {
            self.0.as_ref().release();
        }
    }
}

impl<T> Deref for ComRef<T>
where
    T: IUnknownRelease,
{
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

/// COM object implementing `IUnknown` and nothing else, primarily for `IBindCtx` boolean parameter
/// use.
#[repr(C)]
struct Unknown {
    /// C++ vtable.
    vtbl: &'static IUnknownVtbl,

    /// Object reference count.
    ref_count: Cell<ULONG>,
}

impl Unknown {
    /// C++ vtable for all `Unknown` objects.
    const VTBL: IUnknownVtbl = IUnknownVtbl {
        QueryInterface: Unknown::query_interface,
        AddRef: Unknown::add_ref,
        Release: Unknown::release,
    };

    /// Allocates an `Unknown` COM object with an initial reference count of 1.
    pub fn allocate() -> ComRef<IUnknown> {
        unsafe {
            ComRef::new(Box::into_raw(Box::new(Unknown {
                vtbl: &Self::VTBL,
                ref_count: Cell::new(1),
            })) as *mut _)
            .unwrap()
        }
    }

    /// `IUnknown::QueryInterface` call handler.
    unsafe extern "system" fn query_interface(
        this: *mut IUnknown,
        riid: REFIID,
        ppv_object: *mut *mut c_void,
    ) -> HRESULT {
        if riid == &IUnknown::uuidof() {
            *ppv_object = this as *mut _;
            S_OK
        } else {
            E_NOINTERFACE
        }
    }

    /// `IUnknown::AddRef` call handler.
    unsafe extern "system" fn add_ref(this: *mut IUnknown) -> ULONG {
        let this = this as *mut Unknown;
        let new_count = (*this).ref_count.get() + 1;
        (*this).ref_count.set(new_count);

        new_count
    }

    /// `IUnknown::Release` call handler.
    unsafe extern "system" fn release(this: *mut IUnknown) -> ULONG {
        let this = this as *mut Unknown;
        let new_count = (*this).ref_count.get() - 1;
        if new_count == 0 {
            // Since we don't hold onto the `Box` returned, it is implicitly dropped.
            Box::from_raw(this);
        } else {
            (*this).ref_count.set(new_count);
        }

        new_count
    }
}

/// Helper trait for providing the unsigned equivalent of a signed type.
///
/// This is primarily used to guarantee at compile-time that we are using the same-sized type when
/// converting an `HRESULT` into an unsigned number for bitwise operations in `hresult_to_result`.
trait Signed {
    type Unsigned;
}

impl Signed for i32 {
    type Unsigned = u32;
}

/// Checks whether an `HRESULT` is an error, returning an `io:Error` if so. If the `HRESULT`
/// contains a Win32 error code, `io::Error::from_raw_os_error` will be called with that code to
/// generate a more standard error message.
fn hresult_to_result(result: HRESULT, context: &str) -> io::Result<HRESULT> {
    if result >= 0 {
        Ok(result)
    } else {
        Err(
            if result as <HRESULT as Signed>::Unsigned & 0xffff_0000 == 0x8007_0000 {
                io::Error::from_raw_os_error(result & 0xffff)
            } else {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("{} failed with HRESULT {}.", context, result),
                )
            },
        )
    }
}

/// `SHCreateItemFromParsingName` function pointer.
///
/// Presence of this function is used to determine whether the Windows Property System is present,
/// and is initialized only on the first call to `remove_dir_all` on any thread. If the function
/// cannot be resolved, this will be left as `None`, and support will not be checked again.
static mut SH_CREATE_ITEM_FROM_PARSING_NAME_OPT: Option<SHCreateItemFromParsingNameFn> = None;

/// Single-initialization for `SH_CREATE_ITEM_FROM_PARSING_NAME`.
static SH_CREATE_ITEM_FROM_PARSING_NAME_INIT: Once = Once::new();

/// Deletes a directory and all of its contents using `IFileOperation` if supported.
///
/// Returns one of the following:
/// - `Ok(Some(()))`: Directory deletion succeeded.
/// - `Err(error)`: Directory deletion failed.
/// - `Ok(None)`: `IFileOperation` is not supported.
pub fn remove_dir_all(path: &Path) -> io::Result<Option<()>> {
    SH_CREATE_ITEM_FROM_PARSING_NAME_INIT.call_once(|| unsafe {
        // Attempt to dynamically load `SHCreateItemFromParsingName` from `shell32.dll` (which
        // should be linked with the program and already loaded, otherwise `SHFileOperationW` would
        // fail to link) to determine whether support is present.
        let h_shell32 =
            GetModuleHandleW("shell32.dll\0".encode_utf16().collect::<Vec<_>>().as_ptr());
        if !h_shell32.is_null() {
            SH_CREATE_ITEM_FROM_PARSING_NAME_OPT = mem::transmute(GetProcAddress(
                h_shell32,
                "SHCreateItemFromParsingName\0".as_ptr() as *const c_char,
            ));
        }
    });

    let sh_create_item_from_parsing_name = match unsafe { SH_CREATE_ITEM_FROM_PARSING_NAME_OPT } {
        Some(func) => func,
        None => return Ok(None),
    };

    let path = resolve_absolute_path_utf16(path)?;

    // `IFileOperation` only supports use in an apartment-threaded COM thread, so spawn a separate
    // thread for the operation to avoid any potential conflicts with the application's COM
    // apartment configuration.
    let thread_work = move || -> io::Result<_> {
        unsafe {
            // Create an `IBindCtx` to restrict searches to filesystem paths.
            let mut p_bind_ctx = ptr::null_mut::<IBindCtx>();
            let result = CreateBindCtx(0, &mut p_bind_ctx);
            let bind_ctx_opt = ComRef::new(p_bind_ctx);
            let bind_ctx = hresult_to_result(result, "`CreateBindCtx`").and_then(|_| {
                bind_ctx_opt.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        "`CreateBindCtx` succeeded but did not create an `IBindCtx`.",
                    )
                })
            })?;

            // The "Parsing With Parameters" sample uses `const_cast` to cast away `const`-ness on
            // object parameter strings, so I can only assume it is safe for us to do the same.
            let true_obj = Unknown::allocate();
            hresult_to_result(
                bind_ctx.RegisterObjectParam(
                    STR_PARSE_PREFER_FOLDER_BROWSING.as_ptr() as *mut _,
                    true_obj.as_ptr(),
                ),
                "`IBindCtx::RegisterObjectParam`",
            )?;

            let mut p_item = ptr::null_mut::<IShellItem>();
            let result = (sh_create_item_from_parsing_name)(
                strip_extended_length_path_prefix(&path).as_ptr(),
                bind_ctx.as_ptr(),
                &IShellItem::uuidof(),
                &mut p_item as *mut _ as *mut _,
            );
            let item_opt = ComRef::new(p_item);
            let item =
                hresult_to_result(result, "`SHCreateItemFromParsingName`").and_then(|_| {
                    item_opt.ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::Other,
                            concat!(
                                "`SHCreateItemFromParsingName` succeeded but did not create an ",
                                "`IShellItem`."
                            ),
                        )
                    })
                })?;

            // Double-check the target is a valid filesystem directory. Since Windows distinguishes
            // between file and folder symbolic links, `SFGAO_FOLDER` should be set regardless of
            // whether the target is a symbolic link. `GetAttributes` will return `S_OK` if and only
            // if the attributes match exactly.
            let mut attributes = 0;
            if item.GetAttributes(SFGAO_FOLDER | SFGAO_FILESYSTEM, &mut attributes) != S_OK {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Target is not a directory or directory symlink.",
                ));
            }

            let mut p_file_op = ptr::null_mut::<IFileOperation>();
            let result = CoCreateInstance(
                &FileOperation::uuidof(),
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IFileOperation::uuidof(),
                &mut p_file_op as *mut _ as *mut _,
            );
            let file_op_opt = ComRef::new(p_file_op);
            let file_op =
                hresult_to_result(result, "`IFileOperation` creation").and_then(|_| {
                    file_op_opt.ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::Other,
                            "`CoCreateInstance` succeded but did not create an `IFileOperation`.",
                        )
                    })
                })?;

            hresult_to_result(
                file_op.SetOperationFlags(FOF_NO_UI.into()),
                "`IFileOperation::SetOperationFlags()`",
            )?;
            hresult_to_result(
                file_op.DeleteItem(item.as_ptr(), ptr::null_mut()),
                "`IFileOperation::DeleteItem()`",
            )?;

            hresult_to_result(
                file_op.PerformOperations(),
                "`IFileOperation::PerformOperations`",
            )?;

            let mut aborted = FALSE;
            hresult_to_result(
                file_op.GetAnyOperationsAborted(&mut aborted),
                "`IFileOperation::GetAnyOperationsAborted`",
            )?;
            if aborted != FALSE {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Operation aborted before completion.",
                ));
            }

            Ok(())
        }
    };

    let handle = thread::spawn(move || unsafe {
        hresult_to_result(
            CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED),
            "`CoInitializeEx`",
        )?;
        let result = thread_work();
        CoUninitialize();

        result
    });

    // Propagate panics within the worker thread by unwrapping.
    handle.join().unwrap().map(Some)
}
