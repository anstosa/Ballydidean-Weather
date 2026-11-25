use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use weather_edge::{
    AdminLoginPageApplicationRequest, AdmittedPublicApiUpstreamResponse, AdmittedPublicAssetBytes,
    CanonicalTimeAdapter, ForecastMapSite, NativePublicAssetReader, NativePublicTemplates,
    PublicApiProxyClient, PublicApiProxyClientRequest, PublicServingApplicationBody,
    PublicServingApplicationOutcome, PublicServingApplicationRequest,
    PublicStaticTemplateRenderRequest, PublicStaticTemplateRenderer, PublicTemplateClock,
    PublicTemplateMaterialCodec, PublicTemplateSettings, handle_admin_login_page_application,
    handle_public_static_and_api,
};

const RELEASE: &str = "2026.11.23-1";
const REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SITE: ForecastMapSite = ForecastMapSite {
    latitude: 47.950_429_954_185_445,
    longitude: -122.427_970_126_081_93,
};

/// The actual Node rendering, release-root digest input, and fixed clock.
struct SourceWitness {
    fields: BTreeMap<String, Vec<u8>>,
    digests: Vec<(String, String, Vec<u8>)>,
    root: PathBuf,
}

impl SourceWitness {
    /// Run the original isolated template functions against image-owned repository files.
    fn load() -> Self {
        let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
        let root = tests.join("../../../..");
        let output = Command::new("node")
            .arg(tests.join("public-templates-node-oracle.mjs"))
            .output()
            .expect("node template oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let fields: BTreeMap<String, Vec<u8>> = String::from_utf8(output.stdout)
            .expect("node fixture stdout")
            .lines()
            .map(
                // preserve the exact multi-line bytes via the test's framing
                |line| {
                    let (key, value) = line.split_once('=').expect("witness field");
                    (key.to_owned(), decode_fixture_hex(value))
                },
            )
            .collect();
        let order = std::str::from_utf8(&fields["order"]).expect("ordered asset names");
        let digests = order
            .split(',')
            .map(
                // bind each node-produced digest to independently read source bytes
                |route| {
                    let name = route.rsplit('/').next().expect("asset filename");
                    let root_name = if name == "styles.css" {
                        "public"
                    } else {
                        "dist"
                    };
                    let content = fs::read(root.join("apps/web").join(root_name).join(name))
                        .expect("source-backed shell asset");
                    let digest = String::from_utf8(fields[&format!("digest:{route}")].clone())
                        .expect("base64 digest");
                    (route.to_owned(), digest, content)
                },
            )
            .collect();
        Self {
            fields,
            digests,
            root,
        }
    }

    /// Borrow a typed string from the exact witness frame.
    fn text(&self, label: &str) -> &str {
        std::str::from_utf8(&self.fields[label]).expect("template witness text")
    }
}

impl PublicTemplateMaterialCodec for SourceWitness {
    /// Select the Node hash only for its complete original asset bytes.
    fn sha256_base64(&self, bytes: &[u8]) -> Option<String> {
        let (_, digest, _) = self.digests.iter().find(
            // verify that startup read the real image-owned bytes
            |(_, _, content)| content == bytes,
        )?;
        digest.strip_prefix("sha256-").map(str::to_owned)
    }

    /// Return the exact Node JSON after comparing both order and every digest.
    fn encode_shell_digests(&self, entries: &[(String, String)]) -> Option<String> {
        let expected: Vec<_> = self
            .digests
            .iter()
            .map(
                // compare exact identity pairs without a Rust JSON codec
                |(route, digest, _)| (route.clone(), digest.clone()),
            )
            .collect();
        assert_eq!(entries, expected);
        Some(self.text("digest_map").to_owned())
    }

    /// Use the configured farm center already admitted for the template witness.
    fn decode_forecast_map_site(&self, bytes: &[u8]) -> Option<ForecastMapSite> {
        (bytes == fs::read(self.root.join("config/sites/ballydidean.json")).unwrap())
            .then_some(SITE)
    }
}

/// Decode only the hex framing of test fixtures, not their original JSON wire.
fn decode_fixture_hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(
            // recover each byte of the original rendering
            |pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap(),
        )
        .collect()
}

struct SourceTime(i64, String);

