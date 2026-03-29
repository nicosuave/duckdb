use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=DUCKDB_LINENOISE_DIR");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let obj_dir = out_dir.join("obj");
    let _ = std::fs::create_dir_all(&obj_dir);

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let workspace_root = manifest_dir.join("../..");
    let linenoise_dir = env::var_os("DUCKDB_LINENOISE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("vendor/duckdb/1.4.3/linenoise"));

    let duckdb_include = workspace_root.join("vendor/duckdb/1.4.3/include");
    let utf8proc_include = workspace_root.join("third_party/utf8proc/include");
    let sqlite3_api_wrapper_include =
        workspace_root.join("vendor/duckdb/1.4.3/sqlite3_api_wrapper");
    let include_dir = linenoise_dir.join("include");

    let sources = [
        "linenoise.cpp",
        "linenoise-c.cpp",
        "history.cpp",
        "terminal.cpp",
        "rendering.cpp",
        "highlighting.cpp",
    ];

    let mut objects: Vec<PathBuf> = Vec::new();
    for src in sources {
        let src_path = linenoise_dir.join(src);
        println!("cargo:rerun-if-changed={}", src_path.display());
        let obj_path = obj_dir.join(format!("{}.o", src));
        let mut cxx = Command::new("c++");
        cxx.arg("-std=c++17")
            .arg("-ULINENOISE_LOGGING")
            .arg(format!("-I{}", include_dir.display()))
            .arg(format!("-I{}", duckdb_include.display()))
            .arg(format!("-I{}", utf8proc_include.display()))
            .arg(format!("-I{}", sqlite3_api_wrapper_include.display()))
            .arg("-c")
            .arg(&src_path)
            .arg("-o")
            .arg(&obj_path);

        let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
        if target_os == "macos" {
            cxx.arg("-mmacosx-version-min=11.0");
        }
        if target_os == "linux" {
            cxx.arg("-fPIC");
        }
        let status = cxx.status().expect("failed to invoke c++");
        if !status.success() {
            panic!("c++ failed with status {status}");
        }
        objects.push(obj_path);
    }

    let complete_src = manifest_dir.join("src/duckdb_shell_sqlite3_complete.cc");
    println!("cargo:rerun-if-changed={}", complete_src.display());
    let complete_obj = obj_dir.join("duckdb_shell_sqlite3_complete.cc.o");
    let mut cxx = Command::new("c++");
    cxx.arg("-std=c++17")
        .arg("-ULINENOISE_LOGGING")
        .arg("-c")
        .arg(&complete_src)
        .arg("-o")
        .arg(&complete_obj);
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    if target_os == "macos" {
        cxx.arg("-mmacosx-version-min=11.0");
    }
    if target_os == "linux" {
        cxx.arg("-fPIC");
    }
    let status = cxx.status().expect("failed to invoke c++");
    if !status.success() {
        panic!("c++ failed with status {status}");
    }
    objects.push(complete_obj);

    let color_mode_src = manifest_dir.join("src/duckdb_cli_terminal_color_mode.cc");
    println!("cargo:rerun-if-changed={}", color_mode_src.display());
    let color_mode_obj = obj_dir.join("duckdb_cli_terminal_color_mode.cc.o");
    let mut cxx = Command::new("c++");
    cxx.arg("-std=c++17")
        .arg("-ULINENOISE_LOGGING")
        .arg("-c")
        .arg(&color_mode_src)
        .arg("-o")
        .arg(&color_mode_obj);
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    if target_os == "macos" {
        cxx.arg("-mmacosx-version-min=11.0");
    }
    if target_os == "linux" {
        cxx.arg("-fPIC");
    }
    let status = cxx.status().expect("failed to invoke c++");
    if !status.success() {
        panic!("c++ failed with status {status}");
    }
    objects.push(color_mode_obj);

    let toggles_src = manifest_dir.join("src/duckdb_linenoise_render_toggles.cc");
    println!("cargo:rerun-if-changed={}", toggles_src.display());
    let toggles_obj = obj_dir.join("duckdb_linenoise_render_toggles.cc.o");
    let mut cxx = Command::new("c++");
    cxx.arg("-std=c++17")
        .arg("-ULINENOISE_LOGGING")
        .arg(format!("-I{}", include_dir.display()))
        .arg(format!("-I{}", duckdb_include.display()))
        .arg(format!("-I{}", utf8proc_include.display()))
        .arg(format!("-I{}", sqlite3_api_wrapper_include.display()))
        .arg("-c")
        .arg(&toggles_src)
        .arg("-o")
        .arg(&toggles_obj);
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    if target_os == "macos" {
        cxx.arg("-mmacosx-version-min=11.0");
    }
    if target_os == "linux" {
        cxx.arg("-fPIC");
    }
    let status = cxx.status().expect("failed to invoke c++");
    if !status.success() {
        panic!("c++ failed with status {status}");
    }
    objects.push(toggles_obj);

    let lib_path = out_dir.join("libduckdb_linenoise.a");
    let mut ar = Command::new("ar");
    ar.arg("crus").arg(&lib_path);
    for obj in &objects {
        ar.arg(obj);
    }
    let status = ar.status().expect("failed to invoke ar");
    if !status.success() {
        panic!("ar failed with status {status}");
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=duckdb_linenoise");

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    match target_os.as_str() {
        "macos" => println!("cargo:rustc-link-lib=c++"),
        "linux" => println!("cargo:rustc-link-lib=stdc++"),
        other => panic!("unsupported target OS: {other}"),
    }
}
