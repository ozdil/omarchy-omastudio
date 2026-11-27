use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/raw/libraw_shim.c");
    println!("cargo:rerun-if-changed=src/raw/libraw_shim.h");

    let lib = match pkg_config::Config::new()
        .atleast_version("0.20")
        .cargo_metadata(false)
        .probe("libraw_r")
    {
        Ok(l) => l,
        Err(_) => pkg_config::Config::new()
            .atleast_version("0.20")
            .cargo_metadata(false)
            .probe("libraw")
            .expect("libraw development files required"),
    };

    let mut build = cc::Build::new();
    build.file("src/raw/libraw_shim.c");
    build.include("src/raw");

    for inc in &lib.include_paths {
        build.include(inc);
    }

    build.compile("omaraw_shim");

    let out_dir = env::var("OUT_DIR").unwrap();
    println!("cargo:rustc-link-search=native={}", out_dir);
    for dir in &lib.link_paths {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    println!("cargo:rustc-link-lib=static=omaraw_shim");
    let mut has_stdcpp = false;
    for name in &lib.libs {
        if name == "stdc++" {
            has_stdcpp = true;
        }
        println!("cargo:rustc-link-lib={}", name);
    }
    if !has_stdcpp {
        println!("cargo:rustc-link-lib=stdc++");
    }
}
