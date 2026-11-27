use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/raw/libraw_shim.c");
    println!("cargo:rerun-if-changed=src/raw/libraw_shim.h");

    println!("cargo:rerun-if-env-changed=OMASTUDIO_ALLOW_LEGACY_LIBRAW");

    // LibRaw >= 0.22 is required on Omarchy / Arch Linux (and track 0.22.4+ for CVE-2026-135177,
    // `pentax_4shot_load_raw` OOB read). Set OMASTUDIO_ALLOW_LEGACY_LIBRAW=1 only on older LTS
    // development hosts that still ship 0.20/0.21.
    let allow_legacy = env::var("OMASTUDIO_ALLOW_LEGACY_LIBRAW")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let min_libraw = if allow_legacy { "0.20" } else { "0.22" };

    let lib = match pkg_config::Config::new()
        .atleast_version(min_libraw)
        .cargo_metadata(false)
        .probe("libraw_r")
    {
        Ok(l) => l,
        Err(_) => pkg_config::Config::new()
            .atleast_version(min_libraw)
            .cargo_metadata(false)
            .probe("libraw")
            .unwrap_or_else(|e| {
                panic!(
                    "libraw development files (>= {min_libraw}) required: {e} \
                     (on older LTS hosts with LibRaw 0.21, set OMASTUDIO_ALLOW_LEGACY_LIBRAW=1)"
                )
            }),
    };
    if allow_legacy && lib.version.starts_with("0.20.") || allow_legacy && lib.version.starts_with("0.21.") {
        println!(
            "cargo:warning=Building against legacy LibRaw {} (recommended >= 0.22.4 for CVE-2026-135177 and modern camera support)",
            lib.version
        );
    }

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

    let sqlite = pkg_config::Config::new()
        .atleast_version("3.35")
        .cargo_metadata(false)
        .probe("sqlite3")
        .expect("sqlite3 development files required (package: sqlite / libsqlite3-dev)");
    for dir in &sqlite.link_paths {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    for name in &sqlite.libs {
        println!("cargo:rustc-link-lib={}", name);
    }
}
