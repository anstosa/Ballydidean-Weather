//! Native process configuration and immutable release assets for the edge startup owner.

use crate::{
    NativeEdgeOrigins, NativeOutboundHttpError, NativeWirePrimitives,
    maintenance_canonical_json::field,
};
use std::io;
use url::Url;
use weather_contracts::JsonValue;
use weather_edge::{
    ArchiveWirePrimitives, CanonicalTimeAdapter, EdgeStartupConfiguration,
    EdgeStartupConfigurationError, EdgeXweatherCredentials, EdgeXweatherCredentialsError,
    ForecastMapSite, NativeEdgeStartupAdmission, NativeEdgeStartupAdmissionError,
    NativePublicAssetReader, NativePublicTemplates, OwnedEdgeStartupEnvironment,
    PublicTemplateConfigurationError, PublicTemplateInitializationError,
    PublicTemplateMaterialCodec, PublicTemplateSettings, RevisionArchiveCanonicalCodec,
    StdEdgeProcessSettingsError, StdEdgeProcessSettingsReader, StdEdgeXweatherCredentialByteReader,
    SystemPublicTemplateClock, project_edge_startup_configuration_with_credentials,
    read_public_template_site,
};

/// Actual admitted process startup values, with map credentials kept outside diagnostic material.
pub struct NativeEdgeConfiguration {
    pub configuration: EdgeStartupConfiguration<Url>,
    pub map_credentials: Option<EdgeXweatherCredentials>,
}

/// Preserve the original environment and configured credential loading categories.
#[derive(Debug)]
pub enum NativeEdgeConfigurationError {
    Environment(StdEdgeProcessSettingsError),
    Admission(
        EdgeStartupConfigurationError<
            NativeEdgeStartupAdmissionError<NativeOutboundHttpError>,
            EdgeXweatherCredentialsError<io::Error>,
        >,
    ),
}

/// Read the exact edge environment and credentials using the approved native URL and file owners.
pub fn load_native_edge_configuration()
-> Result<NativeEdgeConfiguration, NativeEdgeConfigurationError> {
    let environment = OwnedEdgeStartupEnvironment::read(&mut StdEdgeProcessSettingsReader)
        .map_err(NativeEdgeConfigurationError::Environment)?;
    let (configuration, map_credentials) = project_edge_startup_configuration_with_credentials(
        environment.as_startup_environment(),
        &mut NativeEdgeStartupAdmission::new(NativeEdgeOrigins),
        &mut StdEdgeXweatherCredentialByteReader,
    )
    .map_err(NativeEdgeConfigurationError::Admission)?;
    Ok(NativeEdgeConfiguration {
        configuration,
        map_credentials,
    })
}

/// Initialize the retained map owner from the exact startup path and shared HTTP pool.
pub fn initialize_native_edge_tile_owner(
    startup: &mut NativeEdgeConfiguration,
    client: std::sync::Arc<crate::NativeOutboundHttp>,
) -> Result<crate::NativeEdgeTileOwner, crate::NativeEdgeTileOwnerCreateError> {
    crate::NativeEdgeTileOwner::new(
        startup.configuration.xweather_map_origin.clone(),
        startup.map_credentials.take(),
        client,
        startup.configuration.xweather_usage_path.clone(),
    )
}

/// Shared native public forecast proxy and widget source using the same persisted settings.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub struct NativeEdgePublicUpstreams {
    pub api: crate::NativePublicApiProxy<
        weather_edge::PublicAdjustmentProjector<
            crate::NativeSharedAdjustmentSettings,
            NativeWirePrimitives,
        >,
    >,
    pub widget: crate::NativeWidgetForecastSource<crate::NativeSharedAdjustmentSettings>,
    pub widget_clock: crate::NativeWidgetReceivedAt,
}

