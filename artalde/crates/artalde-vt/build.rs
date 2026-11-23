//! Builds `libghostty-vt` from the pinned Ghostty source with Zig and
//! generates Rust bindings from its C headers. See `docs/terminals.md`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const SUBMODULE_HINT: &str = "run `git submodule update --init --depth 1 artalde/vendor/ghostty` \
     (or point GHOSTTY_SRC at a Ghostty checkout)";
const ZIG_SERIES: &str = "0.16.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("artalde-vt build: {message}");
            ExitCode::FAILURE
        }
    }
}

fn var(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is not set"))
}

fn tracked_env(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={name}");
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn run() -> Result<(), String> {
    let manifest = PathBuf::from(var("CARGO_MANIFEST_DIR")?);
    let out = PathBuf::from(var("OUT_DIR")?);
    let target = var("TARGET")?;
    let target_os = var("CARGO_CFG_TARGET_OS")?;
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    let src = source_dir(&manifest)?;
    // Everything the Zig build reads. `pkg` holds Ghostty's vendored C deps.
    for path in ["src", "pkg", "include", "build.zig", "build.zig.zon"] {
        println!("cargo:rerun-if-changed={}", src.join(path).display());
    }

    let zig = tracked_env("ZIG").unwrap_or_else(|| "zig".to_owned());
    let (archive_name, linked_name) = if target_env == "msvc" {
        ("ghostty-vt-static.lib", "ghostty-vt.lib")
    } else {
        ("libghostty-vt.a", "libghostty-vt.a")
    };

    let archive = match tracked_env("GHOSTTY_VT_LIB_DIR") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            println!("cargo:rerun-if-changed={}", dir.display());
            [linked_name, archive_name]
                .iter()
                .map(|name| dir.join(name))
                .find(|path| path.is_file())
                .ok_or_else(|| {
                    format!("GHOSTTY_VT_LIB_DIR={} has no {linked_name}", dir.display())
                })?
        }
        None => {
            zig_build(&zig, &src, &out, &target)?;
            out.join("zig-out/lib").join(archive_name)
        }
    };

    // Only the static archive goes in the search path, so the linker can
    // never pick the shared library that Zig installs next to it.
    let lib = out.join("lib");
    std::fs::create_dir_all(&lib).map_err(|e| format!("create {}: {e}", lib.display()))?;
    std::fs::copy(&archive, lib.join(linked_name))
        .map_err(|e| format!("copy {}: {e}", archive.display()))?;
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");

    bindings(&zig, &src, &out, &target_os)
}

fn source_dir(manifest: &Path) -> Result<PathBuf, String> {
    let configured = tracked_env("GHOSTTY_SRC").map(PathBuf::from);
    let dir = configured.unwrap_or_else(|| manifest.join("../../vendor/ghostty"));
    if !dir.join("build.zig").is_file() || !dir.join("include/ghostty/vt.h").is_file() {
        return Err(format!(
            "no Ghostty source at {}: {SUBMODULE_HINT}",
            dir.display()
        ));
    }
    let dir = dir
        .canonicalize()
        .map_err(|e| format!("resolve {}: {e}", dir.display()))?;
    // Windows' canonical form is `\\?\C:\...`, which clang cannot resolve
    // includes against.
    Ok(PathBuf::from(
        dir.to_string_lossy().trim_start_matches(r"\\?\").to_owned(),
    ))
}

/// Zig builds for the machine it runs on unless told otherwise: that CPU's
/// newest instructions (which crash older CPUs) and never another target.
fn zig_target(target: &str) -> Result<&'static str, String> {
    Ok(match target {
        "aarch64-apple-darwin" => "aarch64-macos",
        "x86_64-apple-darwin" => "x86_64-macos",
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        "aarch64-unknown-linux-musl" => "aarch64-linux-musl",
        "x86_64-unknown-linux-musl" => "x86_64-linux-musl",
        "aarch64-pc-windows-msvc" => "aarch64-windows-msvc",
        "x86_64-pc-windows-msvc" => "x86_64-windows-msvc",
        other => {
            return Err(format!(
                "no Zig target known for {other}; build libghostty-vt yourself and set \
                 GHOSTTY_VT_LIB_DIR"
            ));
        }
    })
}

