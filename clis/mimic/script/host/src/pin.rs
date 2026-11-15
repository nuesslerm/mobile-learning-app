//! The pin file of a release: its version, where it is published and the sha256 of each platform's tarball.
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub version: String,
    pub release: String,
    /// `(platform, sha256)` pairs.
    pub platforms: Vec<(String, String)>,
}

impl Pin {
    pub fn parse(text: &str) -> Result<Pin, String> {
        let doc: Value = serde_json::from_str(text).map_err(|e| format!("the pin is not JSON: {e}"))?;
        let text_of = |key: &str| doc[key].as_str().map(String::from).ok_or_else(|| format!("the pin has no `{key}`"));
        let platforms = doc["platforms"].as_object().ok_or("the pin has no `platforms`")?.iter().filter_map(|(name, p)| Some((name.clone(), p["sha256"].as_str()?.to_string()))).collect();
        Ok(Pin { version: text_of("version")?, release: text_of("release")?, platforms })
    }

    pub fn sha256(&self, platform: &str) -> Option<&str> {
        self.platforms.iter().find(|(name, _)| name == platform).map(|(_, sha)| sha.as_str())
    }

    pub fn asset(&self, platform: &str) -> String {
        format!("t3-{}-{platform}.tar.gz", self.version)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// `name -> sha256` of a `SHA256SUMS` file: `<64 hex> [*]<name>` per line.
pub fn parse_sha256_sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (sum, name) = line.trim().split_once(char::is_whitespace)?;
            (sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())).then(|| (name.trim().trim_start_matches('*').to_string(), sum.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_pin_and_a_sums_file_when_parsed_then_the_platform_hash_and_asset_name_are_found() {
        let pin = Pin::parse(r#"{"version":"0.1.9","commit":"c","release":"https://example.org/r","platforms":{"linux-x64":{"bytes":1,"sha256":"aa"}}}"#).unwrap();
        let sums = parse_sha256_sums(&format!("{} *t3-0.1.9-linux-x64.tar.gz\nnot a sum line\n", "a".repeat(64)));

        assert_eq!((pin.sha256("linux-x64"), pin.asset("linux-x64").as_str(), pin.sha256("darwin")), (Some("aa"), "t3-0.1.9-linux-x64.tar.gz", None));
        assert_eq!(sums, [("t3-0.1.9-linux-x64.tar.gz".to_string(), "a".repeat(64))]);
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
