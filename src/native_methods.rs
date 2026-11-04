use core::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};

use jni::objects::{JByteArray, JClass, JObject, JValue};
use jni::sys::{jbyteArray, jclass, jobject};
use jni::{JNIEnv, NativeMethod};

use crate::error::{InjectorError, Result};

const DEFINE_CLASS0_SIGNATURE: &str =
    "(Ljava/lang/ClassLoader;[BLjava/security/ProtectionDomain;)Ljava/lang/Class;";
const DEFINE_CLASS_SIGNATURE: &str =
    "(Ljava/lang/String;[BIILjava/security/ProtectionDomain;)Ljava/lang/Class;";

fn define_class(
    env: &mut JNIEnv,
    loader: &JObject,
    data: &JByteArray,
    protection_domain: &JObject,
) -> jni::errors::Result<jobject> {
    let length = env.get_array_length(data)?;
    let class = env.call_method(
        loader,
        "defineClass",
        DEFINE_CLASS_SIGNATURE,
        &[
            JValue::Object(&JObject::null()),
            JValue::from(data),
            JValue::Int(0),
            JValue::Int(length),
            JValue::from(protection_domain),
        ],
    )?;
    Ok(class.l()?.into_raw())
}

unsafe extern "system" fn define_class0(
    raw_env: *mut jni::sys::JNIEnv,
    _injector_class: jclass,
    loader: jobject,
    data: jbyteArray,
    protection_domain: jobject,
) -> jobject {
    let Ok(mut env) = JNIEnv::from_raw(raw_env) else {
        return core::ptr::null_mut();
    };

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let loader = JObject::from_raw(loader);
        let data = JByteArray::from_raw(data);
        let protection_domain = JObject::from_raw(protection_domain);
        define_class(&mut env, &loader, &data, &protection_domain)
    }));

    match outcome {
        Ok(Ok(class)) => class,
        Ok(Err(_)) | Err(_) => {
            if !env.exception_check().unwrap_or(false) {
                let _ = env.throw_new("java/lang/InternalError", "native defineClass0 failed");
            }
            core::ptr::null_mut()
        }
    }
}

pub fn register(env: &mut JNIEnv, injector_class: &JClass) -> Result<()> {
    let methods = [NativeMethod {
        name: "defineClass0".into(),
        sig: DEFINE_CLASS0_SIGNATURE.into(),
        fn_ptr: define_class0 as *mut c_void,
    }];
    env.register_native_methods(injector_class, &methods)
        .map_err(|_| {
            let _ = env.exception_clear();
            InjectorError::new("Failed to register native methods")
        })
}
