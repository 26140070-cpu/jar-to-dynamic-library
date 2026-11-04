use core::ffi::c_void;
use core::mem;
use core::ptr;

use windows_sys::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::CreateThread;

use super::GetCreatedJavaVms;
use crate::error::{InjectorError, Result};
use crate::injector;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn show_message(message: &str) {
    eprintln!("[JarToDynamicLibrary] {message}");
}

fn show_error(error: &InjectorError) {
    eprintln!("[JarToDynamicLibrary] {}", error.message);
    eprintln!("[JarToDynamicLibrary] Last error code: {}", error.os_error);
}

pub fn find_get_created_java_vms() -> Result<GetCreatedJavaVms> {
    let jvm_dll = unsafe { GetModuleHandleW(wide("jvm.dll").as_ptr()) };
    if jvm_dll.is_null() {
        return Err(InjectorError::new("Can't get jvm.dll handle"));
    }

    let proc = unsafe { GetProcAddress(jvm_dll, b"JNI_GetCreatedJavaVMs\0".as_ptr()) };
    let Some(proc) = proc else {
        return Err(InjectorError::new("Can't get JNI_GetCreatedJavaVMs proc"));
    };
    Ok(unsafe { mem::transmute::<unsafe extern "system" fn() -> isize, GetCreatedJavaVms>(proc) })
}

unsafe extern "system" fn injector_thread(_param: *mut c_void) -> u32 {
    match std::panic::catch_unwind(injector::run) {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            show_error(&error);
            1
        }
        Err(_) => {
            eprintln!("[JarToDynamicLibrary] injector thread panicked");
            1
        }
    }
}

#[no_mangle]
pub extern "system" fn DllMain(_dll_instance: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            CreateThread(
                ptr::null(),
                0,
                Some(injector_thread),
                ptr::null(),
                0,
                ptr::null_mut(),
            );
        }
    }
    TRUE
}
