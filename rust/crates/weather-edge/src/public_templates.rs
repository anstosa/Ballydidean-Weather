//! Native release template policy with existing JSON, hash, and timestamp ports.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    AdminLoginPageRenderRequest, AdminLoginPageRenderer, AdmittedPublicAssetBytes,
    CanonicalTimeAdapter, ForecastMapSite, PublicStaticAssetRead,
    PublicStaticTemplateRenderRequest, PublicStaticTemplateRenderer, SHELL_VERSIONED_ASSET_NAMES,
    forecast_map_preload_link, versioned_asset_plan,
};

const DEVELOPMENT: &str = "development";
const ASSET_VERSION: &str = "__WEATHER_ASSET_VERSION__";
const GIT_REVISION: &str = "__WEATHER_GIT_REVISION__";
const SHELL_ASSET_DIGESTS: &str = "__WEATHER_SHELL_ASSET_DIGESTS__";
const ADMIN: &str = "__WEATHER_ADMIN__";
const ANALYTICS: &str = "__WEATHER_ANALYTICS__";
const ROUTE_PRELOAD: &str = "__WEATHER_ROUTE_PRELOAD__";
const ADMIN_LOGIN_ERROR: &str = "__WEATHER_ADMIN_LOGIN_ERROR__";
const INVALID_CREDENTIAL_NOTICE: &str =
    r#"<p class="admin-login-error" role="alert">The password is incorrect.</p>"#;

/// Inputs read once at release admission rather than on each ordinary request.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicTemplateSettings {
    release: String,
    web_revision: String,
    production_analytics: bool,
    forecast_map_site: Option<ForecastMapSite>,
}

/// Ordinary startup configuration errors from the predecessor's template owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicTemplateConfigurationError {
    InvalidRelease,
    InvalidWebRevision,
}

impl PublicTemplateSettings {
    /// Bind the exact deployed defaults, immutable release, revision, and optional map center.
    pub fn from_runtime_values(
        release: Option<&str>,
        web_revision: Option<&str>,
        node_environment: Option<&str>,
        site: Option<ForecastMapSite>,
    ) -> Result<Self, PublicTemplateConfigurationError> {
        let release = release.unwrap_or(DEVELOPMENT);
        let web_revision = web_revision.unwrap_or(DEVELOPMENT);
        // require the same release label as the public asset URL resolver
        if release != DEVELOPMENT && !immutable_release_label(release) {
            return Err(PublicTemplateConfigurationError::InvalidRelease);
        }
        // preserve the image's source revision binding and normal development fallback
        if web_revision != DEVELOPMENT && !git_revision(web_revision) {
            return Err(PublicTemplateConfigurationError::InvalidWebRevision);
        }
        Ok(Self {
            release: release.to_owned(),
            web_revision: web_revision.to_owned(),
            production_analytics: node_environment == Some("production") && release != DEVELOPMENT,
            forecast_map_site: site.filter(valid_forecast_map_site),
        })
    }

    /// Return the release shared by the template and edge request planner.
    pub fn release(&self) -> &str {
        &self.release
    }
}

/// Existing wire operations not reimplemented by the Rust template renderer.
pub trait PublicTemplateMaterialCodec {
    /// Hash image-owned bytes with the approved standard base64 digest encoding.
    fn sha256_base64(&self, bytes: &[u8]) -> Option<String>;

    /// Encode the original insertion-ordered digest map as compact JSON.
    fn encode_shell_digests(&self, entries: &[(String, String)]) -> Option<String>;

    /// Read only the optional public site coordinates from its JSON document.
    fn decode_forecast_map_site(&self, bytes: &[u8]) -> Option<ForecastMapSite>;
}

/// Read the optional configured map center without preventing the ordinary shell from starting.
pub fn read_public_template_site(
    path: &Path,
    codec: &impl PublicTemplateMaterialCodec,
) -> Option<ForecastMapSite> {
    let bytes = fs::read(path).ok()?;
    codec
        .decode_forecast_map_site(&bytes)
        .filter(valid_forecast_map_site)
}

/// Clock for the existing ten-minute frame selection.
pub trait PublicTemplateClock {
    /// Read milliseconds since the UTC epoch.
    fn now_epoch_ms(&self) -> i64;
}

/// Standard native wall-clock source used by the production template owner.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemPublicTemplateClock;

impl PublicTemplateClock for SystemPublicTemplateClock {
    /// Observe the native wall clock with a signed epoch for the shared time adapter.
    fn now_epoch_ms(&self) -> i64 {
        // retain the normal positive epoch and the standard pre-epoch fallback
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(value) => i64::try_from(value.as_millis()).unwrap_or(i64::MAX),
            Err(error) => -i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX),
        }
    }
}

