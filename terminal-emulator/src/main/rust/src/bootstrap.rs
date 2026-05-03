//! Bootstrap Extractor Module
//!
//! Provides functionality to extract bootstrap zip to target directory.
//! Also embeds the bootstrap zip data at compile time and exposes JNI getZip().

use std::fs::{File, create_dir_all};
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

/// 编译时嵌入的 bootstrap zip 数据（路径由 build.rs 根据目标架构设置）
static BOOTSTRAP_ZIP: &[u8] = include_bytes!(env!("BOOTSTRAP_ZIP_PATH"));

/// JNI: 返回嵌入的 bootstrap zip 字节数组
/// 对应 com.termux.app.TermuxInstaller.getZip()
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_termux_app_TermuxInstaller_getZip(
    env: jni::JNIEnv,
    _class: jni::objects::JClass,
) -> jni::sys::jbyteArray {
    match env.byte_array_from_slice(BOOTSTRAP_ZIP) {
        Ok(array) => array.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

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
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    zip_bytes: jni::objects::JByteArray,
    target_dir: jni::objects::JString,
) -> jni::sys::jlong {
    eprintln!("[Rust Bootstrap] ========== [Extraction Start] ==========");

    // 获取目标目录路径
    let target_dir_str: String = match env.get_string(&target_dir) {
        Ok(s) => {
            let s: String = s.into();
            eprintln!("[Rust Bootstrap] [OK] Target directory: {}", s);
            s
        }
        Err(e) => {
            eprintln!(
                "[Rust Bootstrap] [ERROR] Failed to get target directory: {:?}",
                e
            );
            return -2;
        }
    };

    // 获取 zip 字节数据
    let zip_data: Vec<u8> = match env.convert_byte_array(&zip_bytes) {
        Ok(data) => {
            eprintln!(
                "[Rust Bootstrap] [OK] Zip data loaded, size: {} bytes",
                data.len()
            );
            data
        }
        Err(e) => {
            eprintln!(
                "[Rust Bootstrap] [ERROR] Failed to convert byte array: {:?}",
                e
            );
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
        }
        Err(e) => {
            eprintln!("[Rust Bootstrap] [ERROR] Bootstrap extract error: {:?}", e);
            eprintln!("[Rust Bootstrap] ========== [Extraction Failed] ==========");
            -4
        }
    }
}

/// 解压 zip 到指定目录
fn extract_zip_to_dir(
    zip_bytes: &[u8],
    target_dir: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    eprintln!("[Rust Extract] Opening zip archive...");
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(reader)?;

    let total_entries = archive.len();
    eprintln!(
        "[Rust Extract] Archive opened, total entries: {}",
        total_entries
    );

    let mut extracted_count = 0;
    let mut symlinks: Vec<(String, String)> = Vec::new();
    let mut dir_count = 0;
    let mut file_count = 0;
    let mut symlink_count = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let file_path = file.enclosed_name().ok_or("Invalid file path")?;
        let path_str = file_path.to_string_lossy().to_string();

        // 构建目标路径
        let out_path = Path::new(target_dir).join(&file_path);

        // 处理目录
        if file.is_dir() {
            dir_count += 1;
            extracted_count += 1;
            create_dir_all(&out_path)?;
            eprintln!("[Rust Extract] [{}] Created directory: {}", i, path_str);
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
            eprintln!(
                "[Rust Extract] Found {} symlinks in SYMLINKS.txt",
                symlink_count
            );
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

        eprintln!(
            "[Rust Extract] [{}] Extracted: {} ({} bytes)",
            i, path_str, bytes_copied
        );

        // 设置执行权限 (bin/, libexec/ 等目录)
        let path_str = file_path.to_string_lossy();
        if path_str.contains("/bin/") 
            || path_str.starts_with("bin/")
            || path_str.contains("/libexec/")
            || path_str.starts_with("libexec/")
            || path_str.contains("/lib/apt/")
            || path_str.starts_with("lib/apt/")
            || path_str.ends_with("/bin") // Case like busybox
        {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&out_path)?.permissions();
                
                // BUG FIX: For Android 14+ compatibility, DEX files and their directories
                // MUST NOT be writable. 0o700 was causing `am` (app_process) to Abort.
                // We use 0o500 (read-execute) for binaries/dirs and 0o400 for data/apks.
                if path_str.ends_with(".apk") {
                    perms.set_mode(0o400);
                } else if file.is_dir() {
                    perms.set_mode(0o500);
                } else {
                    perms.set_mode(0o500);
                }
                
                std::fs::set_permissions(&out_path, perms)?;
                eprintln!(
                    "[Rust Extract] [{}] Set secure permissions: {} (mode: {:o})",
                    i, path_str, std::fs::metadata(&out_path)?.permissions().mode()
                );
            }
        }

        file_count += 1;
        extracted_count += 1;
    }

    eprintln!(
        "[Rust Extract] Extraction summary: {} dirs, {} files, {} symlinks to create",
        dir_count, file_count, symlink_count
    );

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
    use std::io::Write;

    #[test]
    fn test_extract_zip_with_empty_dir() {
        let target_dir_path = "test_extract";
        // 清理旧目录
        let _ = std::fs::remove_dir_all(target_dir_path);
        std::fs::create_dir_all(target_dir_path).unwrap();

        // 创建一个内存中的 zip，包含一个空目录和一个文件
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            
            // 添加空目录 (注意末尾的斜杠)
            zip.add_directory("empty_dir/", zip::write::SimpleFileOptions::default()).unwrap();
            
            // 添加文件
            zip.start_file("test.txt", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"hello").unwrap();
            
            zip.finish().unwrap();
        }

        let count = extract_zip_to_dir(&buf, target_dir_path).unwrap();
        assert_eq!(count, 2);

        // 验证目录是否创建
        let empty_dir_path = std::path::Path::new(target_dir_path).join("empty_dir");
        assert!(empty_dir_path.exists(), "Empty directory should exist at {:?}", empty_dir_path);
        assert!(empty_dir_path.is_dir(), "{:?} should be a directory", empty_dir_path);

        // 验证文件是否创建
        let file_path = std::path::Path::new(target_dir_path).join("test.txt");
        assert!(file_path.exists());
        let mut content = String::new();
        File::open(file_path).unwrap().read_to_string(&mut content).unwrap();
        assert_eq!(content, "hello");

        // 清理
        let _ = std::fs::remove_dir_all(target_dir_path);
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;
    use std::io::Write;

    // -------------------------------------------------------------------------
    // Path traversal protection
    // -------------------------------------------------------------------------
    #[test]
    fn extract_zip_rejects_path_traversal() {
        let target_dir = "test_extract_traversal";
        let _ = std::fs::remove_dir_all(target_dir);

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            // zip crate's enclosed_name() should reject this
            zip.start_file("../../etc/passwd", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"root").unwrap();
            zip.finish().unwrap();
        }

        let result = extract_zip_to_dir(&buf, target_dir);
        // enclosed_name returns None for paths with .. components,
        // causing ok_or("Invalid file path") to fail the entire extraction
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(target_dir);
    }

    #[test]
    fn extract_zip_rejects_absolute_path() {
        // Note: ZipWriter normalizes absolute paths to relative, so /etc/passwd
        // becomes etc/passwd. The enclosed_name() in zip crate also strips leading /.
        // We test that the extraction still lands inside target_dir (not /etc).
        let target_dir = "test_extract_absolute";
        let _ = std::fs::remove_dir_all(target_dir);

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zip.start_file("/etc/passwd", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"root").unwrap();
            zip.finish().unwrap();
        }

        // Should succeed but extract to target_dir/etc/passwd, NOT /etc/passwd
        let count = extract_zip_to_dir(&buf, target_dir).unwrap();
        assert_eq!(count, 1);
        let extracted = std::path::Path::new(target_dir).join("etc/passwd");
        assert!(extracted.exists());
        // Verify it did not write to the real /etc/passwd by comparing content
        let real_content = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let extracted_content = std::fs::read_to_string(&extracted).unwrap();
        assert_ne!(real_content, extracted_content);

        let _ = std::fs::remove_dir_all(target_dir);
    }

    // -------------------------------------------------------------------------
    // File extraction with parent directories
    // -------------------------------------------------------------------------
    #[test]
    fn extract_zip_nested_directories() {
        let target_dir = "test_extract_nested";
        let _ = std::fs::remove_dir_all(target_dir);

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zip.add_directory("a/", zip::write::SimpleFileOptions::default()).unwrap();
            zip.add_directory("a/b/", zip::write::SimpleFileOptions::default()).unwrap();
            zip.start_file("a/b/c.txt", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"nested").unwrap();
            zip.finish().unwrap();
        }

        let count = extract_zip_to_dir(&buf, target_dir).unwrap();
        assert_eq!(count, 3); // 2 dirs + 1 file

        let file_path = std::path::Path::new(target_dir).join("a/b/c.txt");
        assert!(file_path.exists());
        let mut content = String::new();
        File::open(file_path).unwrap().read_to_string(&mut content).unwrap();
        assert_eq!(content, "nested");

        let _ = std::fs::remove_dir_all(target_dir);
    }

    // -------------------------------------------------------------------------
    // Empty zip
    // -------------------------------------------------------------------------
    #[test]
    fn extract_empty_zip() {
        let target_dir = "test_extract_empty";
        let _ = std::fs::remove_dir_all(target_dir);

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zip.finish().unwrap();
        }

        let count = extract_zip_to_dir(&buf, target_dir).unwrap();
        assert_eq!(count, 0);

        let _ = std::fs::remove_dir_all(target_dir);
    }

    // -------------------------------------------------------------------------
    // SYMLINKS.txt parsing
    // -------------------------------------------------------------------------
    #[test]
    fn extract_zip_with_symlinks() {
        let target_dir = "test_extract_symlinks";
        let _ = std::fs::remove_dir_all(target_dir);
        std::fs::create_dir_all(target_dir).unwrap();

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zip.start_file("SYMLINKS.txt", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all("old1←new1\nold2←new2\n".as_bytes()).unwrap();
            zip.start_file("real_file", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"data").unwrap();
            zip.finish().unwrap();
        }

        let count = extract_zip_to_dir(&buf, target_dir).unwrap();
        // 1 file + SYMLINKS.txt processed but not counted as extracted
        // + 2 symlinks created afterwards
        // extracted_count counts SYMLINKS.txt as skipped, real_file = 1, plus 2 symlinks
        // Wait, SYMLINKS.txt is skipped via `continue`, so extracted_count only gets real_file (1)
        // Then symlinks are created but not counted in extracted_count
        assert_eq!(count, 1);

        #[cfg(unix)]
        {
            let link1 = std::path::Path::new(target_dir).join("new1");
            let link2 = std::path::Path::new(target_dir).join("new2");
            // symlink_metadata checks the link itself, not the target
            assert!(std::fs::symlink_metadata(&link1).is_ok());
            assert!(std::fs::symlink_metadata(&link2).is_ok());
            assert!(link1.is_symlink());
            assert!(link2.is_symlink());
        }

        let _ = std::fs::remove_dir_all(target_dir);
    }

    // -------------------------------------------------------------------------
    // Overwriting existing files
    // -------------------------------------------------------------------------
    #[test]
    fn extract_zip_overwrites_existing() {
        let target_dir = "test_extract_overwrite";
        let _ = std::fs::remove_dir_all(target_dir);
        std::fs::create_dir_all(target_dir).unwrap();

        // Pre-create file with old content
        let existing = std::path::Path::new(target_dir).join("file.txt");
        std::fs::write(&existing, "old").unwrap();

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zip.start_file("file.txt", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"new").unwrap();
            zip.finish().unwrap();
        }

        let count = extract_zip_to_dir(&buf, target_dir).unwrap();
        assert_eq!(count, 1);

        let content = std::fs::read_to_string(&existing).unwrap();
        assert_eq!(content, "new");

        let _ = std::fs::remove_dir_all(target_dir);
    }
}
