//! Optional Xweather credential loading through an injected file boundary.

use std::fs;
use std::io;
use std::path::Path;

const DEFAULT_CLIENT_ID_PATH: &str = "/run/secrets/weather_xweather_client_id";
const DEFAULT_CLIENT_SECRET_PATH: &str = "/run/secrets/weather_xweather_client_secret";
const MINIMUM_CREDENTIAL_UTF16_UNITS: usize = 4;
const MAXIMUM_CREDENTIAL_UTF16_UNITS: usize = 256;

/// Configured path strings retain nullish fallback and present-empty behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeXweatherCredentialPaths<'a> {
    pub client_id_file: Option<&'a str>,
    pub client_secret_file: Option<&'a str>,
}

impl EdgeXweatherCredentialPaths<'_> {
    /// Construct the exact default secret-file lookup.
    pub const fn defaults() -> Self {
        Self {
            client_id_file: None,
            client_secret_file: None,
        }
    }
}

/// Optional byte custody that distinguishes an absent file from every other read failure.
pub trait EdgeXweatherCredentialByteReader {
    type Error;

    /// Read one complete configured file, returning `None` only for an absent file.
    fn read_optional(&mut self, path: &Path) -> Result<Option<Vec<u8>>, Self::Error>;
}

/// A complete credential pair; deliberately no `Debug` implementation exposes its values.
#[derive(Clone, PartialEq, Eq)]
pub struct EdgeXweatherCredentials {
    client_id: String,
    client_secret: String,
}

impl EdgeXweatherCredentials {
    /// Borrow the normalized provider client identifier.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Borrow the normalized provider client secret.
    pub fn client_secret(&self) -> &str {
        &self.client_secret
    }
}

/// Credential loading failures identify only the owning phase and never the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeXweatherCredentialsError<Error> {
    ClientIdRead(Error),
    ClientIdInvalid,
    ClientSecretRead(Error),
    ClientSecretInvalid,
    PartialProvisioning,
}

enum OptionalCredentialError<Error> {
    Read(Error),
    Invalid,
}

/// Read the identifier and then secret with the exact default and partial-state behavior.
pub fn load_edge_xweather_credentials<Reader: EdgeXweatherCredentialByteReader>(
    paths: EdgeXweatherCredentialPaths<'_>,
    reader: &mut Reader,
) -> Result<Option<EdgeXweatherCredentials>, EdgeXweatherCredentialsError<Reader::Error>> {
    let client_id_path = Path::new(paths.client_id_file.unwrap_or(DEFAULT_CLIENT_ID_PATH));
    let client_id = read_optional_credential(reader, client_id_path).map_err(|error| {
        // Keep identifier read and content failures distinct without carrying bytes.
        match error {
            OptionalCredentialError::Read(error) => {
                EdgeXweatherCredentialsError::ClientIdRead(error)
            }
            OptionalCredentialError::Invalid => EdgeXweatherCredentialsError::ClientIdInvalid,
        }
    })?;
    let client_secret_path = Path::new(
        paths
            .client_secret_file
            .unwrap_or(DEFAULT_CLIENT_SECRET_PATH),
    );
    let client_secret = read_optional_credential(reader, client_secret_path).map_err(|error| {
        // Keep secret read and content failures distinct without carrying bytes.
        match error {
            OptionalCredentialError::Read(error) => {
                EdgeXweatherCredentialsError::ClientSecretRead(error)
            }
            OptionalCredentialError::Invalid => EdgeXweatherCredentialsError::ClientSecretInvalid,
        }
    })?;

    match (client_id, client_secret) {
        (None, None) => Ok(None),
        (Some(client_id), Some(client_secret)) => Ok(Some(EdgeXweatherCredentials {
            client_id,
            client_secret,
        })),
        (None, Some(_)) | (Some(_), None) => Err(EdgeXweatherCredentialsError::PartialProvisioning),
    }
}

/// Read and normalize one optional credential while retaining absence independently.
fn read_optional_credential<Reader: EdgeXweatherCredentialByteReader>(
    reader: &mut Reader,
    path: &Path,
) -> Result<Option<String>, OptionalCredentialError<Reader::Error>> {
    let bytes = reader
        .read_optional(path)
        .map_err(OptionalCredentialError::Read)?;
    bytes
        .map(|bytes| normalize_credential(&bytes).ok_or(OptionalCredentialError::Invalid))
        .transpose()
}

/// Decode like the Node UTF-8 string reader, trim ECMAScript whitespace, and retain its bounds.
fn normalize_credential(bytes: &[u8]) -> Option<String> {
    let decoded = String::from_utf8_lossy(bytes);
    let value = decoded.trim_matches(javascript_whitespace);
    let utf16_units = value.encode_utf16().count();
    // Enforce the source's post-trim JavaScript string-length interval.
    if !(MINIMUM_CREDENTIAL_UTF16_UNITS..=MAXIMUM_CREDENTIAL_UTF16_UNITS).contains(&utf16_units) {
        return None;
    }
    // Reject the exact whitespace and ASCII control population used by the source expression.
    if value.chars().any(|character| {
        javascript_whitespace(character)
            || ('\u{0000}'..='\u{001f}').contains(&character)
            || character == '\u{007f}'
    }) {
        return None;
    }
    Some(value.to_owned())
}

