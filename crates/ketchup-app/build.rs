#[allow(dead_code)]
#[path = "src/server_config.rs"]
mod server_config;

fn main() {
    println!("cargo:rerun-if-env-changed=KETCHUP_SERVER_URL");
    println!("cargo:rerun-if-changed=src/server_config.rs");
    let dotenv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    println!("cargo:rerun-if-changed={}", dotenv.display());
    let server = match std::env::var("KETCHUP_SERVER_URL") {
        Ok(value) => Some(server_config::nonempty(&value).expect("Invalid KETCHUP_SERVER_URL")),
        Err(std::env::VarError::NotPresent) => {
            server_config::read_dotenv(&dotenv).expect("Invalid server .env")
        }
        Err(error) => panic!("Invalid KETCHUP_SERVER_URL: {error}"),
    };
    if let Some(server) = server {
        println!("cargo:rustc-env=KETCHUP_SERVER_URL={server}");
    }

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
