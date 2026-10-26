//! Pure recolor rules: the eight team colours, colour profiles and warcraft2.site preset files.
//! No host calls, so everything here is tested on any machine. Ramp shading follows wc2r-plus `recolour.cpp`.

use crate::json::{self, Value};

/// An sRGB colour.
pub type Rgb = [u8; 3];

/// Number of team colours (colour index 0 red .. 7 yellow).
pub const PLAYERS: usize = 8;
/// The black team's colour index.
pub const BLACK: usize = 5;
/// Length of one team ramp in the render palette (first entry is the base shade).
pub const RAMP: usize = 4;
/// Render palette entry where the red ramp starts; the following ramps are four entries apart.
const FIRST_RAMP: usize = 0xd0;

/// One choice per colour: `None` leaves the game's own colour alone.
pub type Colors = [Option<Rgb>; PLAYERS];

/// Colour names in colour-index order, as the game's lobby shows them.
pub const NAMES: [&str; PLAYERS] = [
    "Red", "Blue", "Teal", "Violet", "Orange", "Black", "White", "Yellow",
];

/// Settings key of colour `i` (`color1` .. `color8`); the value is `#RRGGBB` or empty.
pub fn key(i: usize) -> String {
    format!("color{}", i + 1)
}

/// Settings key holding the name of the profile last applied (empty once a colour is edited).
pub const PROFILE_KEY: &str = "profile";

/// wc2r-plus's Black2Pink and Black2Cyan.
pub const PINK: Rgb = [0xe8, 0x7b, 0xdd];
pub const CYAN: Rgb = [0x00, 0xc6, 0xe0];

/// Classic Warcraft II colours (the DOS / Battle.net Edition palette).
pub const CLASSIC: [Rgb; PLAYERS] = [
    [0xa4, 0x00, 0x00],
    [0x00, 0x3c, 0xc0],
    [0x2c, 0xb4, 0x94],
    [0x98, 0x48, 0xb0],
    [0xf0, 0x84, 0x14],
    [0x28, 0x28, 0x3c],
    [0xe0, 0xe0, 0xe0],
    [0xfc, 0xfc, 0x48],
];

/// The Remastered colours, written out (choosing them pins the values; "Game default" does not).
pub const REMASTERED: [Rgb; PLAYERS] = [
    [0xa6, 0x00, 0x00],
    [0x00, 0x96, 0xff],
    [0x2d, 0xb6, 0x96],
    [0x9a, 0x49, 0xb2],
    [0xfb, 0x8e, 0x14],
    [0x28, 0x28, 0x3d],
    [0xe3, 0xe3, 0xe3],
    [0xff, 0xff, 0x49],
];

/// A named set of colours, with who made it.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub name: String,
    pub author: String,
    pub colors: Colors,
}

/// What a new `color_profiles.ini` holds, in this order.
pub fn default_profiles() -> Vec<Profile> {
    let all = |c: [Rgb; PLAYERS]| c.map(Some);
    let black = |rgb| {
        let mut c = [None; PLAYERS];
        c[BLACK] = Some(rgb);
        c
    };
    let p = |name: &str, author: &str, colors| Profile {
        name: name.into(),
        author: author.into(),
        colors,
    };
    vec![
        p("Remastered", "Blizzard Entertainment", all(REMASTERED)),
        p("Classic", "Blizzard Entertainment", all(CLASSIC)),
        p("Black to pink", "Luxray997 (wc2r-plus)", black(PINK)),
        p("Black to cyan", "Luxray997 (wc2r-plus)", black(CYAN)),
    ]
}

/// "Name by Author", or the name alone.
pub fn title(p: &Profile) -> String {
    if p.author.is_empty() {
        p.name.clone()
    } else {
        format!("{} by {}", p.name, p.author)
    }
}

/// Parses `#RRGGBB` or `RRGGBB` (surrounding spaces allowed, any case).
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let t = text.trim();
    let t = t.strip_prefix('#').unwrap_or(t);
    if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// `#RRGGBB`, upper case.
