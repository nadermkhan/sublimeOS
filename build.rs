use std::path::Path;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(has_b43_ucode29_mimo)");
    println!("cargo::rustc-check-cfg=cfg(has_b43_ht0initvals29)");
    println!("cargo::rustc-check-cfg=cfg(has_b43_ht0bsinitvals29)");
    println!("cargo::rustc-check-cfg=cfg(has_b43_ucode16_mimo)");
    println!("cargo::rustc-check-cfg=cfg(has_b43_ucode16)");
    println!("cargo::rustc-check-cfg=cfg(has_b43_pcm16)");
    println!("cargo:rerun-if-changed=firmware/b43");

    let fw_checks = [
        ("firmware/b43/ucode29_mimo.fw", "has_b43_ucode29_mimo"),
        ("firmware/b43/ht0initvals29.fw", "has_b43_ht0initvals29"),
        ("firmware/b43/ht0bsinitvals29.fw", "has_b43_ht0bsinitvals29"),
        ("firmware/b43/ucode16_mimo.fw", "has_b43_ucode16_mimo"),
        ("firmware/b43/ucode16.fw", "has_b43_ucode16"),
        ("firmware/b43/pcm16.fw", "has_b43_pcm16"),
    ];

    for (rel_path, cfg_name) in fw_checks {
        let p = Path::new(rel_path);
        println!("cargo:rerun-if-changed={}", rel_path);
        if let Ok(meta) = std::fs::metadata(p) {
            if meta.is_file() && meta.len() > 8 {
                println!("cargo:rustc-cfg={}", cfg_name);
            }
        }
    }
}
