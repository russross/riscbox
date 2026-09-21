use std::env;
use std::path::PathBuf;
use std::process::Command;

fn run(command: &mut Command) {
    let status = command.status().expect("failed to launch C build tool");
    assert!(status.success(), "C build tool failed: {command:?}");
}

fn main() {
    let target = env::var("TARGET").expect("Cargo target is set");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory is set"));
    let sources = ["riscv_cpu.c", "iomem.c", "softfp.c", "bridge.c"];
    let mut objects = Vec::new();

    for source in sources {
        let path = PathBuf::from("tinyemu-core").join(source);
        let object = out.join(source.replace(".c", ".o"));
        let mut compiler = Command::new(env::var_os("CLANG").unwrap_or_else(|| "clang".into()));
        if target == "wasm32-unknown-unknown" {
            compiler.arg("--target=wasm32-unknown-unknown");
            compiler.arg("-mbulk-memory");
            compiler.arg("-Itinyemu-core/wasm-include");
        }
        compiler.args([
            "-std=gnu11",
            "-O3",
            "-ffreestanding",
            "-fno-builtin",
            "-fno-strict-aliasing",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-Itinyemu-core",
            "-c",
        ]);
        compiler.arg(&path).arg("-o").arg(&object);
        run(&mut compiler);
        objects.push(object);
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rerun-if-changed=tinyemu-core");

    let archive = out.join("libtinyemu_core.a");
    let mut archiver = Command::new(env::var_os("AR").unwrap_or_else(|| "ar".into()));
    archiver.arg("crs").arg(&archive).args(&objects);
    run(&mut archiver);
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=tinyemu_core");
}
