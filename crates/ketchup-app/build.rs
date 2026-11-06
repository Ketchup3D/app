#[allow(dead_code)]
#[path = "src/server_config.rs"]
mod server_config;

fn main() {
    println!("cargo:rerun-if-env-changed=KETCHUP_UPDATE_REPOSITORY");
    println!("cargo:rerun-if-env-changed=GITHUB_REPOSITORY");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repository = std::env::var("KETCHUP_UPDATE_REPOSITORY")
        .or_else(|_| std::env::var("GITHUB_REPOSITORY"))
        .ok()
        .or_else(|| {
            let output = std::process::Command::new("git")
                .args(["config", "--get", "remote.origin.url"])
                .current_dir(&root).output().ok()?;
            output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        })
        .unwrap_or_else(|| "Ketchup3D/app".to_owned());
    let repository = repository.trim().trim_end_matches('/').trim_end_matches(".git");
    let repository = repository.strip_prefix("https://github.com/")
        .or_else(|| repository.strip_prefix("git@github.com:"))
        .or_else(|| repository.strip_prefix("ssh://git@github.com/"))
        .unwrap_or(repository);
    let parts: Vec<_> = repository.split('/').collect();
    assert!(parts.len() == 2 && parts.iter().all(|part|
        !part.is_empty() && *part != "." && *part != ".."
            && part.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))),
        "Set KETCHUP_UPDATE_REPOSITORY to the GitHub owner/repository for desktop updates");
    println!("cargo:rustc-env=KETCHUP_UPDATE_REPOSITORY_URL=https://github.com/{repository}");
    // Re-evaluate local fork ownership if the origin is changed.
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--git-path", "config"])
        .current_dir(&root).output()
    {
        if output.status.success() {
            let config = String::from_utf8_lossy(&output.stdout);
            println!("cargo:rerun-if-changed={}", root.join(config.trim()).display());
        }
    }
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
