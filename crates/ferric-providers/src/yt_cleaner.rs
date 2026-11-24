use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use std::sync::OnceLock;

const NOISE_PATTERNS: &[&str] = &[
    "(Official Music Video)",
    "(Official Video)",
    "(Official Audio)",
    "(Official Lyric Video)",
    "[Official Music Video]",
    "[Official Video]",
    "[Official Audio]",
    "[Lyric Video]",
    "(Lyric Video)",
    "(Lyrics)",
    "[Lyrics]",
    "(Visualizer)",
    "[Visualizer]",
    "[MV]",
    "(MV)",
    "(HD)",
    "(4K)",
    "[4K]",
    "Official Music Video",
    "Official Audio",
];

fn noise_matcher() -> &'static AhoCorasick {
    static AC: OnceLock<AhoCorasick> = OnceLock::new();
    AC.get_or_init(|| {
        AhoCorasickBuilder::new()
            .ascii_case_insensitive(true)
            .build(NOISE_PATTERNS)
            .expect("valid static noise patterns")
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanedYouTubeTrack {
    pub title: String,
    pub artist: String,
    pub is_yt_music_topic: bool,
}

/// Deterministic zero-regex metadata cleaner for YouTube & YouTube Music catalog titles.
/// Preserves intentional version tags like `(Acoustic)`, `(Live)`, or `(Remix)`.
pub fn clean_youtube_metadata(raw_title: &str, channel_title: &str) -> CleanedYouTubeTrack {
    let trimmed_channel = channel_title.trim();
    let (channel_artist, is_topic) = if let Some(base) = trimmed_channel.strip_suffix(" - Topic") {
        (base.trim().to_string(), true)
    } else {
        (trimmed_channel.to_string(), false)
    };

    let stripped = noise_matcher().replace_all(raw_title, &[""; NOISE_PATTERNS.len()]);
    let collapsed = collapse_whitespace(&stripped);

    if is_topic {
        return CleanedYouTubeTrack {
            title: collapsed,
            artist: channel_artist,
            is_yt_music_topic: true,
        };
    }

    for sep in [" - ", " – ", " — "] {
        if let Some((left, right)) = collapsed.split_once(sep) {
            let left_clean = left.trim();
            let right_clean = right.trim();
            if !left_clean.is_empty() && !right_clean.is_empty() {
                return CleanedYouTubeTrack {
                    title: right_clean.to_string(),
                    artist: left_clean.to_string(),
                    is_yt_music_topic: false,
                };
            }
        }
    }

    CleanedYouTubeTrack {
        title: collapsed,
        artist: if channel_artist.is_empty() {
            "Unknown Artist".to_string()
        } else {
            channel_artist
        },
        is_yt_music_topic: false,
    }
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.trim_matches(|c| c == '-' || c == '|' || c == ' ')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_topic_channel_and_preserves_acoustic_tag() {
        let cleaned = clean_youtube_metadata(
            "Hati-Hati di Jalan (Acoustic) (Official Audio)",
            "Tulus - Topic",
        );
        assert!(cleaned.is_yt_music_topic);
        assert_eq!(cleaned.artist, "Tulus");
        assert_eq!(cleaned.title, "Hati-Hati di Jalan (Acoustic)");
    }

    #[test]
    fn splits_standard_youtube_artist_title_and_strips_mv_noise() {
        let cleaned = clean_youtube_metadata(
            "Daft Punk - Instant Crush (Official Music Video) [4K]",
            "DaftPunkVEVO",
        );
        assert!(!cleaned.is_yt_music_topic);
        assert_eq!(cleaned.artist, "Daft Punk");
        assert_eq!(cleaned.title, "Instant Crush");
    }
}
