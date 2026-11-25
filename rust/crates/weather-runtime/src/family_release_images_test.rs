use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;

use super::{
    FamilyCatalogImageVerification, FamilyReleaseBoundImageRead, FamilyReleaseImagesInput,
    FamilyReleaseRebindInput, verify_family_release_images, verify_family_release_rebind,
};
use crate::family_target_catalog::{object, string};
use crate::family_target_catalog_validation::{V2, V3};
use crate::{
    FamilyTargetImageRead, MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson,
    MaintenanceCanonicalJson, NativeFamilyTargetImage,
};

const REGISTRY_PATH: &str = "config/forecast-adjustments/ballydidean-temperature-canary.json";
const REGISTRY_MAXIMUM: usize = 8 * 1024 * 1024;
const TARGET_CATALOG_BYTES: &[u8] =
    b"{\"contractVersion\":\"adjustment-installed-candidate-catalog/v3\",\"entries\":[]}\n";
const BASELINE_CATALOG_BYTES: &[u8] =
    b"{\"contractVersion\":\"adjustment-installed-candidate-catalog/v2\",\"entries\":[]}\n";
const TARGET_REGISTRY_BYTES: &[u8] = b"{\"target\":true}\n";
const BASELINE_REGISTRY_BYTES: &[u8] = b"{\"baseline\":true}\n";
const TARGET_REGISTRY_SHA256: &str =
    "e7221bb692d80acdf25012015644f321533eac58808ab4a67ffd61010fefa115";
const BASELINE_REGISTRY_SHA256: &str =
    "fa1bd2c1343b99da0ace6a3c78fbb77df6276948af31c9a9d6a67debaef6c8c0";
const TARGET_FAMILY_BYTES: &[u8] = b"{\"activeInstalledReceiptSha256\":null,\"controlInstalledReceiptSha256\":null,\"family\":\"temperature\",\"registrySha256\":\"e7221bb692d80acdf25012015644f321533eac58808ab4a67ffd61010fefa115\",\"shadowInstalledReceiptSha256\":null}\n";
const COMPENSATION_FAMILY_BYTES: &[u8] = b"{\"activeInstalledReceiptSha256\":null,\"controlInstalledReceiptSha256\":null,\"family\":\"temperature\",\"registrySha256\":\"fa1bd2c1343b99da0ace6a3c78fbb77df6276948af31c9a9d6a67debaef6c8c0\",\"shadowInstalledReceiptSha256\":null}\n";
const TARGET_FAMILY_SHA256: &str =
    "29102c22033f08665173f1d395526de486cd60ba44bbe53431126a6f7f720478";
const COMPENSATION_FAMILY_SHA256: &str =
    "0842440b7bbb7cdb0d3bbccc5559f4433409f100cb9e8cc4aa13dd879d03320e";
const SOURCE_SERVER: &str = "ghcr.io/anstosa/weather-server@sha256:5555555555555555555555555555555555555555555555555555555555555555";
const SOURCE_WEB: &str = "ghcr.io/anstosa/weather-web@sha256:6666666666666666666666666666666666666666666666666666666666666666";
const TARGET_SERVER: &str = "ghcr.io/anstosa/weather-server@sha256:1111111111111111111111111111111111111111111111111111111111111111";
const TARGET_WEB: &str = "ghcr.io/anstosa/weather-web@sha256:2222222222222222222222222222222222222222222222222222222222222222";
const COMPENSATION_SERVER: &str = "ghcr.io/anstosa/weather-server@sha256:3333333333333333333333333333333333333333333333333333333333333333";
const COMPENSATION_WEB: &str = "ghcr.io/anstosa/weather-web@sha256:4444444444444444444444444444444444444444444444444444444444444444";
const TARGET_IMAGE_BYTES: &[u8] = b"[{\"digest\":\"sha256:1111111111111111111111111111111111111111111111111111111111111111\",\"runtime\":\"server\"},{\"digest\":\"sha256:2222222222222222222222222222222222222222222222222222222222222222\",\"runtime\":\"web\"}]\n";
const COMPENSATION_IMAGE_BYTES: &[u8] = b"[{\"digest\":\"sha256:3333333333333333333333333333333333333333333333333333333333333333\",\"runtime\":\"server\"},{\"digest\":\"sha256:4444444444444444444444444444444444444444444444444444444444444444\",\"runtime\":\"web\"}]\n";
const TARGET_IMAGE_SHA256: &str =
    "3457a1c387cbbcdcd25c5e31fc9aaf740a8ab3544a051061030badde7dabbd2c";
