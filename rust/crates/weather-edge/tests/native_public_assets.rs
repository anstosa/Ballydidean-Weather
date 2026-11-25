use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_edge::{
    AdmittedPublicApiUpstreamResponse, AdmittedPublicAssetBytes, NativePublicAssetReader,
    PublicApiProxyClient, PublicApiProxyClientRequest, PublicServingApplicationBody,
    PublicServingApplicationOutcome, PublicServingApplicationRequest, PublicStaticAssetRead,
    PublicStaticTemplateRenderRequest, PublicStaticTemplateRenderer, StaticAssetRoot,
    handle_public_static_and_api,
};

struct DisposableAssets {
    reader: NativePublicAssetReader,
    root: PathBuf,
}

impl DisposableAssets {
    /// create one isolated production-shaped public and compiled tree.
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-public-assets-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("apps/web/public/brand")).expect("public fixture root");
        fs::create_dir_all(root.join("apps/web/dist")).expect("compiled fixture root");
        let reader = NativePublicAssetReader::from_repository_root(&root);
        Self { reader, root }
    }

    /// write one exact fixture file below the selected release root.
    fn write(&self, root: StaticAssetRoot, relative_path: &str, bytes: &[u8]) {
        let path = self.reader.root(root).join(relative_path);
        // create only fixture-owned nested directories
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture asset parent");
        }
        fs::write(path, bytes).expect("fixture asset bytes");
    }
}

impl Drop for DisposableAssets {
    /// remove only this disposable fixture tree.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct PassthroughRenderer;

impl PublicStaticTemplateRenderer for PassthroughRenderer {
    type Error = ();

    /// preserve source bytes for the non-template fixture requests.
    fn render_template(
        &self,
        source: &AdmittedPublicAssetBytes,
        _request: PublicStaticTemplateRenderRequest<'_>,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error> {
        Ok(AdmittedPublicAssetBytes::admitted(source.as_bytes()))
    }
}

struct UnusedApi;

impl PublicApiProxyClient for UnusedApi {
    type Error = ();

    /// reject accidental API dispatch from static fixture requests.
    fn fetch_api(
        &self,
        _request: PublicApiProxyClientRequest<'_>,
    ) -> Result<AdmittedPublicApiUpstreamResponse, Self::Error> {
        Err(())
    }
}

/// unwrap one response owned by the static application path.
fn handled(
    outcome: PublicServingApplicationOutcome,
) -> weather_edge::PublicServingApplicationResponse {
    match outcome {
        PublicServingApplicationOutcome::Handled(response) => response,
        PublicServingApplicationOutcome::Boundary(boundary) => {
            panic!("unexpected boundary: {boundary:?}")
        }
    }
}

/// execute one ordinary static request against the native reader.
fn execute(
    reader: &NativePublicAssetReader,
    method: &str,
    pathname: &str,
) -> weather_edge::PublicServingApplicationResponse {
    handled(handle_public_static_and_api(
        PublicServingApplicationRequest {
            accept: None,
            method,
            pathname,
            release: "fixture-release",
            search: "",
        },
        reader,
        &PassthroughRenderer,
        &UnusedApi,
    ))
}

// verify exact release-root selection
#[test]
fn maps_repository_root_and_keeps_public_and_compiled_assets_separate() {
    let fixture = DisposableAssets::new();
    assert_eq!(
        fixture.reader.root(StaticAssetRoot::Public),
        fixture.root.join("apps/web/public")
    );
    assert_eq!(
        fixture.reader.root(StaticAssetRoot::Compiled),
        fixture.root.join("apps/web/dist")
    );

    fixture.write(StaticAssetRoot::Public, "client.js", b"public client");
    fixture.write(StaticAssetRoot::Compiled, "client.js", b"compiled client");
    fixture.write(StaticAssetRoot::Public, "styles.css", b"public styles");
    fixture.write(StaticAssetRoot::Compiled, "styles.css", b"compiled styles");

    assert_eq!(
        fixture
            .reader
            .read_asset(StaticAssetRoot::Public, "styles.css")
            .expect("public styles")
            .as_bytes(),
        b"public styles"
    );
    assert_eq!(
        fixture
            .reader
            .read_asset(StaticAssetRoot::Compiled, "client.js")
            .expect("compiled client")
            .as_bytes(),
        b"compiled client"
    );
    let missing = fixture
        .reader
        .read_asset(StaticAssetRoot::Public, "manifest.webmanifest")
        .expect_err("missing public asset");
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);
}

// verify get and head response composition
#[test]
fn composes_allowlisted_public_and_compiled_get_and_head_responses() {
    let fixture = DisposableAssets::new();
    fixture.write(
        StaticAssetRoot::Public,
        "brand/favicon.svg",
        b"<svg>native</svg>",
    );
    fixture.write(StaticAssetRoot::Public, "styles.css", b"body{}\n");
    fixture.write(
        StaticAssetRoot::Compiled,
        "client.js",
        b"console.log('native');\n",
    );

    let favicon = execute(&fixture.reader, "GET", "/brand/favicon.svg");
    assert_eq!(favicon.status_code, 200);
    assert_eq!(favicon.headers.content_length, 17);
    assert_eq!(
        favicon.body,
        PublicServingApplicationBody::Bytes(Some(b"<svg>native</svg>".to_vec()))
    );

    let styles = execute(&fixture.reader, "GET", "/assets/fixture-release/styles.css");
    assert_eq!(styles.headers.content_length, 7);
    assert_eq!(
        styles.body,
        PublicServingApplicationBody::Bytes(Some(b"body{}\n".to_vec()))
    );

    let client_head = execute(&fixture.reader, "HEAD", "/assets/fixture-release/client.js");
    assert_eq!(client_head.status_code, 200);
    assert_eq!(client_head.headers.content_length, 23);
    assert_eq!(client_head.body, PublicServingApplicationBody::Bytes(None));
}