pub fn to_hex(rgb: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// An edit box or code entry that is neither a colour nor empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAColor;

/// What one colour edit box holds: empty means "game default", anything else must be a colour.
pub fn parse_entry(text: &str) -> Result<Option<Rgb>, NotAColor> {
    let t = text.trim();
    if t.is_empty() || t == "-" {
        return Ok(None);
    }
    parse_hex(t).map(Some).ok_or(NotAColor)
}

/// Most profiles kept from one file; the rest are ignored.
pub const MAX_PROFILES: usize = 64;
/// Longest profile name or author kept (in characters); longer ones are cut.
pub const MAX_NAME: usize = 40;

/// A name safe to show in the game's UI: printable ASCII, trimmed, at most `MAX_NAME`.
pub fn clean_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '?'
            }
        })
        .take(MAX_NAME)
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

/// The colours of one preset's `colors` object (or of the object itself when it has `players`).
/// `players` is either `[{"player": 1, "color": "#A60000"}, ...]` (warcraft2.site and W2
/// Toolkit) or eight colour strings. Unknown fields (bars, highlights) are ignored.
fn colors_of(v: &Value) -> Option<Colors> {
    let players = v
        .get("players")
        .or_else(|| v.get("colors").and_then(|c| c.get("players")))?
        .as_array()?;
    let mut out = [None; PLAYERS];
    let mut any = false;
    for (n, entry) in players.iter().enumerate() {
        let (slot, color) = match entry {
            Value::Str(s) => (n, s.as_str()),
            Value::Obj(_) => {
                let p = entry.get("player")?.as_f64()?;
                if p.fract() != 0.0 || !(1.0..=PLAYERS as f64).contains(&p) {
                    return None;
                }
                (p as usize - 1, entry.get("color")?.as_str()?)
            }
            _ => return None,
        };
        if slot >= PLAYERS {
            return None;
        }
        out[slot] = Some(parse_hex(color)?);
        any = true;
    }
    any.then_some(out)
}

/// Every preset in a warcraft2.site / W2 Toolkit preset file, as profiles. Accepted shapes: one
/// preset (`{"name", "author", "colors": {...}}`), a bare `colors` object, an array of presets, or
/// a listing `{"items": [...]}` (the site's API). Entries that are not presets are skipped; a file
/// with none gives `Err`.
pub fn profiles_from_json(text: &str) -> Result<Vec<Profile>, &'static str> {
    let doc = json::parse(text).ok_or("not valid JSON")?;
    let items: Vec<&Value> = match (&doc, doc.get("items")) {
        (Value::Arr(items), _) => items.iter().collect(),
        (_, Some(Value::Arr(items))) => items.iter().collect(),
        _ => vec![&doc],
    };
    let mut out = Vec::new();
    for (n, item) in items.into_iter().enumerate() {
        if out.len() == MAX_PROFILES {
            break;
        }
        if let Some(colors) = colors_of(item) {
            let fallback = format!("Imported {}", n + 1);
            let text = |k| item.get(k).and_then(Value::as_str).unwrap_or("");
            out.push(Profile {
                name: clean_name(text("name"), &fallback),
                author: clean_name(text("author"), ""),
                colors,
            });
        }
    }
    if out.is_empty() {
        Err("no presets with player colors found")
    } else {
        Ok(out)
    }
}

/// One preset as JSON in the warcraft2.site / W2 Toolkit shape (players only), with every colour
/// written out: `shown[i]` is the colour on screen for colour index `i`.
pub fn export_json(name: &str, author: &str, shown: &[Rgb; PLAYERS]) -> String {
    let players: Vec<String> = shown
        .iter()
        .enumerate()
        .map(|(i, rgb)| {
            format!(
                "      {{ \"color\": \"{}\", \"player\": {} }}",
                to_hex(*rgb),
                i + 1
            )
        })
        .collect();
    format!(
        "{{\n  \"name\": {},\n  \"author\": {},\n  \"colors\": {{\n    \"players\": [\n{}\n    ]\n  }}\n}}\n",
        json::quote(name),
        json::quote(author),
        players.join(",\n")
    )
}

