//! Resolve one immutable release deployment through an injected operator authority.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::CaptureEpochInitError;
use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::maintenance_canonical_json::valid_hash;

const CONTROL_PLANE_V17_SHA256: &str =
    "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb";
const TARGET_ENVIRONMENT_MAX_BYTES: usize = 16 * 1024;
const TARGET_ENVIRONMENT_KEYS: &[&str] = &[
    "CLOUDFLARED_IMAGE",
    "POSTGRES_IMAGE",
    "WEATHER_CONTROL_PLANE_SHA256",
    "WEATHER_CONTROL_PLANE_VERSION",
    "WEATHER_DATABASE_NAME",
    "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH",
    "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH",
    "WEATHER_POSTGRES_DIR",
    "WEATHER_RELEASE",
    "WEATHER_SERVER_IMAGE",
    "WEATHER_WEB_IMAGE",
];
const WEATHER_SERVER_IMAGE_PREFIX: &str = "ghcr.io/anstosa/weather-server@";
const WEATHER_WEB_IMAGE_PREFIX: &str = "ghcr.io/anstosa/weather-web@";

/// Bind the six independently admitted identities used by capture initialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureEpochInitDeployment {
    pub control_plane_sha256: String,
    pub control_plane_version: String,
    pub source_commit: String,
    pub source_release: String,
    pub source_server_image_digest: String,
    pub source_web_image_digest: String,
}

impl CaptureEpochInitDeployment {
    /// Admit legacy v14 or the exact reviewed v17 control identity.
    pub(crate) fn validate(&self) -> Result<(), CaptureEpochInitError> {
        let control_identity_matches = self.control_plane_version == "14"
            || (self.control_plane_version == "17"
                && self.control_plane_sha256 == CONTROL_PLANE_V17_SHA256);
        // reject every noncanonical deployment identity together
        if !valid_hash(&self.control_plane_sha256)
            || !control_identity_matches
            || !valid_lower_hex(&self.source_commit, 40)
            || !valid_capture_epoch_target_release(&self.source_release)
            || !valid_image_digest(&self.source_server_image_digest)
            || !valid_image_digest(&self.source_web_image_digest)
        {
            return Err(CaptureEpochInitError::Deployment);
        }
        Ok(())
    }
}

/// Carry the inert-release verifier's exact target binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureEpochDeploymentProof {
    pub target_release: String,
    pub target_commit: String,
}

/// Read and independently prove only the selected immutable deployment.
pub trait CaptureEpochDeploymentAuthority {
    /// Read the selected root-owned release environment through its bounded authority.
    fn read_target_environment(
        &mut self,
        target_release: &str,
    ) -> Result<Vec<u8>, CaptureEpochInitError>;

    /// Verify the selected inert release and return its independently bound commit.
    fn verify_inert_target_release(
        &mut self,
        target_release: &str,
    ) -> Result<CaptureEpochDeploymentProof, CaptureEpochInitError>;
}

/// Read one selected root-owned private release environment without following its final link.
pub fn read_capture_epoch_target_environment(
    releases_directory: &Path,
    target_release: &str,
) -> Result<Vec<u8>, CaptureEpochInitError> {
    use CaptureEpochInitError::{Arguments, Deployment};

    // reject path-shaped release operands before constructing the fixed filename
    if !valid_capture_epoch_target_release(target_release) {
        return Err(Arguments);
    }
    let path = releases_directory.join(format!("{target_release}.env"));
    let file = open_read_no_follow(&path).map_err(|_| Deployment)?;
    let metadata = file.metadata().map_err(|_| Deployment)?;
    let length = usize::try_from(metadata.size()).map_err(|_| Deployment)?;
    // retain the root-private single-link regular-file authority and exact size ceiling
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.gid() != 0
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
        || length > TARGET_ENVIRONMENT_MAX_BYTES
    {
        return Err(Deployment);
    }
    let mut bytes = Vec::with_capacity(length);
    file.take((TARGET_ENVIRONMENT_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Deployment)?;
    // reject replacement, truncation or growth observed through the open descriptor
    if bytes.len() != length || bytes.len() > TARGET_ENVIRONMENT_MAX_BYTES {
        return Err(Deployment);
    }
    Ok(bytes)
}

/// Resolve the original v14 target environment and its independent Git proof.
pub fn resolve_capture_epoch_deployment(
    target_release: &str,
    authority: &mut impl CaptureEpochDeploymentAuthority,
) -> Result<CaptureEpochInitDeployment, CaptureEpochInitError> {
    use CaptureEpochInitError::{Arguments, Deployment};

    // reject malformed target names before consulting either authority
    if !valid_capture_epoch_target_release(target_release) {
        return Err(Arguments);
    }
    let bytes = authority
        .read_target_environment(target_release)
        .map_err(|_| Deployment)?;
    let environment = parse_target_environment(&bytes)?;
    // bind the selected environment to the requested immutable release
    if environment.get("WEATHER_RELEASE").map(String::as_str) != Some(target_release) {
        return Err(Deployment);
    }
    let proof = authority
        .verify_inert_target_release(target_release)
        .map_err(|_| Deployment)?;
    // accept only an exact independently verified target and lowercase commit
    if proof.target_release != target_release || !valid_lower_hex(&proof.target_commit, 40) {
        return Err(Deployment);
    }

    let server_image = environment.get("WEATHER_SERVER_IMAGE").ok_or(Deployment)?;
    let web_image = environment.get("WEATHER_WEB_IMAGE").ok_or(Deployment)?;
    let deployment = CaptureEpochInitDeployment {
        control_plane_sha256: environment
            .get("WEATHER_CONTROL_PLANE_SHA256")
            .ok_or(Deployment)?
            .clone(),
        control_plane_version: environment
            .get("WEATHER_CONTROL_PLANE_VERSION")
            .ok_or(Deployment)?
            .clone(),
        source_commit: proof.target_commit,
        source_release: target_release.to_owned(),
        source_server_image_digest: server_image
            .strip_prefix(WEATHER_SERVER_IMAGE_PREFIX)
            .ok_or(Deployment)?
            .to_owned(),
        source_web_image_digest: web_image
            .strip_prefix(WEATHER_WEB_IMAGE_PREFIX)
            .ok_or(Deployment)?
            .to_owned(),
    };
    deployment.validate()?;
    Ok(deployment)
}

/// Match the immutable release grammar with exactly one or two revision digits.
pub(crate) fn valid_capture_epoch_target_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    let date_matches = bytes.len() >= 12
        && bytes.get(4) == Some(&b'.')
        && bytes.get(7) == Some(&b'.')
        && bytes.get(10) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit);
    let revision_matches = matches!(bytes.get(11), Some(b'1'..=b'9'))
        && matches!(bytes.len(), 12 | 13)
        && bytes[12..].iter().all(u8::is_ascii_digit);
    date_matches && revision_matches
}