const COMPENSATION_IMAGE_SHA256: &str =
    "d4e13757c2c019214b7d86a7ac8b1538ea2983b058eeef149ed72ff165e80de6";

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Verify {
        image: String,
        bytes: Vec<u8>,
    },
    Read {
        owner: &'static str,
        path: String,
        maximum: usize,
    },
}

struct FixtureImage {
    owner: &'static str,
    image_reference: &'static str,
    bytes: Vec<u8>,
    events: Rc<RefCell<Vec<Event>>>,
}

impl FamilyTargetImageRead for FixtureImage {
    /// Record one exact immutable image read before returning its fixture bytes.
    fn read_member(
        &mut self,
        path: &str,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
        self.events.borrow_mut().push(Event::Read {
            owner: self.owner,
            path: path.to_owned(),
            maximum: maximum_bytes,
        });
        Ok(self.bytes.clone())
    }
}

impl FamilyReleaseBoundImageRead for FixtureImage {
    /// Expose the exact immutable image selected by this fixture reader.
    fn image_reference(&self) -> &str {
        self.image_reference
    }
}

struct FixtureVerifier {
    events: Rc<RefCell<Vec<Event>>>,
}

impl FamilyCatalogImageVerification for FixtureVerifier {
    /// Record the compiled-image verification boundary without running a container.
    fn verify_catalog(
        &mut self,
        image: &str,
        bytes: &[u8],
    ) -> Result<(), MaintenanceAnchorStatusError> {
        self.events.borrow_mut().push(Event::Verify {
            image: image.to_owned(),
            bytes: bytes.to_vec(),
        });
        Ok(())
    }
}

struct FixtureCodec {
    parses: Vec<(&'static [u8], JsonValue)>,
    encodings: Vec<(JsonValue, &'static [u8])>,
    hashes: Vec<(&'static [u8], &'static str)>,
    hashed: Vec<Vec<u8>>,
}

impl MaintenanceCanonicalJson for FixtureCodec {
    /// Parse only the exact canonical catalog fixtures.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.parses
            .iter()
            .find_map(|(expected, value)| (*expected == bytes).then(|| value.clone()))
    }

    /// Emit only exact canonical preimages captured from Node.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        self.encodings
            .iter()
            .find_map(|(expected, bytes)| (expected == value).then(|| bytes.to_vec()))
    }

    /// Record and hash only the exact Node-derived fixture preimages.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.hashed.push(bytes.to_vec());
        self.hashes.iter().find_map(|(preimage, digest)| {
            (*preimage == bytes).then(|| Sha256Digest::admitted(*digest))
        })
    }
}

impl MaintenanceAnchorStatusJson for FixtureCodec {
    /// Leave JavaScript coercion unreachable for empty catalog entries.
    fn js_string(&mut self, _: &JsonValue) -> Option<Vec<u16>> {
        None
    }
}

struct FixtureTime;