/// Bind one existing admin settings handle to both ordinary and widget forecast upstreams.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn initialize_native_edge_public_upstreams(
    configuration: &EdgeStartupConfiguration<Url>,
    admin: &crate::NativeEdgeAdminOwner,
    client: std::sync::Arc<crate::NativeOutboundHttp>,
) -> Result<NativeEdgePublicUpstreams, NativeOutboundHttpError> {
    let settings = admin.shared_adjustment_settings();
    let api = crate::NativePublicApiProxy::new(
        configuration.api_origin.clone(),
        std::sync::Arc::clone(&client),
        weather_edge::PublicAdjustmentProjector::new(settings.clone(), NativeWirePrimitives),
    )?;
    let widget =
        crate::NativeWidgetForecastSource::new(configuration.api_origin.clone(), client, settings)?;
    Ok(NativeEdgePublicUpstreams {
        api,
        widget,
        widget_clock: crate::NativeWidgetReceivedAt,
    })
}

/// Native immutable asset material loaded during the existing edge resource startup phase.
pub struct NativeEdgePublicAssets {
    pub assets: NativePublicAssetReader,
    pub templates: NativePublicTemplates<NativeWirePrimitives, SystemPublicTemplateClock>,
}

/// Keep template configuration and actual release-asset read failures distinct.
#[derive(Debug)]
pub enum NativeEdgePublicAssetsError {
    Settings(PublicTemplateConfigurationError),
    Initialize(PublicTemplateInitializationError<io::Error>),
}

/// Load only the optional public map center from the configured site document.
pub fn native_edge_map_site(
    configuration: &EdgeStartupConfiguration<Url>,
) -> Option<ForecastMapSite> {
    read_public_template_site(
        &configuration.site_configuration_path,
        &NativeWirePrimitives,
    )
}

/// Initialize the actual release assets and digest map after the existing resource order admits them.
pub fn initialize_native_edge_public_assets(
    configuration: &EdgeStartupConfiguration<Url>,
    site: Option<ForecastMapSite>,
) -> Result<NativeEdgePublicAssets, NativeEdgePublicAssetsError> {
    let assets = NativePublicAssetReader::new(
        configuration.public_root.clone(),
        configuration.compiled_root.clone(),
    );
    let node_environment = configuration.production_analytics.then_some("production");
    let settings = PublicTemplateSettings::from_runtime_values(
        Some(&configuration.release),
        Some(&configuration.web_revision),
        node_environment,
        site,
    )
    .map_err(NativeEdgePublicAssetsError::Settings)?;
    let templates = NativePublicTemplates::initialize(
        settings,
        &assets,
        &NativeWirePrimitives,
        NativeWirePrimitives,
        SystemPublicTemplateClock,
    )
    .map_err(NativeEdgePublicAssetsError::Initialize)?;
    Ok(NativeEdgePublicAssets { assets, templates })
}

impl CanonicalTimeAdapter for NativeWirePrimitives {
    /// Format admitted map frames with the source's fourteen fixed UTC digits.
    fn xweather_valid_time(&self, epoch_ms: i64) -> String {
        let canonical = self.utc_iso(epoch_ms);
        // the original Date serialization supplies the fixed calendar and time positions
        canonical
            .bytes()
            .take(19)
            .filter(u8::is_ascii_digit)
            .map(char::from)
            .collect()
    }

    /// Format an admitted process or widget instant through the actual canonical owner.
    fn utc_iso(&self, epoch_ms: i64) -> String {
        self.format_utc_milliseconds(epoch_ms)
            .expect("admitted edge UTC instant")
    }
}

impl PublicTemplateMaterialCodec for NativeWirePrimitives {
    /// Hash exact image-owned bytes with the original standard-base64 digest spelling.
    fn sha256_base64(&self, bytes: &[u8]) -> Option<String> {
        self.encode_standard_base64(&RevisionArchiveCanonicalCodec::sha256(self, bytes))
    }

