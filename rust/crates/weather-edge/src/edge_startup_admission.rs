//! Admit native startup paths and ports while keeping URL ownership outside the edge application.

use std::path::{Component, Path, PathBuf};

use crate::{AdmittedPublicApiOrigin, EdgeStartupAdmission};

/// Typed URL boundary supplied by the selected runtime without duplicating protected URL logic.
pub trait EdgeStartupOrigins {
    type Error;
    type XweatherMapOrigin;

    /// Admit the core API origin with the existing runtime URL owner.
    fn api_origin(&mut self, value: &str) -> Result<AdmittedPublicApiOrigin, Self::Error>;

    /// Admit the compose-private API origin with its existing restrictions.
    fn maintenance_api_origin(
        &mut self,
        value: &str,
    ) -> Result<AdmittedPublicApiOrigin, Self::Error>;

    /// Admit the configured map provider URL with its existing runtime policy.
    fn xweather_map_origin(&mut self, value: &str) -> Result<Self::XweatherMapOrigin, Self::Error>;
}

/// Startup path and port failures remain distinct from the selected URL owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeEdgeStartupAdmissionError<Error> {
    WorkingDirectory,
    Port,
    Origin(Error),
}

/// Actual local lexical path and port owner over the admitted URL boundary.
pub struct NativeEdgeStartupAdmission<Origins>(Origins);

impl<Origins> NativeEdgeStartupAdmission<Origins> {
    /// Reuse the selected URL owner for every startup origin.
    pub const fn new(origins: Origins) -> Self {
        Self(origins)
    }
}

impl<Origins: EdgeStartupOrigins> EdgeStartupAdmission for NativeEdgeStartupAdmission<Origins> {
    type Error = NativeEdgeStartupAdmissionError<Origins::Error>;
    type XweatherMapOrigin = Origins::XweatherMapOrigin;

    /// Process current directory is absolute before it reaches the application.
    fn resolve_working_root(&mut self, value: &Path) -> Result<PathBuf, Self::Error> {
        lexical_absolute_path(value).ok_or(NativeEdgeStartupAdmissionError::WorkingDirectory)
    }

    /// Match Node's lexical relative evidence-root resolution without reading its filesystem.
    fn resolve_evidence_root(&mut self, root: &Path, value: &str) -> Result<PathBuf, Self::Error> {
        lexical_absolute_path(&root.join(value))
            .ok_or(NativeEdgeStartupAdmissionError::WorkingDirectory)
    }

    /// Keep the old finite listener range and numeric environment syntax.
    fn parse_port(&mut self, value: &str) -> Result<u16, Self::Error> {
        parse_edge_listener_port(value).ok_or(NativeEdgeStartupAdmissionError::Port)
    }

    /// Delegate to the selected public upstream URL owner.
    fn admit_api_origin(&mut self, value: &str) -> Result<AdmittedPublicApiOrigin, Self::Error> {
        self.0
            .api_origin(value)
            .map_err(NativeEdgeStartupAdmissionError::Origin)
    }

    /// Delegate to the selected compose-private URL owner.
    fn admit_maintenance_api_origin(
        &mut self,
        value: &str,
    ) -> Result<AdmittedPublicApiOrigin, Self::Error> {
        self.0
            .maintenance_api_origin(value)
            .map_err(NativeEdgeStartupAdmissionError::Origin)
    }

    /// Delegate to the selected map-provider URL owner.
    fn admit_xweather_map_origin(
        &mut self,
        value: &str,
    ) -> Result<Self::XweatherMapOrigin, Self::Error> {
        self.0
            .xweather_map_origin(value)
            .map_err(NativeEdgeStartupAdmissionError::Origin)
    }
}

/// Match the retained Node POSIX lexical path resolver for an already absolute process path.
fn lexical_absolute_path(value: &Path) -> Option<PathBuf> {
    // reject an unbound path without using process cwd implicitly
    if !value.is_absolute() {
        return None;
    }
    let mut output = PathBuf::from("/");
    // collapse dot and parent components without following symbolic links
    for component in value.components() {
        match component {
            Component::RootDir | Component::CurDir => (),
            Component::ParentDir => {
                output.pop();
            }
            Component::Normal(value) => output.push(value),
            Component::Prefix(_) => return None,
        }
    }
    Some(output)
}

/// Parse a numeric JS environment string as one finite nonzero TCP port.
pub fn parse_edge_listener_port(source: &str) -> Option<u16> {
    let value = source.trim_matches(node_number_whitespace);
    // handle the three unsigned radix prefixes accepted by Node Number
    let prefixed = match value.as_bytes() {
        [b'0', b'x' | b'X', ..] => Some(16),
        [b'0', b'o' | b'O', ..] => Some(8),
        [b'0', b'b' | b'B', ..] => Some(2),
        _ => None,
    };
    let port = if let Some(radix) = prefixed {
        let digits = &value[2..];
        // reject signs and keep only the original radix-specific numeric digits
        if digits.is_empty() || !digits.chars().all(|digit| digit.is_digit(radix)) {
            return None;
        }
        u16::from_str_radix(digits, radix).ok()?
    } else {
        let parsed: f64 = value.parse().ok()?;
        // preserve the predecessor's Number rounding before checking the integer TCP range
        if parsed.fract() != 0.0 || !(1.0..=f64::from(u16::MAX)).contains(&parsed) {
            return None;
        }
        parsed as u16
    };
    (port != 0).then_some(port)
}

/// Preserve ECMAScript whitespace and line ending handling before numeric parsing.
fn node_number_whitespace(value: char) -> bool {
    matches!(
        value,
        '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}'
            | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}'
            | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

impl<Origin> From<&crate::EdgeStartupConfiguration<Origin>>
    for crate::EdgeStartupLifecycleConfiguration
{
    /// Pass only the admitted controller and listener gates into the startup lifecycle.
    fn from(config: &crate::EdgeStartupConfiguration<Origin>) -> Self {
        Self {
            archive_transfer_enabled: config.scheduled_archive_transfer_enabled,
            private_listener_enabled: config.adjustment_maintenance_internal_port.is_some(),
            scheduler: crate::ForecastSchedulerConfiguration {
                enabled: config.scheduled_capture_enabled,
                migration_ready: config.scheduled_capture_migration_ready,
            },
        }
    }
}
