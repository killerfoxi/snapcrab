use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let res_file =
            std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("snapcrab.res");

        let rc_content = format!(
            "1 VERSIONINFO\n\
             FILEVERSION 0,1,0,0\n\
             PRODUCTVERSION 0,1,0,0\n\
             BEGIN\n\
               BLOCK \"StringFileInfo\"\n\
               BEGIN\n\
                 BLOCK \"040904b0\"\n\
                 BEGIN\n\
                   VALUE \"CompanyName\", \"killerfoxi\"\n\
                   VALUE \"FileDescription\", \"SnapCrab Screenshot & Annotation Tool\"\n\
                   VALUE \"LegalCopyright\", \"Copyright (C) 2026 killerfoxi\"\n\
                   VALUE \"ProductName\", \"SnapCrab\"\n\
                 END\n\
               END\n\
               BLOCK \"VarFileInfo\"\n\
               BEGIN\n\
                 VALUE \"Translation\", 0x409, 1200\n\
               END\n\
             END\n\
             1 ICON \"assets/snapcrab.ico\"\n\
             1 24 \"snapcrab.exe.manifest\""
        );

        let temp_rc = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("generated.rc");
        std::fs::write(&temp_rc, rc_content).unwrap();

        let output = Command::new("llvm-rc")
            .arg("-no-preprocess")
            .arg(format!("/fo{}", res_file.display()))
            .arg(&temp_rc)
            .output()
            .expect("Failed to execute llvm-rc");

        if !output.status.success() {
            panic!(
                "llvm-rc failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        println!("cargo:rustc-link-arg={}", res_file.display());
        println!("cargo:rerun-if-changed=assets/snapcrab.ico");
        println!("cargo:rerun-if-changed=snapcrab.exe.manifest");
    }
}
