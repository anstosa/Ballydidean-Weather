//! Bind installed family state and immutable application images to release identities.

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use crate::MaintenanceAnchorStatusError;
use crate::family_target_catalog::{catalog_entries, object, string};
use crate::family_target_catalog_validation::V1;
use crate::maintenance_canonical_json::{MaintenanceCanonicalJson, exact_keys, field, text};

use MaintenanceAnchorStatusError::{Codec, Slot};

const FAMILIES: &[&str] = &["temperature", "wind", "rain"];
const SLOTS: &[&str] = &["active", "shadow", "control"];
const V1_ENTRY_KEYS: &[&str] = &["family", "receipt", "registration"];
const SLOTTED_ENTRY_KEYS: &[&str] = &["family", "receipt", "registration", "slot"];

/// Hash the exact registry and installed slot receipts for one adjustment family.
pub fn compute_family_release_identity(
    family: &str,
    registry_bytes: &[u8],
    catalog: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<Sha256Digest, MaintenanceAnchorStatusError> {
    let active = installed_family_receipt_sha256(catalog, family, "active", codec)?;
    // retain the independent control identity only for rain
    let control = if family == "rain" {
        installed_family_receipt_sha256(catalog, family, "control", codec)?
    } else {
        None
    };
    let registry = codec.sha256(registry_bytes).ok_or(Codec)?;
    let shadow = installed_family_receipt_sha256(catalog, family, "shadow", codec)?;
    let identity = object(vec![
        ("activeInstalledReceiptSha256", optional_digest(&active)),
        ("controlInstalledReceiptSha256", optional_digest(&control)),
        ("family", string(family)),
        ("registrySha256", string(registry.as_str())),
        ("shadowInstalledReceiptSha256", optional_digest(&shadow)),
    ]);
    let bytes = codec.encode_canonical_json(&identity).ok_or(Codec)?;
    codec.sha256(&bytes).ok_or(Codec)
}

/// Hash the immutable server and web image digests in their fixed runtime order.
pub fn compute_family_release_image_identity(
    environment: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<Sha256Digest, MaintenanceAnchorStatusError> {
    let server = image_digest(environment, "WEATHER_SERVER_IMAGE")?;
    let web = image_digest(environment, "WEATHER_WEB_IMAGE")?;
    let identity = JsonValue::Array(vec![
        object(vec![
            ("digest", string(&server)),
            ("runtime", string("server")),
        ]),
        object(vec![("digest", string(&web)), ("runtime", string("web"))]),
    ]);
    let bytes = codec.encode_canonical_json(&identity).ok_or(Codec)?;
    codec.sha256(&bytes).ok_or(Codec)
}

// hash the exact selected receipt while retaining explicit absence
fn installed_family_receipt_sha256(
    catalog: &JsonValue,
    family: &str,
    slot: &str,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<Option<Sha256Digest>, MaintenanceAnchorStatusError> {
    // preserve the original family and slot admission boundary
    if !FAMILIES.contains(&family)
        || !SLOTS.contains(&slot)
        || slot == "control" && family != "rain"
    {
        return Err(Slot);
    }
    let contract = text(catalog, "contractVersion").ok_or(Slot)?;
    let legacy = contract == V1;
    // v1 entries represent only the implicit shadow slot
    let entry = catalog_entries(catalog)?.iter().find(|candidate| {
        text(candidate, "family").as_deref() == Some(family)
            && if legacy {
                slot == "shadow"
            } else {
                text(candidate, "slot").as_deref() == Some(slot)
            }
    });
    let Some(entry) = entry else {
        return Ok(None);
    };
    let keys = if legacy {
        V1_ENTRY_KEYS
    } else {
        SLOTTED_ENTRY_KEYS
    };
    // reject extensions only on the selected installed entry
    if !exact_keys(entry, keys) {
        return Err(Slot);
    }
    let receipt = field(entry, "receipt").ok_or(Slot)?;
    let bytes = codec.encode_canonical_json(receipt).ok_or(Codec)?;
    Ok(Some(codec.sha256(&bytes).ok_or(Codec)?))
}

// retain a missing installed receipt as canonical null
fn optional_digest(value: &Option<Sha256Digest>) -> JsonValue {
    value
        .as_ref()
        .map_or(JsonValue::Null, |digest| string(digest.as_str()))
}

// preserve javascript split("@")[1] rather than selecting the final segment
fn image_digest(
    environment: &JsonValue,
    key: &str,
) -> Result<String, MaintenanceAnchorStatusError> {
    let reference = text(environment, key).ok_or(Slot)?;
    reference.split('@').nth(1).map(str::to_owned).ok_or(Slot)
}

#[cfg(test)]
mod tests {
    use weather_contracts::JsonValue;
    use weather_database::Sha256Digest;

    use super::{compute_family_release_identity, compute_family_release_image_identity};
    use crate::family_target_catalog::{object, string};
    use crate::family_target_catalog_validation::{V1, V3};
    use crate::maintenance_canonical_json::MaintenanceCanonicalJson;

    const ACTIVE_BYTES: &[u8] = b"{\"kind\":\"active\"}\n";
    const ACTIVE_SHA256: &str = "1ce498110801821a51dfdf7db5380fc9d17d32b07b217379417ea7892c1eb950";
    const CONTROL_BYTES: &[u8] = b"{\"kind\":\"control\"}\n";
    const CONTROL_SHA256: &str = "862aee01e2ffbfc0d64d93cd3914239837803d02894f6e8d057141ff6e6b1473";
    const SHADOW_BYTES: &[u8] = b"{\"kind\":\"shadow\"}\n";
    const SHADOW_SHA256: &str = "ab2574d10be0b0687ff18401d649d4e2971d88bab78f663b9456b0d2e976af12";
    const REGISTRY_BYTES: &[u8] = b"registry\n";
    const REGISTRY_SHA256: &str =
        "a2782aa6fc01988c16bb63c7cd88d694790db18dd7ae101967ce118bd7d701e4";
    const RAIN_IDENTITY_BYTES: &[u8] = b"{\"activeInstalledReceiptSha256\":\"1ce498110801821a51dfdf7db5380fc9d17d32b07b217379417ea7892c1eb950\",\"controlInstalledReceiptSha256\":\"862aee01e2ffbfc0d64d93cd3914239837803d02894f6e8d057141ff6e6b1473\",\"family\":\"rain\",\"registrySha256\":\"a2782aa6fc01988c16bb63c7cd88d694790db18dd7ae101967ce118bd7d701e4\",\"shadowInstalledReceiptSha256\":\"ab2574d10be0b0687ff18401d649d4e2971d88bab78f663b9456b0d2e976af12\"}\n";
    const RAIN_IDENTITY_SHA256: &str =
        "9e32cb351adb691b25d320ffd144c54435c5c58d2cb13875b4178cea2ec114e0";
    const LEGACY_BYTES: &[u8] = b"{\"kind\":\"legacy\"}\n";
    const LEGACY_SHA256: &str = "6f452b952f3029e602e742815db952d057dbb4aaf063f5ffdd53c086c7a0e040";
    const LEGACY_REGISTRY_BYTES: &[u8] = b"legacy-registry\n";
    const LEGACY_REGISTRY_SHA256: &str =
        "2b4dd02b2e62f64f5bf9216f5f922213b0d9e027bbd551a5ea7e95f375e9364f";
    const LEGACY_IDENTITY_BYTES: &[u8] = b"{\"activeInstalledReceiptSha256\":null,\"controlInstalledReceiptSha256\":null,\"family\":\"temperature\",\"registrySha256\":\"2b4dd02b2e62f64f5bf9216f5f922213b0d9e027bbd551a5ea7e95f375e9364f\",\"shadowInstalledReceiptSha256\":\"6f452b952f3029e602e742815db952d057dbb4aaf063f5ffdd53c086c7a0e040\"}\n";
    const LEGACY_IDENTITY_SHA256: &str =
        "e3b498b73ab13b29638f7c35295e7498acf7dc187d4ca28dc05cda7c78e642d0";
    const IMAGE_BYTES: &[u8] = b"[{\"digest\":\"sha256:server\",\"runtime\":\"server\"},{\"digest\":\"sha256:web\",\"runtime\":\"web\"}]\n";
    const IMAGE_SHA256: &str = "32259bae2448330fab9738487c913a3765faf65aab8432edac95a002fbf28dfa";

    struct FixtureCodec {
        encodings: Vec<(JsonValue, &'static [u8])>,
        hashes: Vec<(&'static [u8], &'static str)>,
        encoded: Vec<Vec<u8>>,
    }

    impl MaintenanceCanonicalJson for FixtureCodec {
        /// Leave parsing outside these already-decoded fixtures.
        fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
            None
        }

        /// Emit only exact canonical preimages captured from Node.
        fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
            let bytes = self
                .encodings
                .iter()
                .find_map(|(expected, bytes)| (expected == value).then_some(*bytes))?
                .to_vec();
            self.encoded.push(bytes.clone());
            Some(bytes)
        }

        /// Return only Node-computed fixture digests for exact preimages.
        fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
            self.hashes.iter().find_map(|(preimage, digest)| {
                (*preimage == bytes).then(|| Sha256Digest::admitted(*digest))
            })
        }
    }

    // build one exact installed entry for the selected catalog generation
    fn entry(family: &str, slot: Option<&str>, kind: &str) -> JsonValue {
        let mut fields = vec![
            ("family", string(family)),
            ("receipt", object(vec![("kind", string(kind))])),
            ("registration", object(vec![])),
        ];
        // add only the post-v1 explicit slot
        if let Some(slot) = slot {
            fields.push(("slot", string(slot)));
        }
        object(fields)
    }

    /// Match every rain slot and the final identity to Node's exact preimages.
    #[test]
    fn hashes_slotted_rain_release_identity() {
        let catalog = object(vec![
            ("contractVersion", string(V3)),
            (
                "entries",
                JsonValue::Array(vec![
                    entry("rain", Some("shadow"), "shadow"),
                    entry("rain", Some("active"), "active"),
                    entry("rain", Some("control"), "control"),
                ]),
            ),
        ]);
        let identity = object(vec![
            ("activeInstalledReceiptSha256", string(ACTIVE_SHA256)),
            ("controlInstalledReceiptSha256", string(CONTROL_SHA256)),
            ("family", string("rain")),
            ("registrySha256", string(REGISTRY_SHA256)),
            ("shadowInstalledReceiptSha256", string(SHADOW_SHA256)),
        ]);
        let mut codec = FixtureCodec {
            encodings: vec![
                (object(vec![("kind", string("active"))]), ACTIVE_BYTES),
                (object(vec![("kind", string("control"))]), CONTROL_BYTES),
                (object(vec![("kind", string("shadow"))]), SHADOW_BYTES),
                (identity, RAIN_IDENTITY_BYTES),
            ],
            hashes: vec![
                (ACTIVE_BYTES, ACTIVE_SHA256),
                (CONTROL_BYTES, CONTROL_SHA256),
                (REGISTRY_BYTES, REGISTRY_SHA256),
                (SHADOW_BYTES, SHADOW_SHA256),
                (RAIN_IDENTITY_BYTES, RAIN_IDENTITY_SHA256),
            ],
            encoded: Vec::new(),
        };

        let actual = compute_family_release_identity("rain", REGISTRY_BYTES, &catalog, &mut codec)
            .expect("rain release identity");

        assert_eq!(actual.as_str(), RAIN_IDENTITY_SHA256);
        assert_eq!(
            codec.encoded.last().map(Vec::as_slice),
            Some(RAIN_IDENTITY_BYTES)
        );
    }

    /// Preserve v1's implicit shadow and explicit nulls for absent slots.
    #[test]
    fn hashes_legacy_shadow_only_release_identity() {
        let catalog = object(vec![
            ("contractVersion", string(V1)),
            (
                "entries",
                JsonValue::Array(vec![entry("temperature", None, "legacy")]),
            ),
        ]);
        let identity = object(vec![
            ("activeInstalledReceiptSha256", JsonValue::Null),
            ("controlInstalledReceiptSha256", JsonValue::Null),
            ("family", string("temperature")),
            ("registrySha256", string(LEGACY_REGISTRY_SHA256)),
            ("shadowInstalledReceiptSha256", string(LEGACY_SHA256)),
        ]);
        let mut codec = FixtureCodec {
            encodings: vec![
                (object(vec![("kind", string("legacy"))]), LEGACY_BYTES),
                (identity, LEGACY_IDENTITY_BYTES),
            ],
            hashes: vec![
                (LEGACY_REGISTRY_BYTES, LEGACY_REGISTRY_SHA256),
                (LEGACY_BYTES, LEGACY_SHA256),
                (LEGACY_IDENTITY_BYTES, LEGACY_IDENTITY_SHA256),
            ],
            encoded: Vec::new(),
        };

        let actual = compute_family_release_identity(
            "temperature",
            LEGACY_REGISTRY_BYTES,
            &catalog,
            &mut codec,
        )
        .expect("legacy release identity");

        assert_eq!(actual.as_str(), LEGACY_IDENTITY_SHA256);
        assert_eq!(codec.encoded, vec![LEGACY_BYTES, LEGACY_IDENTITY_BYTES]);
    }

    /// Bind both runtime digests while preserving split-at index one.
    #[test]
    fn hashes_release_image_identity() {
        let environment = object(vec![
            (
                "WEATHER_SERVER_IMAGE",
                string("registry.example/server@sha256:server@ignored"),
            ),
            (
                "WEATHER_WEB_IMAGE",
                string("registry.example/web@sha256:web"),
            ),
        ]);
        let image_identity = JsonValue::Array(vec![
            object(vec![
                ("digest", string("sha256:server")),
                ("runtime", string("server")),
            ]),
            object(vec![
                ("digest", string("sha256:web")),
                ("runtime", string("web")),
            ]),
        ]);
        let mut codec = FixtureCodec {
            encodings: vec![(image_identity, IMAGE_BYTES)],
            hashes: vec![(IMAGE_BYTES, IMAGE_SHA256)],
            encoded: Vec::new(),
        };

        let actual = compute_family_release_image_identity(&environment, &mut codec)
            .expect("image identity");

        assert_eq!(actual.as_str(), IMAGE_SHA256);
        assert_eq!(codec.encoded, vec![IMAGE_BYTES]);
    }
}