/// Parse the exact eleven-field reviewed v14 release environment.
fn parse_target_environment(
    bytes: &[u8],
) -> Result<BTreeMap<String, String>, CaptureEpochInitError> {
    use CaptureEpochInitError::Deployment;

    // preserve the native reader's original sixteen-kibibyte ceiling
    if bytes.len() > TARGET_ENVIRONMENT_MAX_BYTES {
        return Err(Deployment);
    }
    let input = String::from_utf8_lossy(bytes);
    let Some(declarations) = input.strip_suffix('\n') else {
        return Err(Deployment);
    };
    let lines: Vec<&str> = declarations.split('\n').collect();
    // require one trailing newline and exactly eleven declarations
    if lines.len() != TARGET_ENVIRONMENT_KEYS.len() {
        return Err(Deployment);
    }

    let mut environment = BTreeMap::new();
    // retain text after the first separator exactly as the Node parser does
    for line in lines {
        let Some(separator) = line.find('=') else {
            return Err(Deployment);
        };
        // reject empty keys and duplicate declarations
        if separator == 0
            || environment
                .insert(
                    line[..separator].to_owned(),
                    line[separator + 1..].to_owned(),
                )
                .is_some()
        {
            return Err(Deployment);
        }
    }

    let actual_keys: Vec<&str> = environment.keys().map(String::as_str).collect();
    // require the exact population and the fixed v14 image grammar
    if actual_keys != TARGET_ENVIRONMENT_KEYS
        || environment
            .get("WEATHER_CONTROL_PLANE_VERSION")
            .map(String::as_str)
            != Some("14")
        || !environment
            .get("WEATHER_CONTROL_PLANE_SHA256")
            .is_some_and(|value| valid_hash(value))
        || !environment
            .get("WEATHER_SERVER_IMAGE")
            .is_some_and(|value| valid_release_image(value, WEATHER_SERVER_IMAGE_PREFIX))
        || !environment
            .get("WEATHER_WEB_IMAGE")
            .is_some_and(|value| valid_release_image(value, WEATHER_WEB_IMAGE_PREFIX))
    {
        return Err(Deployment);
    }
    Ok(environment)
}

/// Match one repository-qualified sha256 image identity.
fn valid_release_image(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(valid_image_digest)
}

/// Match one lowercase sha256 image digest.
fn valid_image_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_hash)
}

