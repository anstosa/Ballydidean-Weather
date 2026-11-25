//! Printable ASCII collation compatibility with the pinned Node en-US default Intl collator.
//!
//! Non-ASCII and control characters cannot be honestly assigned source weights by this
//! finite profile; callers refuse unsupported paths rather than publish a different tree hash.

use std::cmp::Ordering;

const PRIMARY: &[u8] = b" _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789abcdefghijklmnopqrstuvwxyz";

const RANKS: [u8; 128] = primary_ranks();

/// Bind each admitted byte once at compile time instead of walking the table per comparison.
const fn primary_ranks() -> [u8; 128] {
    let mut ranks = [0; 128];
    let mut index = 0;
    // retain the same exact primary table for lower and upper-case letters
    while index < PRIMARY.len() {
        let byte = PRIMARY[index];
        ranks[byte as usize] = index as u8;
        if byte >= b'a' && byte <= b'z' {
            ranks[(byte - b'a' + b'A') as usize] = index as u8;
        }
        index += 1;
    }
    ranks
}

/// Check the exact finite filename profile independently proved against Node.
pub(crate) fn native_fit_collation_admits(value: &str) -> bool {
    value.bytes().all(|byte| (b' '..=b'~').contains(&byte))
}

/// Compare admitted paths using primary ASCII weights followed by lower-first case weights.
pub(crate) fn native_fit_collation_cmp(a: &str, b: &str) -> Ordering {
    debug_assert!(native_fit_collation_admits(a) && native_fit_collation_admits(b));
    let primary = a
        .bytes()
        .map(primary_weight)
        .cmp(b.bytes().map(primary_weight));
    // compare the full primary sequence before examining tertiary case differences
    if primary != Ordering::Equal {
        return primary;
    }
    a.bytes()
        .map(|byte| byte.is_ascii_uppercase())
        .cmp(b.bytes().map(|byte| byte.is_ascii_uppercase()))
}

/// Map one ASCII byte to its exact contracted pinned primary weight.
fn primary_weight(byte: u8) -> u8 {
    RANKS[byte as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Compare mixed punctuation and case pairs with the pinned Node default collator.
    #[test]
    fn native_fit_ascii_collation_matches_pinned_node_path_order() {
        let mut state = 0x7472_6163u32;
        let mut paths = vec![
            "_a".to_owned(),
            "-a".to_owned(),
            ".a".to_owned(),
            "@a".to_owned(),
            "a".to_owned(),
            "A".to_owned(),
        ];
        // build a deterministic finite ASCII path population for complete order comparison
        for _ in 0..2048 {
            let length = 1 + (state % 29);
            let mut path = String::new();
            for _ in 0..length {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                path.push(char::from(32 + (state % 95) as u8));
            }
            paths.push(path);
        }
        let input = serde_json::to_string(&paths).unwrap();
        let result=Command::new("node").args(["-e","if(process.version!=='v24.16.0'||Intl.Collator().resolvedOptions().locale!=='en-US')process.exit(2); process.stdout.write(JSON.stringify(JSON.parse(process.argv[1]).sort((a,b)=>a.localeCompare(b))))", &input]).output().unwrap();
        assert!(result.status.success());
        let expected: Vec<String> = serde_json::from_slice(&result.stdout).unwrap();
        paths.sort_by(|a, b| native_fit_collation_cmp(a, b));
        assert_eq!(paths, expected);
    }
}
