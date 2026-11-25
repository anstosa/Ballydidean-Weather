//! Build only the explicitly selected candidate predecessor compression library.

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

const COMPRESSOR_SOURCES: &[&str] = &[
    "adler32.c",
    "crc32.c",
    "deflate.c",
    "trees.c",
    "zutil.c",
    "cpu_features.c",
    "weather_node_gzip.c",
];

/// Disable candidate compilation for ordinary builds and fail closed on unsupported targets.
fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_NODE_ZLIB_CANDIDATE");
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    println!("cargo:rerun-if-changed=vendor/node-zlib-24.16.0");
    // compile no C code for the default non-candidate runtime
    if env::var_os("CARGO_FEATURE_NODE_ZLIB_CANDIDATE").is_none() {
        return;
    }
    let target = env::var("TARGET").expect("candidate target");
    let host = env::var("HOST").expect("candidate build host");
    assert!(
        target == host
            && matches!(
                target.as_str(),
                "x86_64-unknown-linux-gnu" | "aarch64-unknown-linux-gnu"
            ),
        "candidate gzip requires the native Linux x86_64 or ARM64 target"
    );
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("candidate source root"))
        .join("vendor/node-zlib-24.16.0");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("candidate output root"));
    build_archive(&root, &output);
}

/// Compile the pinned prefixed scalar compressor using the installed C toolchain.
fn build_archive(root: &Path, output: &Path) {
    let cc = env::var_os("CC").unwrap_or_else(|| "cc".into());
    let ar = env::var_os("AR").unwrap_or_else(|| "ar".into());
    let mut objects = Vec::with_capacity(COMPRESSOR_SOURCES.len());
    // compile only the narrow checked-in compression subset
    for source in COMPRESSOR_SOURCES {
        let object = output.join(Path::new(source).with_extension("o"));
        let status = Command::new(&cc)
            .args([
                "-O2",
                "-fPIC",
                "-DCPU_NO_SIMD",
                "-DHAVE_HIDDEN",
                "-DZ_PREFIX",
            ])
            .arg("-I")
            .arg(root)
            .arg("-c")
            .arg(root.join(source))
            .arg("-o")
            .arg(&object)
            .status()
            .expect("run candidate C compiler");
        assert!(status.success(), "candidate C compilation failed: {source}");
        objects.push(object);
    }
    let archive = output.join("libweather_node_gzip.a");
    let status = Command::new(ar)
        .arg("crsD")
        .arg(&archive)
        .args(objects)
        .status()
        .expect("run candidate archive tool");
    assert!(status.success(), "candidate C archive creation failed");
    println!("cargo:rustc-link-search=native={}", output.display());
    println!("cargo:rustc-link-lib=static=weather_node_gzip");
}
