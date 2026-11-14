//! AWS Signature Version 4 header signing, hand-rolled on `sha2` and `hmac` for the S3 API of any provider.
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

pub struct Credentials<'a> {
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub region: &'a str,
    pub service: &'a str,
}

/// What is signed. `path` is the already percent-encoded URI path, `query` the canonical query string, and `headers` the
/// headers to sign (at least `host` and `x-amz-date`), in any order and case.
pub struct Parts<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub headers: &'a [(String, String)],
    pub payload_hash: &'a str,
    pub amz_date: &'a str,
}

pub fn signed_header_names(headers: &[(String, String)]) -> String {
    let mut names: Vec<String> = headers.iter().map(|(name, _)| name.to_ascii_lowercase()).collect();
    names.sort();
    names.join(";")
}

fn canonical_request(parts: &Parts) -> String {
    let mut headers: Vec<(String, String)> = parts.headers.iter().map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_string())).collect();
    headers.sort();
    let block: String = headers.iter().map(|(name, value)| format!("{name}:{value}\n")).collect();
    format!("{}\n{}\n{}\n{block}\n{}\n{}", parts.method, parts.path, parts.query, signed_header_names(parts.headers), parts.payload_hash)
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn scope(credentials: &Credentials, amz_date: &str) -> String {
    format!("{}/{}/{}/aws4_request", &amz_date[..8], credentials.region, credentials.service)
}

/// The lowercase hex signature of `parts`.
pub fn signature(credentials: &Credentials, parts: &Parts) -> String {
    let scope = scope(credentials, parts.amz_date);
    let to_sign = format!("AWS4-HMAC-SHA256\n{}\n{scope}\n{}", parts.amz_date, hex(&Sha256::digest(canonical_request(parts).as_bytes())));
    let mut key = hmac(format!("AWS4{}", credentials.secret_key).as_bytes(), &parts.amz_date[..8]);
    for part in [credentials.region, credentials.service, "aws4_request"] {
        key = hmac(&key, part);
    }
    hex(&hmac(&key, &to_sign))
}

/// The value of the `Authorization` header.
pub fn authorization(credentials: &Credentials, parts: &Parts) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        credentials.access_key,
        scope(credentials, parts.amz_date),
        signed_header_names(parts.headers),
        signature(credentials, parts)
    )
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn sha256_file_hex(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            return Ok(hex(&hasher.finalize()));
        }
        hasher.update(&chunk[..read]);
    }
}

/// Percent-encodes a path for the canonical URI, keeping `/` and the unreserved characters.
pub fn encode_path(path: &str) -> String {
    path.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// `YYYYMMDDTHHMMSSZ` for `now`, UTC.
pub fn amz_date(now: SystemTime) -> String {
    let seconds = now.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // civil-from-days (H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    // The AWS Signature V4 test suite, case `get-vanilla`.
    fn vanilla() -> (Credentials<'static>, Vec<(String, String)>) {
        let credentials = Credentials { access_key: "AKIDEXAMPLE", secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY", region: "us-east-1", service: "service" };
        let headers = vec![("Host".to_string(), "example.amazonaws.com".to_string()), ("X-Amz-Date".to_string(), "20150830T123600Z".to_string())];
        (credentials, headers)
    }

    #[test]
    fn given_the_published_get_vanilla_case_when_signed_then_the_canonical_request_and_signature_match_the_documented_values() {
        // Given the documented request
        let (credentials, headers) = vanilla();
        let parts = Parts { method: "GET", path: "/", query: "", headers: &headers, payload_hash: EMPTY_SHA256, amz_date: "20150830T123600Z" };

        // When it is signed, Then the canonical request hashes to the documented value and the signature and header match
        assert_eq!(sha256_hex(canonical_request(&parts).as_bytes()), "bb579772317eb040ac9ed261061d46c1f17a8133879d6129b6e1c25292927e63");
        assert_eq!(signature(&credentials, &parts), "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31");
        assert_eq!(
            authorization(&credentials, &parts),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    #[test]
    fn given_a_unix_time_when_formatted_then_it_is_the_utc_amz_date() {
        assert_eq!(amz_date(UNIX_EPOCH), "19700101T000000Z");
        assert_eq!(amz_date(UNIX_EPOCH + Duration::from_secs(1_440_938_160)), "20150830T123600Z");
        assert_eq!(amz_date(UNIX_EPOCH + Duration::from_secs(1_709_164_800 + 86_399)), "20240229T235959Z");
    }

    #[test]
    fn given_a_key_with_reserved_characters_when_encoded_then_slashes_stay_and_the_rest_is_percent_encoded() {
        assert_eq!(encode_path("a b/c+d/e.f-g_h~"), "a%20b/c%2Bd/e.f-g_h~");
    }
}