/// Render palette entry where colour `i`'s ramp starts, if it has one of its own. Red to white
/// use 0xd0..0xeb, four entries each (the game's unit remap table lists exactly these seven).
/// Yellow has no ramp of its own (0xec.. holds the gold mine highlight), so it is not touched.
pub fn ramp_base(i: usize) -> Option<usize> {
    (i < PLAYERS - 1).then_some(FIRST_RAMP + i * RAMP)
}

fn luma(c: Rgb) -> i32 {
    (c[0] as i32 * 299 + c[1] as i32 * 587 + c[2] as i32 * 114) / 1000
}

/// The ramp for `rgb`: the first entry is `rgb`, the rest keep the stock ramp's brightness ratio
/// to its first entry.
pub fn ramp(stock: &[Rgb; RAMP], rgb: Rgb) -> [Rgb; RAMP] {
    let base = luma(stock[0]);
    let mut out = [[0u8; 3]; RAMP];
    for k in 0..RAMP {
        for c in 0..3 {
            let mut v = rgb[c] as i32;
            if k > 0 && base > 0 {
                v = v * luma(stock[k]) / base;
            }
            out[k][c] = v.clamp(0, 255) as u8;
        }
    }
    out
}

/// The HD table entry (r, g, b as 0..1) for `rgb`.
pub fn float_rgb(rgb: Rgb) -> [f32; 3] {
    rgb.map(|c| c as f32 / 255.0)
}

