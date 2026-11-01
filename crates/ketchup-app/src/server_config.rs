//! Deployment configuration, deliberately separate from saved user preferences.
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

pub const KEY: &str = "KETCHUP_SERVER_URL";

#[cfg(any(not(target_arch = "wasm32"), test))]
pub fn dotenv(contents: &str) -> Result<Option<String>, String> {
    let mut result = None;
    for (index, line) in contents.lines().enumerate() {
        let line = line.trim().trim_start_matches('\u{feff}');
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, raw)) = line.split_once('=') else {
            if line == KEY {
                return Err(format!("Missing '=' for {KEY} on line {}", index + 1));
            }
            continue;
        };
        if key.trim() != KEY {
            continue;
        }
        let raw = raw.trim();
        let value = if raw.starts_with(['\'', '"']) {
            let quote = raw.chars().next().unwrap();
            let end = raw[1..]
                .find(quote)
                .map(|i| i + 1)
                .ok_or_else(|| format!("Unclosed quote for {KEY} on line {}", index + 1))?;
            let trailing = raw[end + 1..].trim();
            if !trailing.is_empty() && !trailing.starts_with('#') {
                return Err(format!("Unexpected text after {KEY} on line {}", index + 1));
            }
            &raw[1..end]
        } else {
            raw.split_once(" #").map_or(raw, |(value, _)| value).trim()
        };
        result = Some(nonempty(value)?);
    }
    Ok(result)
}
#[cfg(not(target_arch = "wasm32"))]
pub fn read_dotenv(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => dotenv(&contents).map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Cannot read {}: {error}", path.display())),
    }
}
pub fn nonempty(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() || value.contains(['\n', '\r']) {
        return Err(format!("{KEY} must contain a server URL"));
    }
    Ok(value.into())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn command_line(args: impl IntoIterator<Item = String>) -> Result<Option<String>, String> {
    let mut args = args.into_iter();
    let mut value = None;
    while let Some(arg) = args.next() {
        if arg == "--server-url" {
            value = Some(nonempty(
                &args
                    .next()
                    .filter(|arg| !arg.starts_with("--"))
                    .ok_or("--server-url requires a URL")?,
            )?);
        } else if let Some(url) = arg.strip_prefix("--server-url=") {
            value = Some(nonempty(url)?);
        }
    }
    Ok(value)
}

pub fn configured_server() -> Result<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(value) = command_line(std::env::args().skip(1))? {
            return Ok(value);
        }
        if let Some(value) = std::env::var_os(KEY) {
            return nonempty(
                &value
                    .into_string()
                    .map_err(|_| format!("{KEY} must be UTF-8"))?,
            );
        }
        if let Some(value) = read_dotenv(Path::new(".env"))? {
            return Ok(value);
        }
    }
    option_env!("KETCHUP_SERVER_URL").map(nonempty).unwrap_or_else(||
        Err("No account server configured. Set KETCHUP_SERVER_URL in .env or at build time, or launch with --server-url URL.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dotenv_supports_plain_quoted_exported_and_missing_values() {
        for source in [
            "KETCHUP_SERVER_URL=https://example.com/",
            "export KETCHUP_SERVER_URL = 'https://example.com/' # comment",
            "KETCHUP_SERVER_URL=\"https://example.com\"",
            "OTHER=value\nKETCHUP_SERVER_URL=https://example.com # comment",
        ] {
            assert_eq!(
                dotenv(source).unwrap().as_deref(),
                Some("https://example.com")
            );
        }
        assert_eq!(dotenv("# comment\nOTHER=value").unwrap(), None);
        for source in [
            "KETCHUP_SERVER_URL=",
            "KETCHUP_SERVER_URL",
            "KETCHUP_SERVER_URL='unclosed",
            "KETCHUP_SERVER_URL='url' extra",
        ] {
            assert!(dotenv(source).is_err());
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn launch_argument_forms_and_errors() {
        for args in [
            vec!["--server-url", "https://example.com/"],
            vec!["--server-url=https://example.com"],
        ] {
            assert_eq!(
                command_line(args.into_iter().map(str::to_owned))
                    .unwrap()
                    .as_deref(),
                Some("https://example.com")
            );
        }
        for args in [
            vec!["--server-url"],
            vec!["--server-url="],
            vec!["--server-url", "--other"],
        ] {
            assert!(command_line(args.into_iter().map(str::to_owned)).is_err());
        }
        assert_eq!(command_line(Vec::<String>::new()).unwrap(), None);
    }
}
