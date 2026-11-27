//! Zero-allocation ASCII case-insensitive string matching and comparison utilities.
//!
//! Used across library filters, instant search, playlist/album/artist sorting,
//! AutoEq lookup, and feed parsers so high-frequency UI/search loops avoid
//! temporary `String::to_ascii_lowercase()` heap allocations.

use std::cmp::Ordering;

/// Find the first UTF-8-aligned byte offset of `needle` inside `haystack`
/// using ASCII case-insensitive matching without allocating.
#[inline]
pub fn find_ascii_icase(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.len() > h.len() {
        return None;
    }
    let first_low = n[0].to_ascii_lowercase();
    let last_start = h.len() - n.len();
    for i in 0..=last_start {
        if h[i].to_ascii_lowercase() == first_low
            && h[i..i + n.len()].eq_ignore_ascii_case(n)
            && haystack.is_char_boundary(i)
        {
            return Some(i);
        }
    }
    None
}

/// Zero-allocation ASCII case-insensitive substring check.
#[inline]
pub fn contains_ascii_icase(haystack: &str, needle: &str) -> bool {
    find_ascii_icase(haystack, needle).is_some()
}

/// Zero-allocation ASCII case-insensitive prefix check.
#[inline]
pub fn starts_with_ascii_icase(haystack: &str, prefix: &str) -> bool {
    haystack
        .as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// Zero-allocation ASCII case-insensitive suffix check.
#[inline]
pub fn ends_with_ascii_icase(haystack: &str, suffix: &str) -> bool {
    haystack
        .len()
        .checked_sub(suffix.len())
        .and_then(|start| haystack.as_bytes().get(start..))
        .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix.as_bytes()))
}

/// Zero-allocation ASCII case-insensitive lexicographical comparator.
#[inline]
pub fn cmp_ascii_icase(a: &str, b: &str) -> Ordering {
    for (ba, bb) in a.bytes().zip(b.bytes()) {
        let ord = ba.to_ascii_lowercase().cmp(&bb.to_ascii_lowercase());
        if ord != Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_icase_matchers_handle_empty_mixed_case_and_utf8() {
        assert_eq!(find_ascii_icase("Die With A Smile", ""), Some(0));
        assert_eq!(find_ascii_icase("Die With A Smile", "with a"), Some(4));
        assert!(contains_ascii_icase("Bohemian Rhapsody", "RHAPSODY"));
        assert!(!contains_ascii_icase("Short", "LongerNeedle"));
        assert!(starts_with_ascii_icase("HTTPS://Example.com", "https://"));
        assert!(!starts_with_ascii_icase("ht", "https://"));
        assert!(ends_with_ascii_icase("Bruno Mars Radio", " radio"));
        assert!(!ends_with_ascii_icase("Hi", " radio"));
        assert_eq!(
            find_ascii_icase("Café (Official Video)", " (official"),
            Some(5)
        );
        assert_eq!(cmp_ascii_icase("bruno mars", "Bruno Mars"), Ordering::Equal);
        assert_eq!(cmp_ascii_icase("Alpha", "beta"), Ordering::Less);
        assert_eq!(cmp_ascii_icase("Abc", "ab"), Ordering::Greater);
    }
}
