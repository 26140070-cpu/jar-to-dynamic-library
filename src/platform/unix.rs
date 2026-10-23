use core::ffi::c_char;
use core::mem;

use super::GetCreatedJavaVms;
use crate::error::{InjectorError, Result};
use crate::injector;

pub fn show_message(message: &str) {
    eprintln!("[JarToDynamicLibrary] {message}");
}

pub fn find_get_created_java_vms() -> Result<GetCreatedJavaVms> {
    let symbol = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            b"JNI_GetCreatedJavaVMs\0".as_ptr() as *const c_char,
        )
    };
    if symbol.is_null() {
        return Err(InjectorError::new(
            "Can't find JNI_GetCreatedJavaVMs: is libjvm loaded in this process?",
        ));
    }
    Ok(unsafe { mem::transmute::<*mut libc::c_void, GetCreatedJavaVms>(symbol) })
}

fn injector_thread() {
    match std::panic::catch_unwind(injector::run) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            eprintln!("[JarToDynamicLibrary] {}", error.message);
            eprintln!("[JarToDynamicLibrary] Last error code: {}", error.os_error);
        }
        Err(_) => eprintln!("[JarToDynamicLibrary] injector thread panicked"),
    }
}

extern "C" fn on_load() {
    let _ = std::thread::Builder::new()
        .name("jar-to-dynamic-library".into())
        .spawn(injector_thread);
}

#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__mod_init_func")]
#[cfg_attr(not(target_vendor = "apple"), link_section = ".init_array")]
static ON_LOAD: extern "C" fn() = on_load;
