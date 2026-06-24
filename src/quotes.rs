//! Iconic catchphrases of Gregorio Sánchez "Chiquito de la Calzada"
//! (1932–2017), the Málaga-born comedian jarl is named after.
//!
//! These are short standalone catchphrases/exclamations (his comedic style
//! relied on them peppered throughout longer routines), reproduced here as
//! widely-known, repeatedly-quoted public expressions — not transcriptions
//! of any single copyrighted routine or written work. Sourced and
//! cross-checked against multiple independent retrospectives published
//! after his passing.
//!
//! Displayed in the header's now-playing box as a rotating tribute when no
//! live track title is available. Each entry is pre-split into one or two
//! short lines to fit the limited space there.

/// A quote, pre-split into 1–2 short display lines.
pub struct Quote {
    pub lines: &'static [&'static str],
}

pub const QUOTES: &[Quote] = &[
    Quote { lines: &["¡Fistro!"] },
    Quote { lines: &["¡Pecador!"] },
    Quote { lines: &["¡Cobarde!"] },
    Quote { lines: &["¿Te da cuén?"] },
    Quote { lines: &["Hasta luego, Lucas"] },
    Quote { lines: &["Por la gloria de mi madre"] },
    Quote { lines: &["¡Al ataquerl!"] },
    Quote { lines: &["¡Jarl!"] },
    Quote { lines: &["¡Quietoorl!"] },
    Quote { lines: &["¡Torpedo!"] },
    Quote { lines: &["La caidita de Roma"] },
    Quote { lines: &["Diodenal"] },
    Quote { lines: &["Gromenauer"] },
    Quote { lines: &["A Candemor"] },
    Quote { lines: &["¿Cómor?"] },
    Quote { lines: &["Meretérica"] },
    Quote { lines: &["Grijandemore"] },
];

/// Picks a pseudo-random quote without pulling in a `rand` dependency,
/// reusing the same low-stakes entropy source as the reconnect jitter in
/// `player.rs` (sub-second clock component).
pub fn random_quote() -> &'static Quote {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let idx = (nanos as usize) % QUOTES.len();
    &QUOTES[idx]
}
