//! Port of `ChecksumVerifier`: SHA256/384/512, SHA1, MD5; normalize-then-
//! compare (strip spaces/dashes/colons, uppercase).
//!
//! The C# original never ran this: vendors.json defined no checksum field
//! (bug B2). It runs now — a vendor may pin a digest in vendors.json, and
//! resolvers additionally carry one from upstream for sources that publish
//! it (see `vendors::installer::resolved_checksum` for the precedence).

use std::io::Read;
use std::path::Path;

use crate::digest::{Digest, Md5, Sha1, Sha256, Sha384, Sha512};

/// `ChecksumInfo` from the vendor model.
#[derive(Debug, Clone, Default)]
pub struct ChecksumInfo {
    /// `SHA256` (default), `SHA512`, `SHA384`, `SHA1`, or `MD5`.
    pub algorithm: String,
    /// Expected hex value.
    pub value: String,
    /// When true a mismatch blocks installation; otherwise it only warns.
    pub required: bool,
}

#[derive(Debug, Default)]
pub struct VerificationResult {
    pub success: bool,
    pub skipped: bool,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub message: Option<String>,
}

/// `ChecksumVerifier.Verify`.
pub fn verify(file_path: &Path, info: &ChecksumInfo) -> VerificationResult {
    if !file_path.is_file() {
        return VerificationResult {
            success: false,
            message: Some(format!("File not found: {}", file_path.display())),
            ..Default::default()
        };
    }
    if info.value.is_empty() {
        return VerificationResult {
            success: true,
            skipped: true,
            message: Some("No checksum provided, skipping verification".into()),
            ..Default::default()
        };
    }

    match compute(file_path, &info.algorithm) {
        Ok(actual) => {
            let expected = normalize(&info.value);
            let actual = normalize(&actual);
            let matches = expected.eq_ignore_ascii_case(&actual);
            VerificationResult {
                success: matches,
                skipped: false,
                expected: Some(expected),
                actual: Some(actual),
                message: None,
            }
        }
        Err(e) => VerificationResult {
            success: false,
            message: Some(format!("Checksum verification failed: {e}")),
            ..Default::default()
        },
    }
}

/// `ComputeChecksum`: uppercase hex, streaming.
pub fn compute(file_path: &Path, algorithm: &str) -> Result<String, String> {
    let mut file = std::fs::File::open(file_path).map_err(|e| e.to_string())?;

    fn hash_stream<D: Digest>(file: &mut std::fs::File) -> Result<Vec<u8>, String> {
        let mut hasher = D::default();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(hasher.finalize())
    }

    let bytes = match algorithm.to_uppercase().as_str() {
        "SHA256" => hash_stream::<Sha256>(&mut file)?,
        "SHA512" => hash_stream::<Sha512>(&mut file)?,
        "SHA384" => hash_stream::<Sha384>(&mut file)?,
        "SHA1" => hash_stream::<Sha1>(&mut file)?,
        "MD5" => hash_stream::<Md5>(&mut file)?,
        other => {
            return Err(format!(
                "Unsupported hash algorithm: {other}. Supported: SHA256, SHA512, SHA384, SHA1, MD5"
            ));
        }
    };

    Ok(bytes.iter().map(|b| format!("{b:02X}")).collect())
}

/// `NormalizeChecksum`: strip ` `, `-`, `:`; uppercase.
fn normalize(checksum: &str) -> String {
    checksum
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | ':'))
        .collect::<String>()
        .to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(content: &[u8]) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f
    }

    #[test]
    fn sha256_of_known_content() {
        let f = temp_file(b"hello");
        // Well-known SHA-256 of "hello".
        assert_eq!(
            compute(f.path(), "sha256").unwrap(),
            "2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824"
        );
    }

    #[test]
    fn verify_normalizes_punctuation_and_case() {
        let f = temp_file(b"hello");
        let info = ChecksumInfo {
            algorithm: "SHA256".into(),
            value: "2c:f2 4d-ba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
            required: true,
        };
        assert!(verify(f.path(), &info).success);
    }

    #[test]
    fn empty_value_skips() {
        let f = temp_file(b"x");
        let result = verify(
            f.path(),
            &ChecksumInfo {
                algorithm: "SHA256".into(),
                value: String::new(),
                required: false,
            },
        );
        assert!(result.success && result.skipped);
    }

    #[test]
    fn mismatch_fails_with_both_values() {
        let f = temp_file(b"hello");
        let result = verify(
            f.path(),
            &ChecksumInfo {
                algorithm: "MD5".into(),
                value: "00000000000000000000000000000000".into(),
                required: true,
            },
        );
        assert!(!result.success);
        assert_eq!(result.actual.unwrap(), "5D41402ABC4B2A76B9719D911017C592");
    }

    #[test]
    fn unknown_algorithm_errors() {
        let f = temp_file(b"x");
        assert!(
            compute(f.path(), "CRC32")
                .unwrap_err()
                .contains("Unsupported")
        );
    }
}
