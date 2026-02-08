fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let _ = embed_resource::compile("snapcrab-manifest.rc", embed_resource::NONE);
    }
}
