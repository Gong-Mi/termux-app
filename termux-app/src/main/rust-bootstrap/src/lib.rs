use jni::JNIEnv;
use jni::objects::{JClass, JByteArray};
use jni::sys::jbyteArray;

// Embed the bootstrap ZIP based on the target architecture.
// The paths are relative to the src/ directory.
#[cfg(target_arch = "aarch64")]
static BOOTSTRAP_ZIP: &[u8] = include_bytes!("../../cpp/bootstrap-aarch64.zip");

#[cfg(target_arch = "arm")]
static BOOTSTRAP_ZIP: &[u8] = include_bytes!("../../cpp/bootstrap-arm.zip");

#[cfg(target_arch = "x86_64")]
static BOOTSTRAP_ZIP: &[u8] = include_bytes!("../../cpp/bootstrap-x86_64.zip");

#[cfg(target_arch = "x86")]
static BOOTSTRAP_ZIP: &[u8] = include_bytes!("../../cpp/bootstrap-i686.zip");

// Fallback for development/testing if architecture is not matched
#[cfg(not(any(target_arch = "aarch64", target_arch = "arm", target_arch = "x86_64", target_arch = "x86")))]
static BOOTSTRAP_ZIP: &[u8] = b"Unsupported architecture for bootstrap";

/// Returns the pointer to the embedded bootstrap data.
/// This can be used by the main Rust engine via dlsym to avoid memory copies.
#[no_mangle]
pub extern "C" fn termux_bootstrap_get_data() -> *const u8 {
    BOOTSTRAP_ZIP.as_ptr()
}

/// Returns the size of the embedded bootstrap data.
#[no_mangle]
pub extern "C" fn termux_bootstrap_get_size() -> usize {
    BOOTSTRAP_ZIP.len()
}

/// JNI interface for the legacy Java installer.
/// Corresponds to com.termux.app.TermuxInstaller.getZip()
#[no_mangle]
pub extern "system" fn Java_com_termux_app_TermuxInstaller_getZip(
    mut env: JNIEnv,
    _class: JClass,
) -> jbyteArray {
    match env.byte_array_from_slice(BOOTSTRAP_ZIP) {
        Ok(array) => array.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_access() {
        let ptr = termux_bootstrap_get_data();
        let size = termux_bootstrap_get_size();
        assert!(size > 0);
        let slice = unsafe { std::slice::from_raw_parts(ptr, size) };
        assert_eq!(slice, BOOTSTRAP_ZIP);
        println!("Data verified, size: {} bytes", size);
    }
}