/// Match one fixed-width lowercase hexadecimal identity.
fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = "2026.11.24-1";
    const SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const COMMIT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct FixtureAuthority {
        environment: Vec<u8>,
        proof: CaptureEpochDeploymentProof,
        reads: usize,
        verifications: usize,
    }

    impl CaptureEpochDeploymentAuthority for FixtureAuthority {
        /// Return the fixture's already selected environment bytes.
        fn read_target_environment(
            &mut self,
            _target_release: &str,
        ) -> Result<Vec<u8>, CaptureEpochInitError> {
            self.reads += 1;
            Ok(self.environment.clone())
        }

        /// Return the fixture's independent inert-release proof.
        fn verify_inert_target_release(
            &mut self,
            _target_release: &str,
        ) -> Result<CaptureEpochDeploymentProof, CaptureEpochInitError> {
            self.verifications += 1;
            Ok(self.proof.clone())
        }
    }

    /// Build one ordinary reviewed eleven-field release environment.
    fn environment(release: &str) -> Vec<u8> {
        format!(
            "CLOUDFLARED_IMAGE=cloudflared\n\
             POSTGRES_IMAGE=postgres\n\
             WEATHER_CONTROL_PLANE_SHA256={SHA256}\n\
             WEATHER_CONTROL_PLANE_VERSION=14\n\
             WEATHER_DATABASE_NAME=weather\n\
             WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=0\n\
             WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0\n\
             WEATHER_POSTGRES_DIR=/var/lib/postgresql/data\n\
             WEATHER_RELEASE={release}\n\
             WEATHER_SERVER_IMAGE=ghcr.io/anstosa/weather-server@sha256:{SHA256}\n\
             WEATHER_WEB_IMAGE=ghcr.io/anstosa/weather-web@sha256:{SHA256}\n"
        )
        .into_bytes()
    }

    /// Build one authority with matching environment and Git identities.
    fn authority() -> FixtureAuthority {
        FixtureAuthority {
            environment: environment(RELEASE),
            proof: CaptureEpochDeploymentProof {
                target_release: RELEASE.to_owned(),
                target_commit: COMMIT.to_owned(),
            },
            reads: 0,
            verifications: 0,
        }
    }

    /// Resolve all six deployment identities from the two authority reads.
    #[test]
    fn resolves_reviewed_v14_deployment() {
        let mut authority = authority();

        let actual = resolve_capture_epoch_deployment(RELEASE, &mut authority).unwrap();

        assert_eq!(
            actual,
            CaptureEpochInitDeployment {
                control_plane_sha256: SHA256.to_owned(),
                control_plane_version: "14".to_owned(),
                source_commit: COMMIT.to_owned(),
                source_release: RELEASE.to_owned(),
                source_server_image_digest: format!("sha256:{SHA256}"),
                source_web_image_digest: format!("sha256:{SHA256}"),
            }
        );
        assert_eq!((authority.reads, authority.verifications), (1, 1));
    }

    /// Reject malformed targets before opening either authority.
    #[test]
    fn rejects_noncanonical_target_before_authority_reads() {
        let mut authority = authority();

        let actual = resolve_capture_epoch_deployment("2026.11.24-01", &mut authority);

        assert_eq!(actual, Err(CaptureEpochInitError::Arguments));
        assert_eq!((authority.reads, authority.verifications), (0, 0));
    }

    /// Reject a selected environment bound to a different release.
    #[test]
    fn rejects_environment_release_mismatch_before_git_verification() {
        let mut authority = authority();
        authority.environment = environment("2026.11.24-2");

        let actual = resolve_capture_epoch_deployment(RELEASE, &mut authority);

        assert_eq!(actual, Err(CaptureEpochInitError::Deployment));
        assert_eq!((authority.reads, authority.verifications), (1, 0));
    }

    /// Reject an independent proof bound to a different release.
    #[test]
    fn rejects_git_proof_mismatch() {
        let mut authority = authority();
        authority.proof.target_release = "2026.11.24-2".to_owned();

        let actual = resolve_capture_epoch_deployment(RELEASE, &mut authority);

        assert_eq!(actual, Err(CaptureEpochInitError::Deployment));
        assert_eq!((authority.reads, authority.verifications), (1, 1));
    }

    /// Preserve legacy v14 while pinning the only accepted v17 control hash.
    #[test]
    fn validates_legacy_and_pinned_control_identities() {
        let base = CaptureEpochInitDeployment {
            control_plane_sha256: SHA256.to_owned(),
            control_plane_version: "14".to_owned(),
            source_commit: COMMIT.to_owned(),
            source_release: RELEASE.to_owned(),
            source_server_image_digest: format!("sha256:{SHA256}"),
            source_web_image_digest: format!("sha256:{SHA256}"),
        };
        assert_eq!(base.validate(), Ok(()));

        let mut v17 = base.clone();
        v17.control_plane_version = "17".to_owned();
        assert_eq!(v17.validate(), Err(CaptureEpochInitError::Deployment));
        v17.control_plane_sha256 = CONTROL_PLANE_V17_SHA256.to_owned();
        assert_eq!(v17.validate(), Ok(()));
    }

    /// Match only one- or two-digit nonzero immutable release revisions.
    #[test]
    fn matches_exact_target_release_grammar() {
        assert!(valid_capture_epoch_target_release("2026.11.24-1"));
        assert!(valid_capture_epoch_target_release("0000.00.00-99"));
        assert!(!valid_capture_epoch_target_release("2026.11.24-0"));
        assert!(!valid_capture_epoch_target_release("2026.11.24-01"));
        assert!(!valid_capture_epoch_target_release("2026.11.24-100"));
        assert!(!valid_capture_epoch_target_release("2026.1.24-1"));
    }
}
