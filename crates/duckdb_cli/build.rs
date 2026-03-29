use std::env;
use std::path::{Path, PathBuf};

fn candidate_lib_name(target_os: &str) -> &'static str {
    match target_os {
        "macos" => "libduckdb.dylib",
        "linux" => "libduckdb.so",
        other => panic!("unsupported target OS: {other}"),
    }
}

fn existing_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

fn main() {
    println!("cargo:rerun-if-env-changed=DUCKDB_LIB_DIR");
    println!("cargo:rerun-if-env-changed=DUCKDB_LIB_SOURCE");
    println!("cargo:rerun-if-env-changed=DUCKDB_VENDOR_VERSION");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    let lib_name = candidate_lib_name(&target_os);
    let vendor_version = env::var("DUCKDB_VENDOR_VERSION").unwrap_or_else(|_| "1.4.3".to_string());
    let lib_source = env::var("DUCKDB_LIB_SOURCE").unwrap_or_else(|_| "vendor".to_string());

    let workspace_root =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"))
            .join("../..");

    let mut lib_dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = env::var_os("DUCKDB_LIB_DIR") {
        lib_dirs.push(PathBuf::from(dir));
    }

    let repo_release = PathBuf::from(format!("{}/build/release/src", workspace_root.display()));
    let repo_debug = PathBuf::from(format!("{}/build/debug/src", workspace_root.display()));
    let vendor_dir = PathBuf::from(format!(
        "{}/vendor/duckdb/{}/lib/{}",
        workspace_root.display(),
        vendor_version,
        match target_os.as_str() {
            "macos" => "darwin",
            "linux" => "linux",
            _ => unreachable!(),
        }
    ));

    match lib_source.as_str() {
        "vendor" => {
            lib_dirs.push(vendor_dir);
            lib_dirs.push(repo_release);
            lib_dirs.push(repo_debug);
        }
        "repo" | "" => {
            lib_dirs.push(repo_release);
            lib_dirs.push(repo_debug);
            lib_dirs.push(vendor_dir);
        }
        other => {
            panic!("Unknown DUCKDB_LIB_SOURCE='{other}' (expected 'repo' or 'vendor')");
        }
    }

    for dir in &lib_dirs {
        println!("cargo:rerun-if-changed={}", dir.join(lib_name).display());
    }

    let found_dir = lib_dirs
        .iter()
        .find(|dir| existing_file(&dir.join(lib_name)))
        .cloned();

    match target_os.as_str() {
        "macos" => println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path"),
        "linux" => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN"),
        other => panic!("unsupported target OS: {other}"),
    }

    if let Some(found_dir) = found_dir {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", found_dir.display());
    } else {
        println!("cargo:warning=could not find libduckdb runtime directory for rpath (expected {lib_name})");
        println!("cargo:warning=set DUCKDB_LIB_DIR to the directory containing libduckdb");
    }
}