impl CanonicalTimeAdapter for SourceTime {
    /// Authenticate the same computed ten-minute window as the node fixture.
    fn xweather_valid_time(&self, frame_epoch: i64) -> String {
        assert_eq!(frame_epoch, self.0 - 301_000);
        self.1.clone()
    }

    /// The template does not require ISO rendering.
    fn utc_iso(&self, _: i64) -> String {
        unreachable!("no ISO serialization in this representation")
    }
}

struct FixedClock(i64);

impl PublicTemplateClock for FixedClock {
    /// Retain the exact independent node clock without touching wall time.
    fn now_epoch_ms(&self) -> i64 {
        self.0
    }
}

struct StaticOnly;

impl PublicApiProxyClient for StaticOnly {
    type Error = Infallible;

    /// Static requests never need the upstream API.
    fn fetch_api(
        &self,
        _: PublicApiProxyClientRequest<'_>,
    ) -> Result<AdmittedPublicApiUpstreamResponse, Self::Error> {
        unreachable!("static-only template test requested the API")
    }
}

/// Render the current bundle, privacy, login, and forecast frame with the original Node bytes.
#[test]
fn native_templates_bind_real_shell_digests_and_match_node_representations() {
    let witness = SourceWitness::load();
    let assets = NativePublicAssetReader::from_repository_root(&witness.root);
    let site = weather_edge::read_public_template_site(
        &witness.root.join("config/sites/ballydidean.json"),
        &witness,
    );
    let settings = PublicTemplateSettings::from_runtime_values(
        Some(RELEASE),
        Some(REVISION),
        Some("production"),
        site,
    )
    .unwrap();
    let epoch = witness.text("clock").parse().unwrap();
    let renderer = NativePublicTemplates::initialize(
        settings,
        &assets,
        &witness,
        SourceTime(epoch, witness.text("frame").to_owned()),
        FixedClock(epoch),
    )
    .unwrap();
    assert_eq!(renderer.release(), RELEASE);
    // use actual serving handlers for the normal public html and worker representations
    for (pathname, label) in [
        ("/forecast", "actual_public"),
        ("/privacy", "actual_privacy"),
        ("/service-worker.js", "actual_worker"),
    ] {
        let outcome = handle_public_static_and_api(
            PublicServingApplicationRequest {
                accept: None,
                method: "GET",
                pathname,
                release: renderer.release(),
                search: "",
            },
            &assets,
            &renderer,
            &StaticOnly,
        );
        let PublicServingApplicationOutcome::Handled(response) = outcome else {
            panic!("unexpected application boundary for {pathname}");
        };
        assert_eq!(response.status_code, 200, "{pathname}");
        assert_eq!(
            response.headers.content_length,
            witness.fields[label].len(),
            "{pathname}"
        );
        assert_eq!(
            response.body,
            PublicServingApplicationBody::Bytes(Some(witness.fields[label].clone())),
            "{pathname}"
        );
    }
    let admin = weather_edge::handle_admin_page_application(
        weather_edge::AdminPageApplicationRequest {
            omit_body: false,
            pathname: "/admin",
        },
        &assets,
        &renderer,
    );
    assert_eq!(admin.headers.cache_control, "private, no-store");
    assert_eq!(
        admin.body,
        PublicServingApplicationBody::Bytes(Some(witness.fields["actual_admin"].clone()))
    );
    let login = handle_admin_login_page_application(
        AdminLoginPageApplicationRequest {
            invalid_credentials: false,
            omit_body: false,
        },
        &assets,
        &renderer,
    );
    assert_eq!(
        login.body,
        PublicServingApplicationBody::Bytes(Some(witness.fields["actual_login"].clone()))
    );
    let preload = renderer
        .render_template(
            &AdmittedPublicAssetBytes::admitted(&b"preload: __WEATHER_ROUTE_PRELOAD__"[..]),
            PublicStaticTemplateRenderRequest {
                administrator: false,
                analytics_enabled: true,
                pathname: "/forecast/",
            },
        )
        .unwrap();
    assert_eq!(preload.as_bytes(), witness.fields["preload"]);
    let rounded = weather_edge::forecast_map_preload_link(
        "/forecast/",
        Some(ForecastMapSite {
            latitude: 0.007_812_5,
            longitude: -0.0,
        }),
        epoch,
        &SourceTime(epoch, witness.text("frame").to_owned()),
    );
    assert_eq!(
        format!("preload: {rounded}"),
        witness.text("rounded_preload")
    );
}