    /// Encode the source's fixed ordered URL-to-digest map without archive framing.
    fn encode_shell_digests(&self, entries: &[(String, String)]) -> Option<String> {
        let value = JsonValue::Object(
            entries
                .iter()
                .map(|(name, value)| {
                    (
                        name.encode_utf16().collect(),
                        JsonValue::String(value.encode_utf16().collect()),
                    )
                })
                .collect(),
        );
        let mut bytes = self.encode_ordered_json(&value)?;
        // preserve compact JavaScript object spelling without a final LF
        if bytes.pop() != Some(b'\n') {
            return None;
        }
        String::from_utf8(bytes).ok()
    }

    /// Decode only the two public coordinates from the existing site configuration shape.
    fn decode_forecast_map_site(&self, bytes: &[u8]) -> Option<ForecastMapSite> {
        let configuration = self.decode_json(String::from_utf8_lossy(bytes).as_bytes())?;
        let site = field(&configuration, "site")?;
        let JsonValue::Number(latitude) = field(site, "latitude")? else {
            return None;
        };
        let JsonValue::Number(longitude) = field(site, "longitude")? else {
            return None;
        };
        Some(ForecastMapSite {
            latitude: *latitude,
            longitude: *longitude,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use weather_edge::{
        EdgeStartupEnvironment, EdgeXweatherCredentialByteReader, PublicStaticAssetRead,
        PublicStaticTemplateRenderRequest, PublicStaticTemplateRenderer, StaticAssetRoot,
    };

    struct MissingCredentials;
    impl EdgeXweatherCredentialByteReader for MissingCredentials {
        type Error = io::Error;
        /// Use a disposable fixture without any real credential access.
        fn read_optional(&mut self, _: &Path) -> Result<Option<Vec<u8>>, Self::Error> {
            Ok(None)
        }
    }

    /// Exercise the actual release asset and template owners against shipped compiled files.
    #[test]
    fn native_edge_startup_material_renders_real_release_assets() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let (configuration, credentials) = project_edge_startup_configuration_with_credentials(
            EdgeStartupEnvironment::new(&root),
            &mut NativeEdgeStartupAdmission::new(NativeEdgeOrigins),
            &mut MissingCredentials,
        )
        .unwrap();
        assert!(credentials.is_none());
        let site = native_edge_map_site(&configuration).unwrap();
        let assets = initialize_native_edge_public_assets(&configuration, Some(site)).unwrap();
        let source = assets
            .assets
            .read_asset(StaticAssetRoot::Public, "index.html")
            .unwrap();
        let rendered = assets
            .templates
            .render_template(
                &source,
                PublicStaticTemplateRenderRequest {
                    administrator: false,
                    analytics_enabled: false,
                    pathname: "/forecast",
                },
            )
            .unwrap();
        let html = String::from_utf8(rendered.as_bytes().to_vec()).unwrap();
        assert!(html.contains("/assets/development/"));
        // the digest map belongs to the service worker, not the HTML shell
        let script = assets
            .assets
            .read_asset(StaticAssetRoot::Public, "service-worker.js")
            .unwrap();
        let script = assets
            .templates
            .render_template(
                &script,
                PublicStaticTemplateRenderRequest {
                    administrator: false,
                    analytics_enabled: false,
                    pathname: "/service-worker.js",
                },
            )
            .unwrap();
        let script = String::from_utf8(script.as_bytes().to_vec()).unwrap();
        assert!(script.contains("sha256-"));
        assert!(!script.contains("__WEATHER_SHELL_ASSET_DIGESTS__"));
        // the current source intentionally has no route-level map preload marker
        assert!(!html.contains("/maps/xweather/history/radar/"));
        let preload = weather_edge::forecast_map_preload_link(
            "/forecast",
            Some(site),
            0,
            &NativeWirePrimitives,
        );
        assert!(preload.contains("/maps/xweather/history/radar/19700101000000/"));
        assert_eq!(
            NativeWirePrimitives.xweather_valid_time(0),
            "19700101000000"
        );
    }
}