/// The colour an HD table entry shows, rounded to bytes (values outside 0..1 are clamped).
pub fn rgb_of(entry: [f32; 3]) -> Rgb {
    entry.map(|f| (f.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        assert_eq!(parse_hex("#A60000"), Some([0xa6, 0, 0]));
        assert_eq!(parse_hex(" 0096ff "), Some([0, 0x96, 0xff]));
        assert_eq!(to_hex([0, 0x96, 0xff]), "#0096FF");
        for bad in ["", "#", "#12345", "#1234567", "GG0000", "#12 345", "+12345"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn entries_allow_default() {
        assert_eq!(parse_entry(""), Ok(None));
        assert_eq!(parse_entry(" - "), Ok(None));
        assert_eq!(parse_entry("ff00ff"), Ok(Some([255, 0, 255])));
        assert_eq!(parse_entry("pink"), Err(NotAColor));
    }

    #[test]
    fn defaults_are_sane() {
        let d = default_profiles();
        assert!(
            d.iter()
                .all(|p| p.name.len() <= MAX_NAME && p.author.len() <= MAX_NAME)
        );
        assert_eq!(d[2].colors[BLACK], Some(PINK));
        assert_eq!(d[2].colors.iter().filter(|c| c.is_some()).count(), 1);
        assert_eq!(title(&d[1]), "Classic by Blizzard Entertainment");
        let anon = Profile {
            name: "X".into(),
            author: String::new(),
            colors: [None; PLAYERS],
        };
        assert_eq!(title(&anon), "X");
    }

    /// One entry of the warcraft2.site API listing, trimmed (shape as served on 2026-10-25).
    const SITE: &str = r##"{"items":[{"id":"x","name":"Classic Colors + Black 2 Pink","colors":{
        "manaBar":"#3470A4","players":[{"color":"#A40000","player":1},{"color":"#003CC0","player":2},
        {"color":"#2CB494","player":3},{"color":"#9848B0","player":4},{"color":"#F08414","player":5},
        {"color":"#FF00FF","player":6},{"color":"#E0E0E0","player":7},{"color":"#FCFC48","player":8}],
        "allyHighlight":"#FFFF00"},"author":"a","installCount":0},
        {"id":"y","name":"no colors here"}],"hasMore":false}"##;

    #[test]
    fn reads_the_site_listing() {
        let p = profiles_from_json(SITE).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "Classic Colors + Black 2 Pink");
        assert_eq!(p[0].author, "a");
        let mut want = CLASSIC.map(Some);
        want[BLACK] = Some([0xff, 0, 0xff]);
        assert_eq!(p[0].colors, want);
    }

    #[test]
    fn reads_other_shapes() {
        let bare = r##"{"players":["#010203","#040506"]}"##;
        let p = profiles_from_json(bare).unwrap();
        assert_eq!(p[0].name, "Imported 1");
        assert_eq!(p[0].colors[1], Some([4, 5, 6]));
        assert_eq!(p[0].colors[2], None);

        let arr = r##"[{"name":"A","colors":{"players":[{"player":8,"color":"FFFF00"}]}}]"##;
        let p = profiles_from_json(arr).unwrap();
        assert_eq!(p[0].colors[7], Some([255, 255, 0]));
    }

    #[test]
    fn refuses_bad_presets() {
        assert!(profiles_from_json("nope").is_err());
        assert!(profiles_from_json("{}").is_err());
        for bad in [
            r##"{"players":[{"player":9,"color":"#000000"}]}"##,
            r##"{"players":[{"player":1.5,"color":"#000000"}]}"##,
            r##"{"players":[{"player":1,"color":"black"}]}"##,
            r##"{"players":["#000000","#000000","#000000","#000000","#000000","#000000","#000000","#000000","#000000"]}"##,
        ] {
            assert!(profiles_from_json(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  ok  ", "f"), "ok");
        assert_eq!(clean_name("", "f"), "f");
        assert_eq!(clean_name("é\n", "f"), "??");
        assert_eq!(clean_name(&"x".repeat(100), "f").len(), MAX_NAME);
    }

    #[test]
    fn caps_the_number_of_presets() {
        let one = r##"{"players":["#000000"]}"##;
        let many = format!("[{}]", vec![one; MAX_PROFILES + 5].join(","));
        assert_eq!(profiles_from_json(&many).unwrap().len(), MAX_PROFILES);
    }

    #[test]
    fn export_reimports() {
        let text = export_json("My \"set\"", "me", &REMASTERED);
        let p = profiles_from_json(&text).unwrap();
        assert_eq!(p[0].name, "My \"set\"");
        assert_eq!(p[0].author, "me");
        assert_eq!(p[0].colors, REMASTERED.map(Some));
    }

    #[test]
    fn ramps_cover_red_to_white_only() {
        assert_eq!(ramp_base(0), Some(0xd0));
        assert_eq!(ramp_base(BLACK), Some(0xe4));
        assert_eq!(ramp_base(6), Some(0xe8));
        assert_eq!(ramp_base(7), None);
    }

    #[test]
    fn ramp_keeps_the_stock_shading() {
        let stock = [[40, 40, 40], [20, 20, 20], [60, 60, 60], [80, 80, 80]];
        let r = ramp(&stock, [200, 100, 50]);
        assert_eq!(r[0], [200, 100, 50]);
        assert_eq!(r[1], [100, 50, 25]);
        assert_eq!(r[2], [255, 150, 75]);
        assert_eq!(r[3], [255, 200, 100]);
    }

    #[test]
    fn black_base_leaves_the_ramp_flat() {
        let r = ramp(&[[0; 3]; RAMP], [10, 20, 30]);
        assert!(r.iter().all(|e| *e == [10, 20, 30]));
    }

    #[test]
    fn float_entries_round_trip() {
        for rgb in REMASTERED {
            assert_eq!(rgb_of(float_rgb(rgb)), rgb);
        }
        assert_eq!(rgb_of([-1.0, 2.0, 0.5]), [0, 255, 128]);
    }
}
