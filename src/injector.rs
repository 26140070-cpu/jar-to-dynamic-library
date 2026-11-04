use core::ptr;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use jni::objects::{JClass, JObject, JObjectArray};
use jni::sys::{self, jsize, JNI_VERSION_1_8};
use jni::JNIEnv;

use crate::classes::{
    BAKED_JAR_PATH, INJECTOR_CLASS_DATA, INJECTOR_CLASS_NAME, JAR_CLASSES, JAR_INJECTOR_CLASSES,
    JAR_INJECTOR_CLASS_DATA, JAR_INJECTOR_CLASS_NAME,
};
use crate::error::{InjectorError, Result};
use crate::native_methods;
use crate::platform::{find_get_created_java_vms, show_message};

fn jar_option(options: &str) -> Option<String> {
    if !options.contains('=') {
        let trimmed = options.trim();
        return (!trimmed.is_empty()).then(|| trimmed.to_string());
    }
    options
        .split(';')
        .find_map(|part| part.trim().strip_prefix("jar=").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn target_jar(options: Option<&str>) -> Option<String> {
    if let Some(path) = options.and_then(jar_option) {
        return Some(path);
    }
    (!BAKED_JAR_PATH.is_empty() && std::path::Path::new(BAKED_JAR_PATH).is_file())
        .then(|| BAKED_JAR_PATH.to_string())
}

fn get_jvm() -> Result<*mut sys::JavaVM> {
    let get_created_java_vms = find_get_created_java_vms()?;

    let mut jvms: [*mut sys::JavaVM; 1] = [ptr::null_mut()];
    let mut n_vms: jsize = 1;
    let status = unsafe { get_created_java_vms(jvms.as_mut_ptr(), n_vms, &mut n_vms) };
    if status != 0 {
        return Err(InjectorError::new("JNI_GetCreatedJavaVMs failed"));
    }
    if n_vms == 0 || jvms[0].is_null() {
        return Err(InjectorError::new("Can't get JVM"));
    }
    Ok(jvms[0])
}

unsafe fn get_jni_env<'a>(jvm: *mut sys::JavaVM) -> Result<(JNIEnv<'a>, bool)> {
    const JNI_OK: i32 = 0;
    const JNI_EDETACHED: i32 = -2;

    let invoke = &**jvm;
    let get_env = invoke
        .GetEnv
        .ok_or_else(|| InjectorError::new("JVM does not provide GetEnv"))?;
    let mut env: *mut core::ffi::c_void = ptr::null_mut();
    let status = get_env(jvm, &mut env, JNI_VERSION_1_8);

    let attached_here = if status == JNI_OK {
        false
    } else if status == JNI_EDETACHED {
        let attach = invoke
            .AttachCurrentThread
            .ok_or_else(|| InjectorError::new("JVM does not provide AttachCurrentThread"))?;
        let attach_status = attach(jvm, &mut env, ptr::null_mut());
        if attach_status != JNI_OK {
            return Err(InjectorError::new("Could not attach injector thread to the JVM"));
        }
        true
    } else {
        return Err(InjectorError::new("GetEnv failed for JNI_VERSION_1_8"));
    };

    if env.is_null() {
        if attached_here {
            if let Some(detach) = invoke.DetachCurrentThread {
                detach(jvm);
            }
        }
        return Err(InjectorError::new("JVM returned a null JNIEnv"));
    }
    let env = match JNIEnv::from_raw(env as *mut sys::JNIEnv) {
        Ok(env) => env,
        Err(_) => {
            if attached_here {
                if let Some(detach) = invoke.DetachCurrentThread {
                    detach(jvm);
                }
            }
            return Err(InjectorError::new("Could not create JNIEnv wrapper"));
        }
    };
    Ok((env, attached_here))
}

fn define_or_get<'a>(env: &mut JNIEnv<'a>, name: &str, data: &[u8]) -> Result<(JClass<'a>, bool)> {
    match env.find_class(name) {
        Ok(existing) => {
            show_message("Injector class is already presented in jvm, using it");
            return Ok((existing, true));
        }
        Err(_) => {
            let _ = env.exception_clear();
        }
    }

    let raw = env.get_raw();
    let define_class = unsafe { (**raw).DefineClass }
        .ok_or_else(|| InjectorError::new("DefineClass missing from JNI table"))?;
    let class = unsafe {
        define_class(
            raw,
            ptr::null(),
            ptr::null_mut(),
            data.as_ptr() as *const sys::jbyte,
            data.len() as jsize,
        )
    };
    if class.is_null() {
        let _ = env.exception_clear();
        return Err(InjectorError::new("Failed to define the injector class"));
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
    env.delete_local_ref(byte_array_class)
        .map_err(|_| InjectorError::new("Failed to release byte array class reference"))?;

    for (i, class_data) in JAR_CLASSES.iter().enumerate() {
        let class_byte_array = env
            .byte_array_from_slice(class_data)
            .map_err(|_| InjectorError::new("Failed to create class byte array"))?;
        env.set_object_array_element(&array, i as jsize, &class_byte_array)
            .map_err(|_| InjectorError::new("Failed to populate class byte array"))?;
        env.delete_local_ref(class_byte_array)
            .map_err(|_| InjectorError::new("Failed to release class byte array reference"))?;
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

    let call_result = unsafe {
        env.call_static_method_unchecked(
            injector_class,
            inject,
            jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
            &[jni::sys::jvalue {
                l: jar_classes.as_raw(),
            }],
        )
    };
    if call_result.is_err() || env.exception_check().unwrap_or(false) {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        return Err(InjectorError::new("Java injector entrypoint threw an exception"));
    }
    show_message("Native injector entered; Java-side injection runs asynchronously");
    Ok(())
}

fn define_injector_support_classes(env: &mut JNIEnv) -> Result<()> {
    for (name, data) in JAR_INJECTOR_CLASSES {
        if *name == JAR_INJECTOR_CLASS_NAME {
            continue;
        }
        let _ = define_or_get(env, name, data)?;
    }
    Ok(())
}

fn call_jar_injector(env: &mut JNIEnv, jar: &str, options: Option<&str>) -> Result<()> {
    define_injector_support_classes(env)?;
    let (class, _) = define_or_get(env, JAR_INJECTOR_CLASS_NAME, JAR_INJECTOR_CLASS_DATA)?;
    let arguments = options.unwrap_or(jar);
    let path = env
        .new_string(arguments)
        .map_err(|_| InjectorError::new("Could not build the JAR path/options string"))?;

    let call = env.call_static_method(
        &class,
        "inject",
        "(Ljava/lang/String;)V",
        &[jni::objects::JValue::from(&path)],
    );
    let threw = env.exception_check().unwrap_or(false);
    if call.is_err() || threw {
        if threw {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        return Err(InjectorError::new(
            "JarInjector.inject failed; see ~/jar-to-dynamic-library-log.txt",
        ));
    }
    show_message(&format!(
        "JarInjector received {jar}; injection runs on its own thread"
    ));
    Ok(())
}

unsafe fn detach_current_thread(jvm: *mut sys::JavaVM) {
    if let Some(detach) = (**jvm).DetachCurrentThread {
        detach(jvm);
    }
}

static INJECTED_SESSIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn claim_session(session: &str) -> bool {
    let sessions = INJECTED_SESSIONS.get_or_init(|| Mutex::new(HashSet::new()));
    sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(session.to_string())
}

fn release_session(session: &str) {
    if let Some(sessions) = INJECTED_SESSIONS.get() {
        sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(session);
    }
}

fn session_id(options: Option<&str>) -> String {
    options
        .and_then(|value| {
            value.split(';').find_map(|part| {
                part.trim()
                    .strip_prefix("session=")
                    .filter(|session| !session.is_empty())
            })
        })
        .map(str::to_string)
        .or_else(|| options.and_then(jar_option))
        .unwrap_or_else(|| "legacy-attach".to_string())
}

pub fn run() -> Result<()> {
    let jvm = get_jvm()?;
    run_with_jvm(jvm, None)
}

pub fn run_with_jvm(jvm: *mut sys::JavaVM, options: Option<&str>) -> Result<()> {
    if jvm.is_null() {
        return Err(InjectorError::new("Agent entrypoint received a null JavaVM"));
    }

    let (mut env, attached_here) = unsafe { get_jni_env(jvm)? };
    let session = session_id(options);
    if !claim_session(&session) {
        if attached_here {
            unsafe { detach_current_thread(jvm) };
        }
        show_message("This attach session was already handled; skipping duplicate entrypoint");
        return Ok(());
    }
    show_message("Starting");

    let jar = target_jar(options);

    let result = (|| match jar {
        Some(jar) => {
            show_message(&format!("JAR-driven injector: {jar}"));
            call_jar_injector(&mut env, &jar, options)
        }
        None => {
            show_message("No JAR supplied; falling back to the legacy embedded-class injector");
            let (injector_class, already_existed) =
                define_or_get(&mut env, INJECTOR_CLASS_NAME, INJECTOR_CLASS_DATA)?;
            register_natives(&mut env, &injector_class, already_existed)?;
            let jar_classes = get_jar_classes_array(&mut env)?;
            call_injector(&mut env, &injector_class, &jar_classes)
        }
    })();

    if attached_here {
        unsafe { detach_current_thread(jvm) };
    }
    if result.is_err() {
        release_session(&session);
    }
    result
}
