fn main() {
    // Web imports use the vendored JS reader in a worker, without C/C++ tools.
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        return;
    }
    let base = "vendor/skp/native";
    println!("cargo:rerun-if-changed={base}");
    println!("cargo:rerun-if-changed=src/skp_native.cpp");
    let mut reader = cc::Build::new();
    reader
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .opt_level(2)
        .include(format!("{base}/include"))
        .include(base)
        .include(format!("{base}/miniz"))
        .define("OPENSKP_STATIC_DEFINE", None)
        .file("src/skp_native.cpp");
    if reader.get_compiler().is_like_msvc() {
        reader.flag("/EHsc").define("_CRT_SECURE_NO_WARNINGS", None);
    }
    for name in [
        "core",
        "errors",
        "geometry",
        "json_export",
        "legacy",
        "model",
        "observability",
        "pages_dimensions",
        "tlv",
        "transforms",
        "vff",
    ] {
        reader.file(format!("{base}/{name}.cpp"));
    }
    reader.compile("ketchup_skp");
    let mut zip = cc::Build::new();
    zip.warnings(false)
        .opt_level(2)
        .include(format!("{base}/miniz"));
    for name in ["miniz", "miniz_tdef", "miniz_tinfl", "miniz_zip"] {
        zip.file(format!("{base}/miniz/{name}.c"));
    }
    zip.compile("ketchup_skp_zip");
}
