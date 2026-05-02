use std::env;

fn main() {
    let target = env::var("TARGET").unwrap();
    let arch = target.split('-').next().unwrap();

    let zip_name = match arch {
        "aarch64" => "bootstrap-aarch64.zip",
        "armv7" => "bootstrap-arm.zip",
        "i686" => "bootstrap-i686.zip",
        "x86_64" => "bootstrap-x86_64.zip",
        _ => panic!("Unsupported target architecture for bootstrap: {}", target),
    };

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let zip_path = format!("{}/../../../../termux-app/src/main/cpp/{}", manifest_dir, zip_name);

    if !std::path::Path::new(&zip_path).exists() {
        panic!(
            "Bootstrap zip not found: {} (run ./gradlew :termux-app:downloadBootstraps)",
            zip_path
        );
    }

    println!("cargo:rustc-env=BOOTSTRAP_ZIP_PATH={}", zip_path);
    println!("cargo:rerun-if-changed={}", zip_path);
}
