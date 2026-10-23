use core::ptr;

use jni::objects::{JClass, JObject, JObjectArray};
use jni::sys::{self, jsize, JNI_VERSION_1_8};
use jni::JNIEnv;

use crate::classes::{INJECTOR_CLASS_DATA, INJECTOR_CLASS_NAME, JAR_CLASSES};
use crate::error::{InjectorError, Result};
use crate::native_methods;
use crate::platform::{find_get_created_java_vms, show_message};

fn get_jvm() -> Result<*mut sys::JavaVM> {
    let get_created_java_vms = find_get_created_java_vms()?;

    let mut jvms: [*mut sys::JavaVM; 1] = [ptr::null_mut()];
    let mut n_vms: jsize = 1;
    unsafe { get_created_java_vms(jvms.as_mut_ptr(), n_vms, &mut n_vms) };

    if n_vms == 0 || jvms[0].is_null() {
        return Err(InjectorError::new("Can't get JVM"));
    }
    Ok(jvms[0])
}

unsafe fn get_jni_env(jvm: *mut sys::JavaVM) -> Result<JNIEnv<'static>> {
    let mut env: *mut core::ffi::c_void = ptr::null_mut();
    let invoke = &**jvm;

    if let Some(attach) = invoke.AttachCurrentThread {
        attach(jvm, &mut env, ptr::null_mut());
    }
    if let Some(get_env) = invoke.GetEnv {
        get_env(jvm, &mut env, JNI_VERSION_1_8);
    }

    if env.is_null() {
        return Err(InjectorError::new("Can't get JNIEnv"));
    }
    JNIEnv::from_raw(env as *mut sys::JNIEnv).map_err(|_| InjectorError::new("Can't get JNIEnv"))
}

fn define_or_get_injector<'a>(env: &mut JNIEnv<'a>) -> Result<(JClass<'a>, bool)> {
    match env.find_class(INJECTOR_CLASS_NAME) {
        Ok(existing) => {
            show_message("Injector class is already presented in jvm, using it");
            return Ok((existing, true));
        }
        Err(_) => {
            let _ = env.exception_clear();
        }
    }

    let raw = env.get_raw();
    let class = unsafe {
        let define_class = (**raw).DefineClass.expect("DefineClass missing from JNI table");
        define_class(
            raw,
            ptr::null(),
            ptr::null_mut(),
            INJECTOR_CLASS_DATA.as_ptr() as *const sys::jbyte,
            INJECTOR_CLASS_DATA.len() as jsize,
        )
    };
    if class.is_null() {
        let _ = env.exception_clear();
        return Err(InjectorError::new("Failed to define injector class"));
    }
    Ok((unsafe { JClass::from_raw(class) }, false))
}

fn register_natives(env: &mut JNIEnv, injector_class: &JClass, already_existed: bool) -> Result<()> {
    match native_methods::register(env, injector_class) {
        Err(_) if already_existed => Ok(()),
        result => result,
    }
}

fn get_jar_classes_array<'a>(env: &mut JNIEnv<'a>) -> Result<JObjectArray<'a>> {
    let byte_array_class = env
        .find_class("[B")
        .map_err(|_| InjectorError::new("Failed to get byte array class"))?;

    let array = env
        .new_object_array(JAR_CLASSES.len() as jsize, &byte_array_class, JObject::null())
        .map_err(|_| InjectorError::new("Failed to create jar classes array"))?;

    for (i, class_data) in JAR_CLASSES.iter().enumerate() {
        let class_byte_array = env
            .byte_array_from_slice(class_data)
            .map_err(|_| InjectorError::new("Failed to create class byte array"))?;
        env.set_object_array_element(&array, i as jsize, class_byte_array)
            .map_err(|_| InjectorError::new("Failed to create class byte array"))?;
    }
    Ok(array)
}

fn call_injector(env: &mut JNIEnv, injector_class: &JClass, jar_classes: &JObjectArray) -> Result<()> {
    let inject = env
        .get_static_method_id(injector_class, "inject", "([[B)V")
        .map_err(|_| {
            let _ = env.exception_clear();
            InjectorError::new("Failed to find inject method ID")
        })?;

    unsafe {
        let _ = env.call_static_method_unchecked(
            injector_class,
            inject,
            jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
            &[jni::sys::jvalue {
                l: jar_classes.as_raw(),
            }],
        );
    }
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
    }
    show_message("Native part ready, now java part is injecting");
    Ok(())
}

unsafe fn detach_current_thread(jvm: *mut sys::JavaVM) {
    if let Some(detach) = (**jvm).DetachCurrentThread {
        detach(jvm);
    }
}

pub fn run() -> Result<()> {
    show_message("Starting");

    let jvm = get_jvm()?;
    let mut env = unsafe { get_jni_env(jvm)? };

    let result = (|| {
        let (injector_class, already_existed) = define_or_get_injector(&mut env)?;
        register_natives(&mut env, &injector_class, already_existed)?;
        let jar_classes = get_jar_classes_array(&mut env)?;
        call_injector(&mut env, &injector_class, &jar_classes)
    })();

    unsafe { detach_current_thread(jvm) };
    result
}
