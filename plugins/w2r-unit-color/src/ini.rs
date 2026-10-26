//! The old `color_profiles.ini` (sections of `playerN = #RRGGBB`), read once to convert it to
//! `color_profiles.json` when only the old file exists. Nothing writes this format any more.
//!
//! ```ini
//! [Classic Colors + Black 2 Pink]
//! author = Maxx444
//! player1 = #A40000 ; Red
//! player6 = #FF00FF ; Black
//! ```

use crate::model::{self, MAX_PROFILES, PLAYERS, Profile};

/// Why one line of the file was skipped; the rest of the file still loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub line: usize,
    pub what: &'static str,
}

/// Every profile in the file, in file order, with what was skipped. A section without a usable name
/// is skipped; a second section with the same name (ignoring case) too.
pub fn parse(text: &str) -> (Vec<Profile>, Vec<Warning>) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out: Vec<Profile> = Vec::new();
    let mut warnings = Vec::new();
    // The section being read; `None` before the first one or inside a skipped one.
    let mut cur: Option<Profile> = None;
    let mut skipping = false;
    let mut warn = |line, what| warnings.push(Warning { line, what });

    // Sections are only opened while there is room, so a finished one always fits.
    let finish = |p: Option<Profile>, out: &mut Vec<Profile>| out.extend(p);

    for (n, raw) in text.lines().enumerate() {
        let line_no = n + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            finish(cur.take(), &mut out);
            skipping = true;
            let Some(end) = rest.rfind(']') else {
                warn(line_no, "section header without ]");
                continue;
            };
            let name = model::clean_name(&rest[..end], "");
            if name.is_empty() {
                warn(line_no, "profile without a name");
            } else if out.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
                warn(line_no, "a profile with this name came earlier");
            } else if out.len() >= MAX_PROFILES {
                warn(line_no, "too many profiles");
            } else {
                skipping = false;
                cur = Some(Profile {
                    name,
                    author: String::new(),
                    colors: [None; PLAYERS],
                    extra: Vec::new(),
                });
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            warn(line_no, "not key = value");
            continue;
        };
        let Some(p) = cur.as_mut() else {
            if !skipping {
                warn(line_no, "setting outside a [profile] section");
            }
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if key == "author" {
            p.author = model::clean_name(value, "");
        } else if let Some(i) = player_index(&key) {
            let value = value.split(';').next().unwrap_or("");
            match model::parse_entry(value) {
                Ok(c) => p.colors[i] = c,
                Err(_) => warn(line_no, "not a #RRGGBB color"),
            }
        }
    }
    finish(cur, &mut out);
    (out, warnings)
}

/// `player1` .. `player8` to colour index 0 .. 7.
fn player_index(key: &str) -> Option<usize> {
    let n: usize = key.strip_prefix("player")?.parse().ok()?;
    (1..=PLAYERS).contains(&n).then(|| n - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BLACK;

    const SAMPLE: &str = "\
; comment
# also a comment
[Classic Colors + Black 2 Pink]
author = Maxx444#2154
player1 = #A40000 ; Red
Player6=ff00ff
player7 = -
player8 =
unknown = whatever

[  ]
player1 = #000000

[Classic Colors + black 2 PINK]
player1 = #FFFFFF

[Broken]
player2 = blue
player9 = #000000
just text
";

    #[test]
    fn reads_profiles_and_reports_problems() {
        let (p, w) = parse(SAMPLE);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].name, "Classic Colors + Black 2 Pink");
        assert_eq!(p[0].author, "Maxx444#2154");
        let mut want = [None; PLAYERS];
        want[0] = Some([0xa4, 0, 0]);
        want[BLACK] = Some([0xff, 0, 0xff]);
        assert_eq!(p[0].colors, want);
        assert_eq!(p[1].name, "Broken");
        assert_eq!(p[1].colors, [None; PLAYERS]);
        let lines: Vec<usize> = w.iter().map(|w| w.line).collect();
        assert_eq!(lines, vec![11, 14, 18, 20]);
    }

    #[test]
    fn caps_the_number_of_profiles() {
        let text: String = (0..MAX_PROFILES + 3)
            .map(|i| format!("[p{i}]\nplayer1 = #000000\n"))
            .collect();
        let (p, w) = parse(&text);
        assert_eq!(p.len(), MAX_PROFILES);
        assert_eq!(w.len(), 3);
    }
}
