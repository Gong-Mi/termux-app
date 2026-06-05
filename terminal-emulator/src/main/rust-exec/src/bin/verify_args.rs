use std::env;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

fn main() {
    // 直接从内核获取原始 OsString 列表
    let args: Vec<OsString> = env::args_os().collect();
    
    println!("--- [Termux Argument Integrity Check] ---");
    println!("Process PID: {}", std::process::id());
    println!("Total argv count: {}", args.len());
    
    for (i, arg) in args.iter().enumerate() {
        let bytes = arg.as_bytes();
        
        // 打印索引和 Debug 表示（会自动转义不可打印字符）
        print!("argv[{}] = {:?}", i, arg);
        
        // 打印十六进制，确保字节级别的精确对齐
        print!(" (Hex: ");
        for b in bytes {
            print!("{:02x} ", b);
        }
        println!(")");
    }
    
    println!("------------------------------------------");
    
    // 验证某些预期的特殊测试用例
    if args.len() > 1 {
        let first_arg = args[1].to_string_lossy();
        if first_arg == "test_space" && args.len() == 2 {
             println!("[SUCCESS] Simple space test passed.");
        }
    }
}