/// Immutable startup failure if the release cannot reproduce its exact shell assets.
#[derive(Debug, PartialEq, Eq)]
pub enum PublicTemplateInitializationError<E> {
    Asset { name: &'static str, error: E },
    Digest { name: &'static str },
    DigestMap,
}

/// One edge-owned renderer sharing validated startup material across public and admin requests.
#[derive(Debug)]
pub struct NativePublicTemplates<Time, Clock> {
    settings: PublicTemplateSettings,
    shell_digests: String,
    time: Time,
    clock: Clock,
}

impl<Time: CanonicalTimeAdapter, Clock: PublicTemplateClock> NativePublicTemplates<Time, Clock> {
    /// Read and bind the complete immutable release bundle before admitting the server.
    pub fn initialize<Assets: PublicStaticAssetRead>(
        settings: PublicTemplateSettings,
        assets: &Assets,
        codec: &impl PublicTemplateMaterialCodec,
        time: Time,
        clock: Clock,
    ) -> Result<Self, PublicTemplateInitializationError<Assets::Error>> {
        let mut digests = Vec::with_capacity(SHELL_VERSIONED_ASSET_NAMES.len());
        // preserve node insertion order and route-visible image byte selection
        for name in SHELL_VERSIONED_ASSET_NAMES {
            let plan = versioned_asset_plan(name, false).expect("fixed shell asset name");
            let bytes = assets.read_asset(plan.root, plan.relative_path).map_err(
                // retain exactly which source file prevented startup
                |error| PublicTemplateInitializationError::Asset { name, error },
            )?;
            let digest = codec
                .sha256_base64(bytes.as_bytes())
                .ok_or(PublicTemplateInitializationError::Digest { name })?;
            digests.push((
                format!("/assets/{}/{name}", settings.release),
                format!("sha256-{digest}"),
            ));
        }
        let shell_digests = codec
            .encode_shell_digests(&digests)
            .ok_or(PublicTemplateInitializationError::DigestMap)?;
        Ok(Self {
            settings,
            shell_digests,
            time,
            clock,
        })
    }

    /// Return the release shared by the template and edge route selection.
    pub fn release(&self) -> &str {
        self.settings.release()
    }
}

impl<Time: CanonicalTimeAdapter, Clock: PublicTemplateClock> PublicStaticTemplateRenderer
    for NativePublicTemplates<Time, Clock>
{
    type Error = std::convert::Infallible;

    /// Render the deployed shell, service worker, or other admitted public template.
    fn render_template(
        &self,
        source: &AdmittedPublicAssetBytes,
        request: PublicStaticTemplateRenderRequest<'_>,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error> {
        let administrator = if request.administrator {
            "true"
        } else {
            "false"
        };
        let analytics = if self.settings.production_analytics
            && request.analytics_enabled
            && !request.administrator
        {
            "true"
        } else {
            "false"
        };
        // build the provider frame only for the reviewed route and optional map center
        let preload = if matches!(request.pathname, "/forecast" | "/forecast/")
            && self.settings.forecast_map_site.is_some()
        {
            forecast_map_preload_link(
                request.pathname,
                self.settings.forecast_map_site,
                self.clock.now_epoch_ms(),
                &self.time,
            )
        } else {
            String::new()
        };
        Ok(render(
            source,
            &[
                (ASSET_VERSION, &self.settings.release),
                (GIT_REVISION, &self.settings.web_revision),
                (SHELL_ASSET_DIGESTS, &self.shell_digests),
                (ADMIN, administrator),
                (ANALYTICS, analytics),
                (ROUTE_PRELOAD, &preload),
            ],
        ))
    }
}

impl<Time, Clock> AdminLoginPageRenderer for NativePublicTemplates<Time, Clock> {
    type Error = std::convert::Infallible;

    /// Render the source's fixed login state rather than synthesizing a new document.
    fn render_login_page(
        &self,
        source: &AdmittedPublicAssetBytes,
        request: AdminLoginPageRenderRequest,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error> {
        let message = if request.invalid_credentials {
            INVALID_CREDENTIAL_NOTICE
        } else {
            ""
        };
        Ok(render(
            source,
            &[
                (ASSET_VERSION, &self.settings.release),
                (ADMIN_LOGIN_ERROR, message),
            ],
        ))
    }
}

/// Apply replace-all substitutions in the original source order.
fn render(
    source: &AdmittedPublicAssetBytes,
    replacements: &[(&str, &str)],
) -> AdmittedPublicAssetBytes {
    let mut text = String::from_utf8_lossy(source.as_bytes());
    // avoid copying the entire document for a marker absent from this representation
    for (marker, value) in replacements {
        if text.contains(marker) {
            text = text.replace(marker, value).into();
        }
    }
    AdmittedPublicAssetBytes::admitted(text.into_owned().into_bytes())
}

/// Match the closed immutable release label used by the web image's asset URLs.
fn immutable_release_label(value: &str) -> bool {
    let bytes = value.as_bytes();
    // the deployed label consists of one date-shaped prefix and a one- or two-digit serial
    if !(12..=13).contains(&bytes.len()) {
        return false;
    }
    bytes.iter().enumerate().all(
        // require the same ascii separators and first nonzero serial digit
        |(index, marker)| match index {
            4 | 7 => *marker == b'.',
            10 => *marker == b'-',
            11 => (b'1'..=b'9').contains(marker),
            _ => marker.is_ascii_digit(),
        },
    )
}

/// Match the image's normalized forty-character Git commit identity.
fn git_revision(value: &str) -> bool {
    value.len() == 40
        && value.bytes().all(
            // metadata retains the lowercase hex spelling from the producer
            |marker| marker.is_ascii_digit() || (b'a'..=b'f').contains(&marker),
        )
}

/// Use the same optional bounded map coordinates as the Node configuration reader.
fn valid_forecast_map_site(site: &ForecastMapSite) -> bool {
    (-85.0..=85.0).contains(&site.latitude) && (-180.0..=180.0).contains(&site.longitude)
}
