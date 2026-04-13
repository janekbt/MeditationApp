use std::path::Path;
use std::process::Command;

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let ui_out_dir = format!("{out_dir}/ui");
    std::fs::create_dir_all(&ui_out_dir).unwrap();

    // Compile Blueprint (.blp) → GTK UI XML (.ui)
    let blueprint_dir = Path::new("data/ui");
    if blueprint_dir.exists() {
        for entry in std::fs::read_dir(blueprint_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().map_or(false, |e| e == "blp") {
                let stem = path.file_stem().unwrap().to_str().unwrap();
                let output = format!("{ui_out_dir}/{stem}.ui");

                let status = Command::new("blueprint-compiler")
                    .args(["compile", "--output", &output, path.to_str().unwrap()])
                    .status()
                    .expect(
                        "Failed to run blueprint-compiler. \
                         Install it via your distro's package manager or: \
                         pip install blueprint-compiler",
                    );
                assert!(
                    status.success(),
                    "blueprint-compiler failed for {}",
                    path.display()
                );
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    // Compile GResource bundle and embed it in the binary
    // glib-build-tools searches out_dir for the compiled .ui files
    glib_build_tools::compile_resources(
        &[out_dir.as_str(), "data"],
        "data/io.github.janekbt.Meditate.gresource.xml",
        "compiled.gresource",
    );
    println!("cargo:rerun-if-changed=data/io.github.janekbt.Meditate.gresource.xml");

    // Pass build-time config to Rust via env vars (Meson/Flatpak may override these)
    if std::env::var("APP_ID").is_err() {
        println!("cargo:rustc-env=APP_ID=io.github.janekbt.Meditate");
    }
    if std::env::var("PKGDATADIR").is_err() {
        println!("cargo:rustc-env=PKGDATADIR=./data");
    }
    // APP_VERSION is the user-visible version string (e.g. "26.4.1").
    // Meson and Flatpak builds override this via the APP_VERSION env var.
    if std::env::var("APP_VERSION").is_err() {
        println!("cargo:rustc-env=APP_VERSION=26.4.1");
    }
}
