//! Exact low-level value classes shared by the revision archive owners.

/// Decode canonical timestamps and hash bytes with the admitted runtime implementations.
pub trait RevisionArchiveCanonicalCodec {
    /// Admit one strict canonical UTC timestamp as milliseconds.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64>;

    /// Hash the exact supplied canonical byte sequence.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32];
}

/// Clock owner for a new immutable archive receipt.
pub trait RevisionArchiveCanonicalClock {
    /// Return the current canonical archive instant.
    fn canonical_now(&mut self) -> Option<String>;
}

/// Fence fixed-json interpolation before asking the admitted UTC parser.
pub(crate) fn instant(codec: &impl RevisionArchiveCanonicalCodec, value: &str) -> Option<i64> {
    // preserve both normal and extended-year ISO forms from the source UTC codec
    let mask: &[u8] = match value.len() {
        24 => b"dddd-dd-ddTdd:dd:dd.dddZ",
        27 => b"sdddddd-dd-ddTdd:dd:dd.dddZ",
        _ => return None,
    };
    // keep punctuation fixed before calling the actual canonical-date owner
    if !value.bytes().zip(mask).all(
        // require ascii digits and the standard extended-year sign
        |(byte, expected)| match expected {
            b'd' => byte.is_ascii_digit(),
            b's' => matches!(byte, b'+' | b'-'),
            punctuation => byte == *punctuation,
        },
    ) {
        return None;
    }
    codec.canonical_utc_milliseconds(value)
}

/// Match the canonical identity of one content-addressed archive basename.
pub(crate) fn is_payload_name(name: &str) -> bool {
    name.strip_prefix("sha256-")
        .and_then(
            // require the exact retained object suffix
            |digest| digest.strip_suffix(".json"),
        )
        .is_some_and(is_sha256)
}

/// Match only lowercase hexadecimal SHA-256 text.
pub(crate) fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text.bytes().all(
            // preserve Node's ascii lowercase SHA-256 alphabet
            |byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        )
}

/// Render digest bytes without adding a formatting or cryptography package.
pub(crate) fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    // emit each nibble in the exact lowercase wire alphabet
    for byte in digest {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0xf)]));
    }
    value
}
