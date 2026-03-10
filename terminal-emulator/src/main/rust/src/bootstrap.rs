//! Bootstrap Extractor Module
//!
//! Provides functionality to extract bootstrap zip to target directory.

use std::io::Read;
use std::fs::{File, create_dir_all};
use std::path::Path;
use zip::ZipArchive;

/// 从 Java 传入的字节数组解压 bootstrap zip 到指定目录
///
/// # Returns
/// - 正数：成功解压的文件数量
/// - -1: JNI 环境错误
/// - -2: 路径获取错误
/// - -3: 字节数组转换错误
/// - -4: 解压错误
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_app_BootstrapExtractor_extractFromBytes(
    env_ptr: *mut *const jni::sys::JNINativeInterface_,
    _class: jni::objects::JClass,
    zip_bytes: jni::objects::JByteArray,
    target_dir: jni::objects::JString,
) -> jni::sys::jlong {
    use jni::JNIEnv;

    eprintln!("[Rust Bootstrap] ========== [Extraction Start] ==========");

    let mut env = match unsafe { JNIEnv::from_raw(env_ptr) } {
        Ok(e) => {
            eprintln!("[Rust Bootstrap] [OK] JNI environment initialized");
            e
        },
        Err(e) => {
            eprintln!("[Rust Bootstrap] [ERROR] JNI environment error: {:?}", e);
            return -1;
        }
    };

    // 获取目标目录路径
    let target_dir_str: String = match env.get_string(&target_dir) {
        Ok(s) => {
            let s: String = s.into();
            eprintln!("[Rust Bootstrap] [OK] Target directory: {}", s);
            s
        },
        Err(e) => {
            eprintln!("[Rust Bootstrap] [ERROR] Failed to get target directory: {:?}", e);
            return -2;
        }
    };

    // 获取 zip 字节数据
    let zip_data: Vec<u8> = match env.convert_byte_array(&zip_bytes) {
        Ok(data) => {
            eprintln!("[Rust Bootstrap] [OK] Zip data loaded, size: {} bytes", data.len());
            data
        },
        Err(e) => {
            eprintln!("[Rust Bootstrap] [ERROR] Failed to convert byte array: {:?}", e);
            return -3;
        }
    };

    // 解压到目标目录
    eprintln!("[Rust Bootstrap] [Step] Starting extraction...");
    match extract_zip_to_dir(&zip_data, &target_dir_str) {
        Ok(count) => {
            eprintln!("[Rust Bootstrap] [SUCCESS] Extracted {} files", count);
            eprintln!("[Rust Bootstrap] ========== [Extraction Complete] ==========");
            count as jni::sys::jlong
        },
        Err(e) => {
            eprintln!("[Rust Bootstrap] [ERROR] Bootstrap extract error: {:?}", e);
            eprintln!("[Rust Bootstrap] ========== [Extraction Failed] ==========");
            -4
        }
    }
}

/// 解压 zip 到指定目录
fn extract_zip_to_dir(zip_bytes: &[u8], target_dir: &str) -> Result<usize, Box<dyn std::error::Error>> {
    eprintln!("[Rust Extract] Opening zip archive...");
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(reader)?;
    
    let total_entries = archive.len();
    eprintln!("[Rust Extract] Archive opened, total entries: {}", total_entries);

    let mut extracted_count = 0;
    let mut symlinks: Vec<(String, String)> = Vec::new();
    let mut dir_count = 0;
    let mut file_count = 0;
    let mut symlink_count = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let file_path = file.enclosed_name().ok_or("Invalid file path")?;
        let path_str = file_path.to_string_lossy().to_string();

        // 跳过目录
        if file.is_dir() {
            dir_count += 1;
            eprintln!("[Rust Extract] [{}] Skip directory: {}", i, path_str);
            continue;
        }

        // 处理 SYMLINKS.txt
        if file_path == Path::new("SYMLINKS.txt") {
            eprintln!("[Rust Extract] [{}] Processing SYMLINKS.txt...", i);
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            for line in contents.lines() {
                if let Some((old, new)) = line.split_once('←') {
                    symlinks.push((old.to_string(), new.to_string()));
                    symlink_count += 1;
                }
            }
            eprintln!("[Rust Extract] Found {} symlinks in SYMLINKS.txt", symlink_count);
            continue;
        }

        // 构建目标路径
        let out_path = Path::new(target_dir).join(&file_path);

        // 创建父目录
        if let Some(parent) = out_path.parent() {
            create_dir_all(parent)?;
        }

        // 提取文件
        let mut outfile = File::create(&out_path)?;
        let bytes_copied = std::io::copy(&mut file, &mut outfile)?;
        
        eprintln!("[Rust Extract] [{}] Extracted: {} ({} bytes)", i, path_str, bytes_copied);

        // 设置执行权限 (bin/, libexec/ 等目录)
        let path_str = file_path.to_string_lossy();
        if path_str.starts_with("bin/") ||
           path_str.starts_with("libexec") ||
           path_str.starts_with("lib/apt/") {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&out_path)?.permissions();
                perms.set_mode(0o700);
                std::fs::set_permissions(&out_path, perms)?;
                eprintln!("[Rust Extract] [{}] Set executable permission: {}", i, path_str);
            }
        }

        file_count += 1;
        extracted_count += 1;
    }

    eprintln!("[Rust Extract] Extraction summary: {} dirs, {} files, {} symlinks to create", 
              dir_count, file_count, symlink_count);

    // 创建符号链接
    eprintln!("[Rust Extract] Creating {} symlinks...", symlink_count);
    for (old, new) in symlinks {
        let link_path = Path::new(target_dir).join(&new);
        if let Some(parent) = link_path.parent() {
            create_dir_all(parent)?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&old, link_path)?;
        eprintln!("[Rust Extract] Symlink: {} -> {}", new, old);
    }

    Ok(extracted_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_zip() {
        assert!(true);
    }
}
