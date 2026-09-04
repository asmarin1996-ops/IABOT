use cmake;
use std::{env, path::PathBuf};

const PROFILE: &str = "Release";

fn main() {
    let mut header_filename = "wrapper.h";
    let mut binding_dst = "src/bindings.rs";

    let mut project = format!(
        "{}/libpiper/",
        env::var("CARGO_MANIFEST_DIR").expect("Unable to find manifest dir")
    );

    if cfg!(feature = "cuda") {
        project = format!(
            "{}/libpiper-cuda/",
            env::var("CARGO_MANIFEST_DIR").expect("Unable to find manifest dir")
        );

        header_filename = "wrapper_cuda.h";
        binding_dst = "src/bindings_cuda.rs";
    }

    let mut cfg = cmake::Config::new(&project);
    cfg.define("CMAKE_INSTALL_PREFIX", format!("{}/install", project));
    cfg.profile(PROFILE);

    let dst: PathBuf = cfg.build();

    // Finding libs for CUDA feature
    if cfg!(feature = "cuda") {
        println!("cargo:rerun-if-env-changed=CUDA_PATH");

        for lib_dir in find_cuda_helper::find_cuda_lib_dirs() {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
        }

        if cfg!(target_os = "windows") {
            println!("cargo:rustc-link-lib=cudart");
            println!("cargo:rustc-link-lib=cublas");
            println!("cargo:rustc-link-lib=cublasLt");
        } else {
            println!("cargo:rustc-link-lib=static=cudart_static");
            println!("cargo:rustc-link-lib=static=cublas_static");
            println!("cargo:rustc-link-lib=static=cublasLt_static");
            println!("cargo:rustc-link-lib=static=culibos");
        }
    }

    let include_dir = dst.join("build");
    println!("cargo:rustc-link-search=native={}", include_dir.display());
    println!("cargo:rustc-link-search=native={}", include_dir.join("Release").display());
    println!("cargo:rustc-link-search=native={}", include_dir.join("Debug").display());
    println!("cargo:rustc-link-lib=piper");

    let out_path =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("Unable to find out manifest dir"));

    bindgen::Builder::default()
        .header(header_filename)
        .clang_arg(format!("-I{}", include_dir.display()))
        .generate()
        .expect("Bindgen failed")
        .write_to_file(out_path.join(binding_dst))
        .expect("Couldn't write bindings");
}