/// Match the ECMAScript whitespace and line-terminator set used by `trim` and `\s`.
const fn javascript_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'..='\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// Native optional file reader matching the source's absent-only disable rule.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdEdgeXweatherCredentialByteReader;

impl EdgeXweatherCredentialByteReader for StdEdgeXweatherCredentialByteReader {
    type Error = io::Error;

    /// Read one complete credential file and classify only `NotFound` as absence.
    fn read_optional(&mut self, path: &Path) -> Result<Option<Vec<u8>>, Self::Error> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeReadError {
        Unavailable,
    }

    #[derive(Default)]
    struct FakeReader {
        values: HashMap<PathBuf, Result<Option<Vec<u8>>, FakeReadError>>,
        reads: Vec<PathBuf>,
    }

    impl EdgeXweatherCredentialByteReader for FakeReader {
        type Error = FakeReadError;

        /// Return only fixture-owned bytes without touching the host filesystem.
        fn read_optional(&mut self, path: &Path) -> Result<Option<Vec<u8>>, Self::Error> {
            self.reads.push(path.to_path_buf());
            self.values.get(path).cloned().unwrap_or(Ok(None))
        }
    }

    /// Prove two absent default files disable the optional integration in source order.
    #[test]
    fn disables_credentials_when_both_default_files_are_absent() {
        let mut reader = FakeReader::default();

        let credentials =
            load_edge_xweather_credentials(EdgeXweatherCredentialPaths::defaults(), &mut reader)
                .expect("absent credentials");

        assert!(credentials.is_none());
        assert_eq!(
            reader.reads,
            [
                PathBuf::from(DEFAULT_CLIENT_ID_PATH),
                PathBuf::from(DEFAULT_CLIENT_SECRET_PATH),
            ]
        );
    }

    /// Prove configured ordinary files normalize surrounding whitespace and retain values.
    #[test]
    fn loads_configured_complete_credential_pair() {
        let id_path = PathBuf::from("fixtures/client-id");
        let secret_path = PathBuf::from("fixtures/client-secret");
        let mut reader = FakeReader::default();
        reader
            .values
            .insert(id_path.clone(), Ok(Some(b"  client-id-42\r\n".to_vec())));
        reader.values.insert(
            secret_path.clone(),
            Ok(Some(b"\nclient-secret-84\t".to_vec())),
        );

        let credentials = load_edge_xweather_credentials(
            EdgeXweatherCredentialPaths {
                client_id_file: Some("fixtures/client-id"),
                client_secret_file: Some("fixtures/client-secret"),
            },
            &mut reader,
        )
        .expect("configured credentials")
        .expect("complete pair");

        assert_eq!(credentials.client_id(), "client-id-42");
        assert_eq!(credentials.client_secret(), "client-secret-84");
        assert_eq!(reader.reads, [id_path, secret_path]);
    }

    /// Prove one absent configured file reports partial provisioning after both reads.
    #[test]
    fn rejects_partial_configured_credential_pair() {
        let mut reader = FakeReader::default();
        reader.values.insert(
            PathBuf::from("fixtures/client-id"),
            Ok(Some(b"client-id-42".to_vec())),
        );

        let error = load_edge_xweather_credentials(
            EdgeXweatherCredentialPaths {
                client_id_file: Some("fixtures/client-id"),
                client_secret_file: Some("fixtures/client-secret"),
            },
            &mut reader,
        )
        .err()
        .expect("partial provisioning");

        assert_eq!(error, EdgeXweatherCredentialsError::PartialProvisioning);
        assert_eq!(
            reader.reads,
            [
                PathBuf::from("fixtures/client-id"),
                PathBuf::from("fixtures/client-secret"),
            ]
        );
    }

    /// Prove an ordinary identifier read failure stops before the secret path is opened.
    #[test]
    fn retains_identifier_read_failure_order() {
        let mut reader = FakeReader::default();
        reader.values.insert(
            PathBuf::from("fixtures/client-id"),
            Err(FakeReadError::Unavailable),
        );

        let error = load_edge_xweather_credentials(
            EdgeXweatherCredentialPaths {
                client_id_file: Some("fixtures/client-id"),
                client_secret_file: Some("fixtures/client-secret"),
            },
            &mut reader,
        )
        .err()
        .expect("identifier read error");

        assert_eq!(
            error,
            EdgeXweatherCredentialsError::ClientIdRead(FakeReadError::Unavailable)
        );
        assert_eq!(reader.reads, [PathBuf::from("fixtures/client-id")]);
    }
}
