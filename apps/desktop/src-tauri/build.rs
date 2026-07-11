fn main() {
    #[cfg(all(windows, target_env = "msvc"))]
    compile_test_manifest();

    tauri_build::build()
}

#[cfg(all(windows, target_env = "msvc"))]
fn compile_test_manifest() {
    use std::{env, fs, process::Command};

    let out_dir = env::var("OUT_DIR").expect("Cargo must set OUT_DIR for build scripts");
    let manifest_path = format!(r"{out_dir}\cmux-test.manifest");
    let resource_path = format!(r"{out_dir}\cmux-test-manifest.rc");
    let library_path = format!(r"{out_dir}\cmux_test_manifest.lib");

    fs::write(
        &manifest_path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
</assembly>
"#,
    )
    .expect("failed to write the desktop test manifest");
    fs::write(
        &resource_path,
        format!(r#"1 24 "{}""#, manifest_path.replace('\\', r"\\")),
    )
    .expect("failed to write the desktop test resource script");

    let rc = embed_resource::find_windows_sdk_tool("rc.exe")
        .expect("rc.exe is required to compile the desktop test manifest");
    let status = Command::new(rc)
        .args(["/nologo", "/fo", &library_path, &resource_path])
        .status()
        .expect("failed to run rc.exe for the desktop test manifest");
    assert!(
        status.success(),
        "rc.exe failed to compile the desktop test manifest"
    );

    println!("cargo:rustc-link-search=native={out_dir}");
}
