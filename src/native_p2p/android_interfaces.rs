//! Process-level JNI references for optional, on-demand LAN candidate lookup.
use super::*;
use jni::{objects::{GlobalRef, JObject, JString, JValue}, JavaVM, JNIEnv};

struct Bridge {
    vm: JavaVM,
    ffi: GlobalRef,
    context: GlobalRef,
}

hbb_common::lazy_static::lazy_static! {
    static ref BRIDGE: Mutex<Option<Bridge>> = Mutex::new(None);
}

#[no_mangle]
pub extern "system" fn Java_ffi_FFI_initNativeP2p(env: JNIEnv, ffi: JObject, context: JObject) {
    let result = (|| -> Result<Bridge> {
        hbb_common::anyhow::ensure!(!ffi.is_null() && !context.is_null(), "missing Android application context");
        Ok(Bridge { vm: env.get_java_vm()?, ffi: env.new_global_ref(ffi)?, context: env.new_global_ref(context)? })
    })();
    match result {
        Ok(bridge) => { *BRIDGE.lock().unwrap() = Some(bridge); }
        Err(error) => log::debug!("Android native P2P LAN adapter unavailable: {error}"),
    }
}

pub(super) fn read() -> Result<String> {
    let bridge = BRIDGE.lock().unwrap();
    let bridge = bridge.as_ref().context("Android LAN adapter not initialized")?;
    let mut env = bridge.vm.attach_current_thread()?;
    let result = env.with_local_frame(8, |env| -> Result<String> {
        let value = env.call_method(&bridge.ffi, "nativeP2pInterfaces",
            "(Landroid/content/Context;)Ljava/lang/String;",
            &[JValue::Object(bridge.context.as_obj())])?.l()?;
        let value = JString::from(value);
        let value: String = env.get_string(&value)?.into();
        Ok(value)
    });
    // Leave the shared Rust worker thread usable if a JNI lookup failed.
    if env.exception_check()? { env.exception_clear()?; }
    result
}