fn check_zig(zig: &str) -> Result<(), String> {
    let output = Command::new(zig).arg("version").output().map_err(|e| {
        format!(
            "cannot run `{zig}` ({e}). libghostty-vt needs Zig {ZIG_SERIES}x: install it, set \
             ZIG to its path, or set GHOSTTY_VT_LIB_DIR to a directory with a prebuilt \
             libghostty-vt.a"
        )
    })?;
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || !version.starts_with(ZIG_SERIES) {
        return Err(format!(
            "`{zig} version` reports `{version}`, but Ghostty at this pin builds only with Zig \
             {ZIG_SERIES}x. Set ZIG to a {ZIG_SERIES}x binary, or GHOSTTY_VT_LIB_DIR to a \
             prebuilt libghostty-vt.a"
        ));
    }
    Ok(())
}

fn zig_build(zig: &str, src: &Path, out: &Path, target: &str) -> Result<(), String> {
    check_zig(zig)?;
    let status = Command::new(zig)
        .current_dir(src)
        // Ghostty builds the xcframework whenever it finds xcodebuild, and
        // with only the Command Line Tools that binary exists but fails.
        .args([
            "build",
            "-Demit-lib-vt",
            "-Demit-xcframework=false",
            "-Doptimize=ReleaseFast",
        ])
        .arg(format!("-Dtarget={}", zig_target(target)?))
        .arg("--prefix")
        .arg(out.join("zig-out"))
        .arg("--cache-dir")
        .arg(out.join("zig-cache"))
        .status()
        .map_err(|e| format!("cannot run `{zig} build`: {e}"))?;
    if !status.success() {
        return Err(format!(
            "`{zig} build -Demit-lib-vt` failed in {} ({status}). If a dependency download \
             failed, see \"Building the emulator\" in docs/terminals.md",
            src.display()
        ));
    }
    Ok(())
}

fn bindings(zig: &str, src: &Path, out: &Path, target_os: &str) -> Result<(), String> {
    let include = src.join("include");
    let mut builder = bindgen::Builder::default()
        .header(include.join("ghostty/vt.h").to_string_lossy())
        .clang_arg(format!("-I{}", include.display()));
    if target_os == "macos" {
        // Apple's libclang has its builtin headers but needs the SDK for the
        // C library.
        let sdk = tracked_env("SDKROOT").or_else(|| {
            let output = Command::new("xcrun").arg("--show-sdk-path").output().ok()?;
            Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
                .filter(|path| !path.is_empty())
        });
        if let Some(sdk) = sdk {
            builder = builder.clang_args(["-isysroot".to_owned(), sdk]);
        }
    } else if Command::new("clang").arg("--version").output().is_err() {
        // A bare libclang lacks its builtin headers; Zig ships a set. With a
        // clang installed, bindgen asks it for its own, which always match.
        if let Some(lib_dir) = zig_lib_dir(zig) {
            builder = builder.clang_arg(format!("-isystem{lib_dir}/include"));
        }
    }
    builder
        .allowlist_function("ghostty_.*")
        .allowlist_type("Ghostty.*")
        .allowlist_var("GHOSTTY_.*")
        .prepend_enum_name(false)
        .derive_default(true)
        .layout_tests(false)
        .generate()
        .map_err(|e| format!("bindgen failed on ghostty/vt.h: {e}"))?
        .write_to_file(out.join("bindings.rs"))
        .map_err(|e| format!("write bindings.rs: {e}"))
}

fn zig_lib_dir(zig: &str) -> Option<String> {
    let output = Command::new(zig).arg("env").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.lines()
        .find_map(|line| line.trim().strip_prefix(".lib_dir = \""))
        .and_then(|rest| rest.strip_suffix("\","))
        .map(str::to_owned)
}
