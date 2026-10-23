use jni::sys::{jint, jsize, JavaVM};

pub type GetCreatedJavaVms = unsafe extern "system" fn(*mut *mut JavaVM, jsize, *mut jsize) -> jint;

#[cfg(windows)]
mod win32;
#[cfg(windows)]
pub use self::win32::{find_get_created_java_vms, show_message};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use self::unix::{find_get_created_java_vms, show_message};
