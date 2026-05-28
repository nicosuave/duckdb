use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn compile_cpp(
    src_path: &Path,
    obj_path: &Path,
    include_dirs: &[PathBuf],
    disable_linenoise_editor: bool,
    target_os: &str,
    is_msvc: bool,
) {
    let cxx_bin = env::var("CXX").unwrap_or_else(|_| {
        if is_msvc {
            "cl".to_string()
        } else {
            "c++".to_string()
        }
    });
    let mut cxx = Command::new(cxx_bin);
    if is_msvc {
        cxx.arg("/std:c++17")
            .arg("/EHsc")
            .arg("/ULINENOISE_LOGGING");
        if disable_linenoise_editor {
            cxx.arg("/DDUCKDB_RUST_CLI_DISABLE_LINENOISE_EDITOR");
        }
        for include_dir in include_dirs {
            cxx.arg(format!("/I{}", include_dir.display()));
        }
        cxx.arg("/c")
            .arg(src_path)
            .arg(format!("/Fo{}", obj_path.display()));
    } else {
        cxx.arg("-std=c++17").arg("-ULINENOISE_LOGGING");
        if disable_linenoise_editor {
            cxx.arg("-DDUCKDB_RUST_CLI_DISABLE_LINENOISE_EDITOR");
        }
        for include_dir in include_dirs {
            cxx.arg(format!("-I{}", include_dir.display()));
        }
        cxx.arg("-c").arg(src_path).arg("-o").arg(obj_path);
        if target_os == "macos" {
            cxx.arg("-mmacosx-version-min=11.0");
        }
        if target_os == "linux" || target_os == "windows" {
            cxx.arg("-fPIC");
        }
    }

    let status = cxx.status().expect("failed to invoke C++ compiler");
    if !status.success() {
        panic!("C++ compiler failed with status {status}");
    }
}

fn static_lib_path(out_dir: &Path, name: &str, is_msvc: bool) -> PathBuf {
    if is_msvc {
        out_dir.join(format!("{name}.lib"))
    } else {
        out_dir.join(format!("lib{name}.a"))
    }
}

fn archive_static_library(out_dir: &Path, name: &str, objects: &[PathBuf], is_msvc: bool) {
    let lib_path = static_lib_path(out_dir, name, is_msvc);
    let ar_bin = env::var("AR").unwrap_or_else(|_| {
        if is_msvc {
            "lib".to_string()
        } else {
            "ar".to_string()
        }
    });
    let mut ar = Command::new(ar_bin);
    if is_msvc {
        ar.arg(format!("/OUT:{}", lib_path.display()));
    } else {
        ar.arg("crus").arg(&lib_path);
    }
    for obj in objects {
        ar.arg(obj);
    }
    let status = ar.status().expect("failed to invoke archiver");
    if !status.success() {
        panic!("archiver failed with status {status}");
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ENV");
    println!("cargo:rerun-if-env-changed=CXX");
    println!("cargo:rerun-if-env-changed=AR");
    println!("cargo:rerun-if-env-changed=DUCKDB_LINENOISE_DIR");
    println!("cargo:rerun-if-env-changed=DUCKDB_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=DUCKDB_FMT_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=DUCKDB_SQLITE3_API_WRAPPER_DIR");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let obj_dir = out_dir.join("obj");
    let _ = std::fs::create_dir_all(&obj_dir);

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let workspace_root = manifest_dir.join("../..");
    let linenoise_dir = env::var_os("DUCKDB_LINENOISE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("tools/shell/linenoise"));

    let duckdb_include = env::var_os("DUCKDB_INCLUDE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("src/include"));
    let fmt_include = env::var_os("DUCKDB_FMT_INCLUDE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("third_party/fmt/include"));
    let utf8proc_include = workspace_root.join("third_party/utf8proc/include");
    let sqlite3_api_wrapper_include = env::var_os("DUCKDB_SQLITE3_API_WRAPPER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("tools/shell/include"));
    let shim_include = manifest_dir.join("include");
    let include_dir = linenoise_dir.join("include");
    let include_dirs = vec![
        shim_include.clone(),
        include_dir.clone(),
        duckdb_include.clone(),
        fmt_include.clone(),
        utf8proc_include.clone(),
        sqlite3_api_wrapper_include.clone(),
    ];
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let is_msvc = target_os == "windows" && target_env == "msvc";

    println!(
        "cargo:rerun-if-changed={}",
        shim_include.join("shell_highlight.hpp").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        shim_include.join("shell_state.hpp").display()
    );

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
        let obj_path = obj_dir.join(if is_msvc {
            format!("{src}.obj")
        } else {
            format!("{src}.o")
        });
        compile_cpp(
            &src_path,
            &obj_path,
            &include_dirs,
            true,
            &target_os,
            is_msvc,
        );
        objects.push(obj_path);
    }

    let complete_src = manifest_dir.join("src/duckdb_shell_sqlite3_complete.cc");
    println!("cargo:rerun-if-changed={}", complete_src.display());
    let complete_obj = obj_dir.join(if is_msvc {
        "duckdb_shell_sqlite3_complete.cc.obj"
    } else {
        "duckdb_shell_sqlite3_complete.cc.o"
    });
    compile_cpp(
        &complete_src,
        &complete_obj,
        &[],
        false,
        &target_os,
        is_msvc,
    );
    objects.push(complete_obj);

    let color_mode_src = manifest_dir.join("src/duckdb_cli_terminal_color_mode.cc");
    println!("cargo:rerun-if-changed={}", color_mode_src.display());
    let color_mode_obj = obj_dir.join(if is_msvc {
        "duckdb_cli_terminal_color_mode.cc.obj"
    } else {
        "duckdb_cli_terminal_color_mode.cc.o"
    });
    compile_cpp(
        &color_mode_src,
        &color_mode_obj,
        &[],
        false,
        &target_os,
        is_msvc,
    );
    objects.push(color_mode_obj);

    let highlight_bridge_src = manifest_dir.join("src/duckdb_cli_highlight_bridge.cc");
    println!("cargo:rerun-if-changed={}", highlight_bridge_src.display());
    let highlight_bridge_obj = obj_dir.join(if is_msvc {
        "duckdb_cli_highlight_bridge.cc.obj"
    } else {
        "duckdb_cli_highlight_bridge.cc.o"
    });
    compile_cpp(
        &highlight_bridge_src,
        &highlight_bridge_obj,
        &include_dirs,
        false,
        &target_os,
        is_msvc,
    );
    objects.push(highlight_bridge_obj);

    archive_static_library(&out_dir, "duckdb_linenoise", &objects, is_msvc);

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=duckdb_linenoise");

    match target_os.as_str() {
        "macos" => println!("cargo:rustc-link-lib=c++"),
        "linux" => println!("cargo:rustc-link-lib=stdc++"),
        "windows" if target_env == "msvc" => {}
        "windows" => println!("cargo:rustc-link-lib=stdc++"),
        other => panic!("unsupported target OS: {other}"),
    }
}
