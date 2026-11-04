use core::ffi::{c_char, c_void};
use std::ffi::CStr;

use jni::sys::{jint, JavaVM};

use crate::injector;
use crate::platform::show_message;

const JNI_OK: jint = 0;
const JNI_ERR: jint = -1;

fn option_string(options: *mut c_char) -> Option<String> {
    if options.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(options) }.to_string_lossy().into_owned();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn run_from_agent(entry: &str, vm: *mut JavaVM, options: Option<String>) -> jint {
    if vm.is_null() {
        eprintln!("[JarToDynamicLibrary] {entry} was invoked with a null JavaVM pointer");
        return JNI_ERR;
    }
    match &options {
        Some(options) => show_message(&format!("{entry}: options = {options}")),
        None => show_message(&format!("{entry}: no options supplied")),
    }

    let jvm_addr = vm as usize;
    let spawned = std::thread::Builder::new()
        .name("jar-to-dynamic-library-agent".into())
        .spawn(move || {
            let jvm = jvm_addr as *mut JavaVM;
            match std::panic::catch_unwind(move || injector::run_with_jvm(jvm, options.as_deref())) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    eprintln!("[JarToDynamicLibrary] {}", error.message);
                    eprintln!("[JarToDynamicLibrary] Last error code: {}", error.os_error);
                }
                Err(_) => eprintln!("[JarToDynamicLibrary] injector thread panicked"),
            }
        });
    match spawned {
        Ok(_) => JNI_OK,
        Err(_) => {
            eprintln!("[JarToDynamicLibrary] could not spawn the injector thread");
            JNI_ERR
        }
    }
}

#[no_mangle]
pub extern "system" fn Agent_OnAttach(vm: *mut JavaVM, options: *mut c_char, _reserved: *mut c_void) -> jint {
    run_from_agent(
        "Agent_OnAttach (dynamic attach to running JVM)",
        vm,
        option_string(options),
    )
}

#[no_mangle]
pub extern "system" fn Agent_OnLoad(vm: *mut JavaVM, options: *mut c_char, _reserved: *mut c_void) -> jint {
    run_from_agent(
        "Agent_OnLoad (-agentpath at JVM startup)",
        vm,
        option_string(options),
    )
}