impl CurrentTimeResolver for FixtureTime {
    /// Leave timestamp resolution unreachable for empty catalog entries.
    fn resolve(&self, _: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        unreachable!("empty catalog fixtures contain no timestamps")
    }
}

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    /// Create one isolated disposable process fixture directory.
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-family-release-images-{}-{sequence}",
            std::process::id(),
        ));
        fs::create_dir(&path).expect("create family release image fixture");
        Self(path)
    }

    /// Borrow the disposable fixture directory.
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureDirectory {
    /// Remove only this disposable process fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// build one closed empty catalog fixture
fn catalog(version: &str) -> JsonValue {
    object(vec![
        ("contractVersion", string(version)),
        ("entries", JsonValue::Array(Vec::new())),
    ])
}

// build one exact two-image release environment
fn environment(server: &str, web: &str) -> JsonValue {
    object(vec![
        ("WEATHER_SERVER_IMAGE", string(server)),
        ("WEATHER_WEB_IMAGE", string(web)),
    ])
}

// build the exact family identity preimage expected from an empty catalog
fn family_identity(registry_sha256: &str) -> JsonValue {
    object(vec![
        ("activeInstalledReceiptSha256", JsonValue::Null),
        ("controlInstalledReceiptSha256", JsonValue::Null),
        ("family", string("temperature")),
        ("registrySha256", string(registry_sha256)),
        ("shadowInstalledReceiptSha256", JsonValue::Null),
    ])
}

// build the fixed runtime-ordered image identity preimage
fn image_identity(server: &str, web: &str) -> JsonValue {
    JsonValue::Array(vec![
        object(vec![
            (
                "digest",
                string(server.split('@').nth(1).expect("server digest")),
            ),
            ("runtime", string("server")),
        ]),
        object(vec![
            ("digest", string(web.split('@').nth(1).expect("web digest"))),
            ("runtime", string("web")),
        ]),
    ])
}

// provide all canonical and hash fixtures used by a complete release binding
fn fixture_codec() -> FixtureCodec {
    let target_catalog = catalog(V3);
    let baseline_catalog = catalog(V2);
    FixtureCodec {
        parses: vec![
            (TARGET_CATALOG_BYTES, target_catalog.clone()),
            (BASELINE_CATALOG_BYTES, baseline_catalog.clone()),
        ],
        encodings: vec![
            (target_catalog, TARGET_CATALOG_BYTES),
            (baseline_catalog, BASELINE_CATALOG_BYTES),
            (family_identity(TARGET_REGISTRY_SHA256), TARGET_FAMILY_BYTES),
            (
                family_identity(BASELINE_REGISTRY_SHA256),
                COMPENSATION_FAMILY_BYTES,
            ),
            (
                image_identity(TARGET_SERVER, TARGET_WEB),
                TARGET_IMAGE_BYTES,
            ),
            (
                image_identity(COMPENSATION_SERVER, COMPENSATION_WEB),
                COMPENSATION_IMAGE_BYTES,
            ),
        ],
        hashes: vec![
            (TARGET_REGISTRY_BYTES, TARGET_REGISTRY_SHA256),
            (BASELINE_REGISTRY_BYTES, BASELINE_REGISTRY_SHA256),
            (TARGET_FAMILY_BYTES, TARGET_FAMILY_SHA256),
            (COMPENSATION_FAMILY_BYTES, COMPENSATION_FAMILY_SHA256),
            (TARGET_IMAGE_BYTES, TARGET_IMAGE_SHA256),
            (COMPENSATION_IMAGE_BYTES, COMPENSATION_IMAGE_SHA256),
        ],
        hashed: Vec::new(),
    }
}

// bind one fixture reader to the shared operation log
fn image(
    owner: &'static str,
    image_reference: &'static str,
    bytes: &[u8],
    events: &Rc<RefCell<Vec<Event>>>,
) -> FixtureImage {
    FixtureImage {
        owner,
        image_reference,
        bytes: bytes.to_vec(),
        events: Rc::clone(events),
    }
}

// write one executable registry reader shim
fn write_registry_program(root: &Path) -> PathBuf {
    let path = root.join("read-registry");
    fs::write(
        &path,
        "#!/bin/sh\nset -eu\nprintf '%s\\n' '{\"target\":true}'\n",
    )
    .expect("write registry reader shim");
    let mut permissions = fs::metadata(&path)
        .expect("read registry reader metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&path, permissions).expect("make registry reader executable");
    path
}

/// Verify both compiled catalogs before ordered bounded reads and retain exact identities.
#[test]
fn verifies_catalogs_then_reads_registries_and_returns_exact_identities() {
    let source_environment = environment(SOURCE_SERVER, SOURCE_WEB);
    let target_environment = environment(TARGET_SERVER, TARGET_WEB);
    let compensation_environment = environment(COMPENSATION_SERVER, COMPENSATION_WEB);
    let input = FamilyReleaseImagesInput {
        family: "temperature",
        baseline_catalog_bytes: BASELINE_CATALOG_BYTES,
        target_catalog_bytes: TARGET_CATALOG_BYTES,
        source_environment: &source_environment,
        target_environment: &target_environment,
        compensation_environment: &compensation_environment,
    };
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut source = image("source", SOURCE_SERVER, BASELINE_REGISTRY_BYTES, &events);
    let mut target = image("target", TARGET_SERVER, TARGET_REGISTRY_BYTES, &events);
    let mut compensation = image(
        "compensation",
        COMPENSATION_SERVER,
        BASELINE_REGISTRY_BYTES,
        &events,
    );
    let mut verifier = FixtureVerifier {
        events: Rc::clone(&events),
    };
    let mut codec = fixture_codec();

    let actual = verify_family_release_images(
        &input,
        &mut source,
        &mut target,
        &mut compensation,
        &mut verifier,
        &mut codec,
        &FixtureTime,
    )
    .expect("release image identities");

    assert_eq!(actual.target_family.as_str(), TARGET_FAMILY_SHA256);
    assert_eq!(
        actual.compensation_family.as_str(),
        COMPENSATION_FAMILY_SHA256,
    );
    assert_eq!(actual.target_images.as_str(), TARGET_IMAGE_SHA256);
    assert_eq!(
        actual.compensation_images.as_str(),
        COMPENSATION_IMAGE_SHA256,
    );
    assert_eq!(
        events.borrow().as_slice(),
        [
            Event::Verify {
                image: TARGET_SERVER.to_owned(),
                bytes: TARGET_CATALOG_BYTES.to_vec(),
            },
            Event::Verify {
                image: COMPENSATION_SERVER.to_owned(),
                bytes: BASELINE_CATALOG_BYTES.to_vec(),
            },
            Event::Read {
                owner: "source",
                path: REGISTRY_PATH.to_owned(),
                maximum: REGISTRY_MAXIMUM,
            },
            Event::Read {
                owner: "target",
                path: REGISTRY_PATH.to_owned(),
                maximum: REGISTRY_MAXIMUM,
            },
            Event::Read {
                owner: "compensation",
                path: REGISTRY_PATH.to_owned(),
                maximum: REGISTRY_MAXIMUM,
            },
        ],
    );
}

/// Reject changed compensation bytes after reads but before any identity hashing.
#[test]
fn rejects_compensation_registry_mismatch_before_hashing() {
    let source_environment = environment(SOURCE_SERVER, SOURCE_WEB);
    let target_environment = environment(TARGET_SERVER, TARGET_WEB);
    let compensation_environment = environment(COMPENSATION_SERVER, COMPENSATION_WEB);
    let input = FamilyReleaseImagesInput {
        family: "temperature",
        baseline_catalog_bytes: BASELINE_CATALOG_BYTES,
        target_catalog_bytes: TARGET_CATALOG_BYTES,
        source_environment: &source_environment,
        target_environment: &target_environment,
        compensation_environment: &compensation_environment,
    };
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut source = image("source", SOURCE_SERVER, BASELINE_REGISTRY_BYTES, &events);
    let mut target = image("target", TARGET_SERVER, TARGET_REGISTRY_BYTES, &events);
    let mut compensation = image("compensation", COMPENSATION_SERVER, b"changed\n", &events);
    let mut verifier = FixtureVerifier {
        events: Rc::clone(&events),
    };
    let mut codec = fixture_codec();

    let actual = verify_family_release_images(
        &input,
        &mut source,
        &mut target,
        &mut compensation,
        &mut verifier,
        &mut codec,
        &FixtureTime,
    );

    assert_eq!(actual, Err(MaintenanceAnchorStatusError::Slot));
    assert!(codec.hashed.is_empty());
    assert_eq!(events.borrow().len(), 5);
}

/// Refuse a target image-identity change before family hashing or compiled verification.
#[test]
fn target_rebind_refuses_image_identity_change_before_verification() {
    let target_environment = environment(TARGET_SERVER, TARGET_WEB);
    let input = FamilyReleaseRebindInput {
        family: "temperature",
        catalog_bytes: TARGET_CATALOG_BYTES,
        environment: &target_environment,
        expected_family_sha256: TARGET_FAMILY_SHA256,
        expected_images_sha256: "0000000000000000000000000000000000000000000000000000000000000000",
    };
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut target = image("target", TARGET_SERVER, TARGET_REGISTRY_BYTES, &events);
    let mut verifier = FixtureVerifier {
        events: Rc::clone(&events),
    };
    let mut codec = fixture_codec();

    let actual =
        verify_family_release_rebind(&input, &mut target, &mut verifier, &mut codec, &FixtureTime);

    assert_eq!(actual, Err(MaintenanceAnchorStatusError::Slot));
    assert_eq!(codec.hashed, [TARGET_IMAGE_BYTES]);
    assert_eq!(
        events.borrow().as_slice(),
        [Event::Read {
            owner: "target",
            path: REGISTRY_PATH.to_owned(),
            maximum: REGISTRY_MAXIMUM,
        }],
    );
}

/// Refuse a reader bound to a different image before verification or member reads.
#[test]
fn rebind_refuses_a_different_bound_image_before_observation() {
    let target_environment = environment(TARGET_SERVER, TARGET_WEB);
    let input = FamilyReleaseRebindInput {
        family: "temperature",
        catalog_bytes: TARGET_CATALOG_BYTES,
        environment: &target_environment,
        expected_family_sha256: TARGET_FAMILY_SHA256,
        expected_images_sha256: TARGET_IMAGE_SHA256,
    };
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut target = image(
        "target",
        COMPENSATION_SERVER,
        TARGET_REGISTRY_BYTES,
        &events,
    );
    let mut verifier = FixtureVerifier {
        events: Rc::clone(&events),
    };
    let mut codec = fixture_codec();

    let actual =
        verify_family_release_rebind(&input, &mut target, &mut verifier, &mut codec, &FixtureTime);

    assert_eq!(actual, Err(MaintenanceAnchorStatusError::Slot));
    assert!(events.borrow().is_empty());
    assert!(codec.hashed.is_empty());
}

/// Refuse changed compensation family state before compiled verification.
#[test]
fn compensation_rebind_refuses_family_identity_change_before_verification() {
    let compensation_environment = environment(COMPENSATION_SERVER, COMPENSATION_WEB);
    let input = FamilyReleaseRebindInput {
        family: "temperature",
        catalog_bytes: BASELINE_CATALOG_BYTES,
        environment: &compensation_environment,
        expected_family_sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        expected_images_sha256: COMPENSATION_IMAGE_SHA256,
    };
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut compensation = image(
        "compensation",
        COMPENSATION_SERVER,
        BASELINE_REGISTRY_BYTES,
        &events,
    );
    let mut verifier = FixtureVerifier {
        events: Rc::clone(&events),
    };
    let mut codec = fixture_codec();

    let actual = verify_family_release_rebind(
        &input,
        &mut compensation,
        &mut verifier,
        &mut codec,
        &FixtureTime,
    );

    assert_eq!(actual, Err(MaintenanceAnchorStatusError::Slot));
    assert_eq!(
        codec.hashed,
        [
            COMPENSATION_IMAGE_BYTES,
            BASELINE_REGISTRY_BYTES,
            COMPENSATION_FAMILY_BYTES,
        ],
    );
    assert_eq!(
        events.borrow().as_slice(),
        [Event::Read {
            owner: "compensation",
            path: REGISTRY_PATH.to_owned(),
            maximum: REGISTRY_MAXIMUM,
        }],
    );
}

/// Rebind through the real bounded process owner before compiled verification.
#[test]
fn successful_rebind_uses_the_native_process_reader() {
    let fixture = FixtureDirectory::new();
    let program = write_registry_program(fixture.path());
    let target_environment = environment(TARGET_SERVER, TARGET_WEB);
    let input = FamilyReleaseRebindInput {
        family: "temperature",
        catalog_bytes: TARGET_CATALOG_BYTES,
        environment: &target_environment,
        expected_family_sha256: TARGET_FAMILY_SHA256,
        expected_images_sha256: TARGET_IMAGE_SHA256,
    };
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut target = NativeFamilyTargetImage::for_fixture_program(TARGET_SERVER, &program)
        .expect("valid target image fixture");
    let mut verifier = FixtureVerifier {
        events: Rc::clone(&events),
    };
    let mut codec = fixture_codec();

    verify_family_release_rebind(&input, &mut target, &mut verifier, &mut codec, &FixtureTime)
        .expect("native process rebind");

    assert_eq!(
        codec.hashed,
        [
            TARGET_IMAGE_BYTES,
            TARGET_REGISTRY_BYTES,
            TARGET_FAMILY_BYTES
        ],
    );
    assert_eq!(
        events.borrow().as_slice(),
        [Event::Verify {
            image: TARGET_SERVER.to_owned(),
            bytes: TARGET_CATALOG_BYTES.to_vec(),
        }],
    );
}
