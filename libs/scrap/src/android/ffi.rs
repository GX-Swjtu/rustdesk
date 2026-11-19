use jni::objects::JByteBuffer;
use jni::objects::JString;
use jni::objects::JValue;
use jni::sys::jboolean;
use jni::JNIEnv;
use jni::{
    objects::{GlobalRef, JClass, JObject},
    strings::JNIString,
    JavaVM,
};

use base::message_proto::MultiClipboards;
use hbb_common::protobuf::Message;
use jni::errors::{Error as JniError, Result as JniResult};
use lazy_static::lazy_static;
use serde::Deserialize;
use std::ops::Not;
use std::os::raw::c_void;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

lazy_static! {
    static ref JVM: RwLock<Option<JavaVM>> = RwLock::new(None);
    static ref APPLICATION_CONTEXT: RwLock<Option<GlobalRef>> = RwLock::new(None);
    static ref AUDIO_RAW: Mutex<FrameRaw> = Mutex::new(FrameRaw::new("audio", MAX_AUDIO_FRAME_TIMEOUT));
    static ref NDK_CONTEXT_INITED: Mutex<bool> = Default::default();
    static ref MEDIA_CODEC_INFOS: RwLock<Option<MediaCodecInfos>> = RwLock::new(None);
    static ref CLIPBOARD_MANAGER: RwLock<Option<GlobalRef>> = RwLock::new(None);
    static ref CLIPBOARDS_CLIENT: Mutex<Option<MultiClipboards>> = Mutex::new(None);
}

const MAX_AUDIO_FRAME_TIMEOUT: Duration = Duration::from_millis(1000);

struct FrameRaw {
    name: &'static str,
    data: Vec<u8>,
    ready: bool,
    last_update: Instant,
    timeout: Duration,
    enable: bool,
}

impl FrameRaw {
    fn new(name: &'static str, timeout: Duration) -> Self {
        FrameRaw {
            name,
            data: Vec::new(),
            ready: false,
            last_update: Instant::now(),
            timeout,
            enable: false,
        }
    }

    fn set_enable(&mut self, value: bool) {
        self.enable = value;
        self.ready = false;
        self.data = Vec::new();
    }

    fn update(&mut self, data: *mut u8, len: usize) {
        if self.enable.not() || data.is_null() || len == 0 {
            return;
        }
        let src = unsafe { std::slice::from_raw_parts(data, len) };
        self.data.clear();
        self.data.extend_from_slice(src);
        self.ready = true;
        self.last_update = Instant::now();
    }

    fn take<'a>(&mut self, dst: &mut Vec<u8>, last: &mut Vec<u8>) -> Option<()> {
        if self.enable.not() || !self.ready {
            return None;
        }
        if self.last_update.elapsed() > self.timeout {
            log::trace!("Failed to take {} raw,timeout!", self.name);
            return None;
        }
        self.ready = false;
        // Never true: `last` starts empty and only `would_block_if_equal` fills it. Left off on
        // purpose: Android sends a frame only when the screen recomposes, so repeats are rare.
        if last.len() == self.data.len() && crate::would_block_if_equal(last, &self.data).is_err()
        {
            return None;
        }
        std::mem::swap(dst, &mut self.data);
        Some(())
    }
}

pub fn get_video_raw(_dst: &mut Vec<u8>, _last: &mut Vec<u8>) -> Option<()> {
    None
}

pub fn get_audio_raw<'a>(dst: &mut Vec<u8>, last: &mut Vec<u8>) -> Option<()> {
    AUDIO_RAW.lock().ok()?.take(dst, last)
}

pub fn get_clipboards(client: bool) -> Option<MultiClipboards> {
    if client {
        CLIPBOARDS_CLIENT.lock().ok()?.take()
    } else {
        None
    }
}

