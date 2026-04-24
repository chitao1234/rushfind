use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/platform/locale_solarish_regex.c");
    println!(
        "cargo:rustc-env=RUSHFIND_BUILD_VERSION={}",
        std::env::var("CARGO_PKG_VERSION").unwrap()
    );
    println!(
        "cargo:rustc-env=RUSHFIND_BUILD_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    println!(
        "cargo:rustc-env=RUSHFIND_BUILD_GIT_HASH={}",
        read_git_hash().unwrap_or_else(|| "unknown".to_string())
    );

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if matches!(target_os.as_str(), "solaris" | "illumos") {
        cc::Build::new()
            .file("src/platform/locale_solarish_regex.c")
            .compile("rushfind_solarish_regex");
    }
}

fn read_git_hash() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
