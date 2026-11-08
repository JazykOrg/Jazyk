//! Id slugs and id hashes: the parts every minted id is built from.
//! Mirrors docs/compiler/model.md#identifiers.

// The longest slug part of an id.
pub const SLUG_MAX: usize = 32;

// Crockford base 32, lowercase: no `i`, `l`, `o`, `u`.
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

// The prefix lengths of the id hash tried in turn when a shorter one is taken.
pub const HASH_LENGTHS: [usize; 4] = [4, 6, 8, 13];

// The id slug of a text: transliterated to ASCII, lowercased, every run of other
// characters written as one `-`, trimmed, cut to SLUG_MAX at the last `-` that fits
// (a single longer word is cut at SLUG_MAX). Empty is `x`.
pub fn slug(text: &str) -> String {
    let ascii = deunicode::deunicode(text).to_lowercase();
    let mut out = String::with_capacity(ascii.len());
    let mut dash = false;
    for c in ascii.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-');
    let cut = if out.len() <= SLUG_MAX {
        out
    } else {
        let head = &out[..SLUG_MAX];
        match head.rfind('-') {
            // A boundary right after the cut keeps the whole last word.
            _ if out.as_bytes()[SLUG_MAX] == b'-' => head,
            Some(i) if i > 0 => &head[..i],
            _ => head,
        }
    };
    let cut = cut.trim_end_matches('-');
    if cut.is_empty() {
        "x".to_string()
    } else {
        cut.to_string()
    }
}

// The slug part of an entity id: `ent:order` → `order`, the name every per-entity
// path and derived id carries (`sm:order`, `view:state/order`, the ledger key, the
// card page). An id without the prefix is its own slug.
pub fn entity_slug(id: &str) -> &str {
    id.strip_prefix("ent:").unwrap_or(id)
}

// 64-bit FNV-1a, the hash file ids use too.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// The whole id hash of a key: 13 base-32 characters, most significant first.
pub fn hash_full(key: &str) -> String {
    let mut h = fnv1a64(key.as_bytes());
    let mut digits = [0u8; 13];
    for d in digits.iter_mut().rev() {
        *d = ALPHABET[(h & 31) as usize];
        h >>= 5;
    }
    // The top digit holds the remaining four bits; spread it so short prefixes vary.
    digits.rotate_left(1);
    String::from_utf8(digits.to_vec()).unwrap_or_default()
}

// The four-character id hash of a key.
pub fn hash(key: &str) -> String {
    hash_full(key)[..HASH_LENGTHS[0]].to_string()
}

// The first id `<base>-<hash>` not taken: the hash prefixes in turn, then the full
// hash with `-2`, `-3`, and so on.
pub fn unique(base: &str, key: &str, taken: impl Fn(&str) -> bool) -> String {
    let full = hash_full(key);
    for n in HASH_LENGTHS {
        let id = format!("{}-{}", base, &full[..n]);
        if !taken(&id) {
            return id;
        }
    }
    let mut n = 2;
    loop {
        let id = format!("{}-{}-{}", base, full, n);
        if !taken(&id) {
            return id;
        }
        n += 1;
    }
}

// The plain id when free, else the first free hashed one.
pub fn plain_or_unique(plain: &str, key: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(plain) {
        return plain.to_string();
    }
    unique(plain, key, taken)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_slug_drops_the_entity_prefix_only() {
        assert_eq!(entity_slug("ent:order"), "order");
        assert_eq!(entity_slug("order"), "order");
        assert_eq!(entity_slug("req:order-1"), "req:order-1");
    }

    #[test]
    fn slug_collapses_runs_transliterates_and_bounds_length() {
        assert_eq!(slug("aaa       #$%^  bbb"), "aaa-bbb");
        assert_eq!(slug("  --Hello__World--  "), "hello-world");
        assert_eq!(slug("Čajovňa U Zlatého"), "cajovna-u-zlateho");
        assert_eq!(
            slug("Harbor redevelopment conflict: Characters"),
            "harbor-redevelopment-conflict"
        );
        assert_eq!(slug("a".repeat(40).as_str()), "a".repeat(32));
        assert_eq!(slug("#$%"), "x");
        assert_eq!(slug(""), "x");
        // A word ending exactly at the limit stays whole.
        let exact = format!("{} {}", "a".repeat(10), "b".repeat(21));
        assert_eq!(
            slug(&format!("{} more", exact)),
            format!("{}-{}", "a".repeat(10), "b".repeat(21))
        );
        for s in [
            "Order Service API",
            "Ines's stance — four",
            "日本語のテキスト",
        ] {
            let out = slug(s);
            assert!(out.len() <= SLUG_MAX);
            assert!(out
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'));
            assert!(!out.contains("--") && !out.starts_with('-') && !out.ends_with('-'));
        }
    }

    #[test]
    fn hashes_are_stable_short_and_extend_on_collision() {
        let h = hash("docs/a.md#/a\nthe cart holds items");
        assert_eq!(h.len(), 4);
        assert_eq!(h, hash("docs/a.md#/a\nthe cart holds items"));
        assert_ne!(h, hash("docs/a.md#/a\nthe cart holds orders"));
        assert!(h.bytes().all(|b| ALPHABET.contains(&b)));
        let first = unique("req:a", "k", |_| false);
        assert_eq!(first.len(), "req:a-".len() + 4);
        let second = unique("req:a", "k", |id| id == first);
        assert_eq!(second.len(), "req:a-".len() + 6);
        assert!(second.starts_with(&first));
        let last = unique("req:a", "k", |id| !id.ends_with("-2"));
        assert!(last.ends_with("-2"));
        assert_eq!(plain_or_unique("ent:order", "k", |_| false), "ent:order");
        assert_ne!(
            plain_or_unique("ent:order", "k", |id| id == "ent:order"),
            "ent:order"
        );
    }
}