#[no_mangle]
pub extern "system" fn Java_ffi_FFI_onAudioFrameUpdate(
    env: JNIEnv,
    _class: JClass,
    buffer: JObject,
) {
    let jb = JByteBuffer::from(buffer);
    if let Ok(data) = env.get_direct_buffer_address(&jb) {
        if let Ok(len) = env.get_direct_buffer_capacity(&jb) {
            AUDIO_RAW.lock().unwrap().update(data, len);
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_ffi_FFI_onClipboardUpdate(
    env: JNIEnv,
    _class: JClass,
    buffer: JByteBuffer,
) {
    if let Ok(data) = env.get_direct_buffer_address(&buffer) {
        if let Ok(len) = env.get_direct_buffer_capacity(&buffer) {
            let data = unsafe { std::slice::from_raw_parts(data, len) };
            if data.first() == Some(&1) {
                if let Ok(clips) = MultiClipboards::parse_from_bytes(&data[1..]) {
                    *CLIPBOARDS_CLIENT.lock().unwrap() = Some(clips);
                }
            }
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_ffi_FFI_setFrameRawEnable(
    env: JNIEnv,
    _class: JClass,
    name: JString,
    value: jboolean,
) {
    let mut env = env;
    if let Ok(name) = env.get_string(&name) {
        let name: String = name.into();
        let value = value.eq(&1);
        if name.eq("audio") {
            AUDIO_RAW.lock().unwrap().set_enable(value);
        }
    };
}

#[no_mangle]
pub extern "system" fn Java_ffi_FFI_setClipboardManager(
    env: JNIEnv,
    _class: JClass,
    clipboard_manager: JObject,
) {
    log::debug!("ClipboardManager init from java");
    if let Ok(jvm) = env.get_java_vm() {
        let java_vm = jvm.get_java_vm_pointer() as *mut c_void;
        let mut jvm_lock = JVM.write().unwrap();
        if jvm_lock.is_none() {
            *jvm_lock = Some(jvm);
        }
        drop(jvm_lock);
        if let Ok(manager) = env.new_global_ref(clipboard_manager) {
            *CLIPBOARD_MANAGER.write().unwrap() = Some(manager);
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct MediaCodecInfo {
    pub name: String,
    pub is_encoder: bool,
    #[serde(default)]
    pub hw: Option<bool>, // api 29+
    pub mime_type: String,
    pub surface: bool,
    pub nv12: bool,
    #[serde(default)]
    pub low_latency: Option<bool>, // api 30+, decoder
    pub min_bitrate: u32,
    pub max_bitrate: u32,
    pub min_width: usize,
    pub max_width: usize,
    pub min_height: usize,
    pub max_height: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MediaCodecInfos {
    pub version: usize,
    pub w: usize, // aligned
    pub h: usize, // aligned
    pub codecs: Vec<MediaCodecInfo>,
}

pub fn get_codec_info() -> Option<MediaCodecInfos> {
    MEDIA_CODEC_INFOS.read().unwrap().as_ref().cloned()
}

pub fn clear_codec_info() {
    *MEDIA_CODEC_INFOS.write().unwrap() = None;
}

// Shared server code also builds for Android. This APK has no Android host service.
pub fn call_main_service_pointer_input(_: &str, _: i32, _: i32, _: i32) -> JniResult<()> {
    Err(JniError::NullPtr("Android host service"))
}

pub fn call_main_service_key_event(_: &[u8]) -> JniResult<()> {
    Err(JniError::NullPtr("Android host service"))
}

fn _call_clipboard_manager<S, T>(name: S, sig: T, args: &[JValue]) -> JniResult<()>
where
    S: Into<JNIString>,
    T: Into<JNIString> + AsRef<str>,
{
    if let (Some(jvm), Some(cm)) = (
        JVM.read().unwrap().as_ref(),
        CLIPBOARD_MANAGER.read().unwrap().as_ref(),
    ) {
        let mut env = jvm.attach_current_thread()?;
        env.call_method(cm, name, sig, args)?;
        return Ok(());
    } else {
        return Err(JniError::ThrowFailed(-1));
    }
}

pub fn call_clipboard_manager_update_clipboard(data: &[u8]) -> JniResult<()> {
    if let (Some(jvm), Some(cm)) = (
        JVM.read().unwrap().as_ref(),
        CLIPBOARD_MANAGER.read().unwrap().as_ref(),
    ) {
        let mut env = jvm.attach_current_thread()?;
        let data = env.byte_array_from_slice(data)?;

        env.call_method(
            cm,
            "rustUpdateClipboard",
            "([B)V",
            &[JValue::Object(&JObject::from(data))],
        )?;
        return Ok(());
    } else {
        return Err(JniError::ThrowFailed(-1));
    }
}

pub fn call_clipboard_manager_enable_client_clipboard(enable: bool) -> JniResult<()> {
    _call_clipboard_manager(
        "rustEnableClientClipboard",
        "(Z)V",
        &[JValue::Bool(jboolean::from(enable))],
    )
}

pub fn call_main_service_get_by_name(_: &str) -> JniResult<String> {
    Err(JniError::NullPtr("Android host service"))
}

pub fn call_main_service_set_by_name(_: &str, _: Option<&str>, _: Option<&str>) -> JniResult<()> {
    Err(JniError::NullPtr("Android host service"))
}

fn init_ndk_context(java_vm: *mut c_void, context_jobject: *mut c_void) {
    let mut lock = NDK_CONTEXT_INITED.lock().unwrap();
    if *lock {
        unsafe {
            ndk_context::release_android_context();
        }
        *lock = false;
    }
    unsafe {
        ndk_context::initialize_android_context(java_vm, context_jobject);
        #[cfg(feature = "hwcodec")]
        hwcodec::android::ffmpeg_set_java_vm(java_vm);
    }
    *lock = true;
}

fn try_init_rustls_platform_verifier(env: &mut JNIEnv, context_jobject: *mut c_void) {
    use hbb_common::config::ANDROID_RUSTLS_PLATFORM_VERIFIER_INITIALIZED as INITIALIZED;
    use std::sync::atomic::Ordering;
    let initialized = INITIALIZED.load(Ordering::Relaxed);
    if !initialized {
        let ctx_for_rustls = unsafe { JObject::from_raw(context_jobject as jni::sys::jobject) };
        if let Err(e) =
            hbb_common::rustls_platform_verifier::android::init_hosted(env, ctx_for_rustls)
        {
            log::error!("Failed to initialize rustls-platform-verifier: {:?}", e);
        } else {
            INITIALIZED.store(true, Ordering::Relaxed);
            log::info!("rustls-platform-verifier initialized successfully");
        }
    }
}

// https://cjycode.com/flutter_rust_bridge/guides/how-to/ndk-init
#[no_mangle]
pub extern "C" fn JNI_OnLoad(vm: jni::JavaVM, res: *mut std::os::raw::c_void) -> jni::sys::jint {
    if let Ok(env) = vm.get_env() {
        let vm = vm.get_java_vm_pointer() as *mut std::os::raw::c_void;
        init_ndk_context(vm, res);
    }
    jni::JNIVersion::V6.into()
}

#[no_mangle]
pub extern "system" fn Java_ffi_FFI_onAppStart(mut env: JNIEnv, _class: JClass, ctx: JObject) {
    if ctx.is_null() {
        log::error!("application context is null");
        return;
    }
    if APPLICATION_CONTEXT.read().unwrap().is_some() {
        log::info!("application context already initialized");
        return;
    }
    if let Ok(jvm) = env.get_java_vm() {
        if let Ok(context) = env.new_global_ref(ctx) {
            let java_vm = jvm.get_java_vm_pointer() as *mut c_void;
            let context_jobject = context.as_obj().as_raw() as *mut c_void;
            *APPLICATION_CONTEXT.write().unwrap() = Some(context);
            *JVM.write().unwrap() = Some(jvm);
            if let Err(error) = base::fs::android::initialize(call_document_storage) {
                log::error!("Unable to initialize document storage: {}", error);
            }
            init_ndk_context(java_vm, context_jobject);
            try_init_rustls_platform_verifier(&mut env, context_jobject);
        }
    }
}

fn call_document_storage(request: &str) -> hbb_common::ResultType<String> {
    use hbb_common::anyhow::anyhow;

    const LOCAL_REFS: i32 = 4;
    let context = APPLICATION_CONTEXT
        .read()
        .unwrap()
        .clone()
        .ok_or(anyhow!("Android application context is unavailable"))?;
    let jvm = JVM.read().unwrap();
    let jvm = jvm.as_ref().ok_or(anyhow!("Android VM is unavailable"))?;
    let mut env = jvm.attach_current_thread_as_daemon()?;
    let result = env.with_local_frame(LOCAL_REFS, |env| -> JniResult<String> {
        let request = env.new_string(request)?;
        let value = env
            .call_method(
                &context,
                "rustDocumentStorage",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&JObject::from(request))],
            )?
            .l()?;
        let value = JString::from(value);
        let value = env.get_string(&value)?;
        Ok(value.into())
    });
    if result.is_err() && env.exception_check()? {
        env.exception_clear()?;
    }
    Ok(result?)
}
