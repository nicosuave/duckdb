use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/echo.cc");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=DUCKDB_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=DUCKDB_VENDOR_VERSION");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let echo_obj_path = out_dir.join("echo.o");
    let lib_path = out_dir.join("libduckdb_shellshim.a");

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let workspace_root = manifest_dir.join("../..");
    let vendor_version = env::var("DUCKDB_VENDOR_VERSION").unwrap_or_else(|_| "1.5.3".to_string());
    let duckdb_include = env::var_os("DUCKDB_INCLUDE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join(format!("vendor/duckdb/{vendor_version}/include")));

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    let deployment_target = env::var("MACOSX_DEPLOYMENT_TARGET").ok();

    let compile = |src: &str, obj: &PathBuf| {
        let mut cxx = Command::new("c++");
        cxx.arg("-std=c++17").arg(format!("-I{}", duckdb_include.display()));
        if target_os == "macos" {
            if let Some(target) = deployment_target.as_deref() {
                cxx.arg(format!("-mmacosx-version-min={}", target));
            }
        }
        if target_os == "linux" {
            cxx.arg("-fPIC");
        }
        cxx.arg("-c").arg(src).arg("-o").arg(obj);
        let status = cxx.status().expect("failed to invoke c++");
        if !status.success() {
            panic!("c++ failed with status {status}");
        }
    };

    compile("src/echo.cc", &echo_obj_path);

    let status = Command::new("ar")
        .arg("crus")
        .arg(&lib_path)
        .arg(&echo_obj_path)
        .status()
        .expect("failed to invoke ar");
    if !status.success() {
        panic!("ar failed with status {status}");
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=duckdb_shellshim");

    match target_os.as_str() {
        // Rust links with `cc`, so we must explicitly link the C++ stdlib when we ship C++ objects.
        "macos" => println!("cargo:rustc-link-lib=c++"),
        "linux" => println!("cargo:rustc-link-lib=stdc++"),
        other => panic!("unsupported target OS: {other}"),
    }
}
