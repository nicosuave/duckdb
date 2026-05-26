use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ENV");
    println!("cargo:rerun-if-env-changed=CXX");
    println!("cargo:rerun-if-env-changed=AR");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let obj_dir = out_dir.join("obj");
    let _ = std::fs::create_dir_all(&obj_dir);

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let workspace_root = manifest_dir.join("../..");

    let include_dir = workspace_root.join("third_party/utf8proc/include");
    let utf8proc_src = workspace_root.join("third_party/utf8proc/utf8proc.cpp");
    let wrapper_src = manifest_dir.join("src/render_width.cpp");
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let is_msvc = target_os == "windows" && target_env == "msvc";

    println!("cargo:rerun-if-changed={}", utf8proc_src.display());
    println!("cargo:rerun-if-changed={}", wrapper_src.display());

    let sources = [utf8proc_src, wrapper_src];
    let mut objects: Vec<PathBuf> = Vec::new();
    for src in sources {
        let file_name = src.file_name().and_then(|s| s.to_str()).unwrap_or("object");
        let obj_path = obj_dir.join(if is_msvc {
            format!("{file_name}.obj")
        } else {
            format!("{file_name}.o")
        });

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
                .arg(format!("/I{}", include_dir.display()))
                .arg("/c")
                .arg(&src)
                .arg(format!("/Fo{}", obj_path.display()));
        } else {
            cxx.arg("-std=c++17")
                .arg(format!("-I{}", include_dir.display()))
                .arg("-c")
                .arg(&src)
                .arg("-o")
                .arg(&obj_path);
            if target_os == "macos" {
                cxx.arg("-mmacosx-version-min=11.0");
            }
            if target_os == "linux" || target_os == "windows" {
                cxx.arg("-fPIC");
            }
        }

        let status = cxx.status().expect("failed to invoke c++");
        if !status.success() {
            panic!("c++ failed with status {status}");
        }
        objects.push(obj_path);
    }

    let lib_path = out_dir.join(if is_msvc {
        "duckdb_render_width.lib"
    } else {
        "libduckdb_render_width.a"
    });
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
    for obj in &objects {
        ar.arg(obj);
    }
    let status = ar.status().expect("failed to invoke ar");
    if !status.success() {
        panic!("ar failed with status {status}");
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=duckdb_render_width");

    match target_os.as_str() {
        "macos" => println!("cargo:rustc-link-lib=c++"),
        "linux" => println!("cargo:rustc-link-lib=stdc++"),
        "windows" if target_env == "msvc" => {}
        "windows" => println!("cargo:rustc-link-lib=stdc++"),
        other => panic!("unsupported target OS: {other}"),
    }
}
