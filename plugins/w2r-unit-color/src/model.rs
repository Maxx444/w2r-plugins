//! Pure recolor rules: the eight team colours, colour profiles, and what the settings store. No
//! host calls, so everything here is tested on any machine. Ramp shading follows wc2r-plus `recolour.cpp`.

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
    pub colors: Colors,
    /// The `colors` object's other fields (highlight and bar colors from warcraft2.site and W2
    /// Toolkit presets). Not applied, only kept so the file loses nothing when it is saved again.
    pub extra: Vec<(String, crate::json::Value)>,
}

/// What a new `color_profiles.json` holds, in this order.
pub fn default_profiles() -> Vec<Profile> {
    let all = |c: [Rgb; PLAYERS]| c.map(Some);
    let black = |rgb| {
        let mut c = [None; PLAYERS];
        c[BLACK] = Some(rgb);
        c
    };
    let p = |name: &str, colors| Profile {
        name: name.into(),
        colors,
        extra: Vec::new(),
    };
    vec![
        p("Remastered", all(REMASTERED)),
        p("Classic", all(CLASSIC)),
        p("Black to pink", black(PINK)),
        p("Black to cyan", black(CYAN)),
    ]
}

/// The options screen's profile dropdown: `Custom`, `Game default`, then the profiles as they
/// were when the game started (the host fixes a dropdown's list at load).
pub const CHOICE_CUSTOM: i64 = 0;
pub const CHOICE_GAME_DEFAULT: i64 = 1;
/// Most entries a dropdown may have (`W2R_MAX_SETTING_CHOICE_COUNT`), and so most profiles listed.
pub const MAX_CHOICES: usize = 16;
pub const MAX_LISTED: usize = MAX_CHOICES - 2;
/// Longest dropdown label the host accepts.
const MAX_CHOICE_LABEL: usize = 64;
/// Settings key of the dropdown (an index).
pub const PROFILE_KEY: &str = "profile";
/// Settings keys of the options screen's save fields: the checkbox that makes Apply save, and the
/// name it saves under.
pub const SAVE_KEY: &str = "save_profile";
pub const NAME_KEY: &str = "profile_name";
/// Settings keys of the options screen's delete: the checkbox that asks for it, and the second one
/// that confirms it. Apply deletes only when both are checked.
pub const DELETE_KEY: &str = "delete_profile";
pub const CONFIRM_KEY: &str = "delete_profile_confirm";

/// What Apply on the options screen does with the save and delete checkboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileAction {
    None,
    Save,
    Delete,
    /// Delete was checked without "Yes, Delete It".
    DeleteNotConfirmed,
    /// Save and Delete were both checked: nothing is done.
    Both,
}

pub fn profile_action(save: bool, delete: bool, confirmed: bool) -> ProfileAction {
    match (save, delete, confirmed) {
        (true, true, _) => ProfileAction::Both,
        (true, false, _) => ProfileAction::Save,
        (false, true, true) => ProfileAction::Delete,
        (false, true, false) => ProfileAction::DeleteNotConfirmed,
        // "Yes" alone does nothing.
        (false, false, _) => ProfileAction::None,
    }
}

/// The dropdown's labels for `profiles`: their names, cut to the host's 64 bytes.
pub fn choice_labels(profiles: &[Profile]) -> Vec<String> {
    let mut out = vec!["Custom".to_string(), "Game default".to_string()];
    for p in profiles.iter().take(MAX_LISTED) {
        let mut label = p.name.clone();
        label.truncate(MAX_CHOICE_LABEL); // ASCII only (clean_name), so any cut is on a boundary
        out.push(label.trim_end().to_string());
    }
    out
}

/// The dropdown index that describes the colours in use: Game default when nothing is changed,
/// the listed profile (by name) that matches, or Custom.
pub fn choice_for(listed: &[String], all_default: bool, matched: Option<&str>) -> i64 {
    if all_default {
        return CHOICE_GAME_DEFAULT;
    }
    matched
        .and_then(|name| listed.iter().position(|n| n.eq_ignore_ascii_case(name)))
        .map_or(CHOICE_CUSTOM, |i| i as i64 + 2)
}

/// What choosing dropdown entry `index` asks for: `Some(None)` the game's own colours,
/// `Some(Some(name))` the profile listed at launch under that name, `None` nothing (Custom, or an
/// index out of range).
pub fn choice_target(listed: &[String], index: i64) -> Option<Option<&str>> {
    match index {
        CHOICE_GAME_DEFAULT => Some(None),
        i if i >= 2 => listed
            .get(usize::try_from(i - 2).ok()?)
            .map(|n| Some(n.as_str())),
        _ => None,
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

/// The settings value for a colour: six lowercase hex digits, the format the options screen's
/// colour picker stores (`W2R_SETTING_COLOR`).
pub fn to_setting(rgb: Rgb) -> String {
    format!("{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// What a stored colour means: `None` (the game's own) when it is empty, unreadable, or equal to
/// the game's own colour `stock`, so picking the stock colour in the picker is "no override".
pub fn wanted(stored: &str, stock: Rgb) -> Option<Rgb> {
    parse_entry(stored)
        .ok()
        .flatten()
        .filter(|rgb| *rgb != stock)
}

/// The first profile whose colours are what is in use, treating a colour equal to the game's own
/// as unset on both sides. `None`: no profile matches (custom colours).
pub fn matching<'a>(
    profiles: &'a [Profile],
    cur: &Colors,
    stock: &[Rgb; PLAYERS],
) -> Option<&'a Profile> {
    let norm = |c: &Colors| -> Colors {
        let mut out = *c;
        for (o, s) in out.iter_mut().zip(stock) {
            if *o == Some(*s) {
                *o = None;
            }
        }
        out
    };
    let cur = norm(cur);
    profiles.iter().find(|p| norm(&p.colors) == cur)
}

/// The stock colours as read from the game, or the Remastered values when the read looks wrong
/// (all black, as an uninitialised table would be).
pub fn stock_or_default(read: [Rgb; PLAYERS]) -> [Rgb; PLAYERS] {
    if read.iter().all(|c| *c == [0, 0, 0]) {
        REMASTERED
    } else {
        read
    }
}

/// Most profiles kept from one file; the rest are ignored.
pub const MAX_PROFILES: usize = 64;
/// Longest profile name kept (in characters); longer ones are cut.
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
        assert!(d.iter().all(|p| p.name.len() <= MAX_NAME));
        assert_eq!(d[1].name, "Classic");
        assert_eq!(d[2].colors[BLACK], Some(PINK));
        assert_eq!(d[2].colors.iter().filter(|c| c.is_some()).count(), 1);
    }

    #[test]
    fn settings_values() {
        assert_eq!(to_setting([0xa4, 0, 0x3c]), "a4003c");
        assert_eq!(parse_entry("a4003c"), Ok(Some([0xa4, 0, 0x3c])));
        let stock = REMASTERED[0];
        assert_eq!(wanted("", stock), None);
        assert_eq!(wanted("junk", stock), None);
        assert_eq!(wanted(&to_setting(stock), stock), None);
        assert_eq!(wanted("#A40000", stock), Some([0xa4, 0, 0]));
    }

    #[test]
    fn finds_the_profile_in_use() {
        let profiles = default_profiles();
        let stock = REMASTERED;
        // The Remastered profile spells out the stock colours, so it matches "all unset".
        assert_eq!(
            matching(&profiles, &[None; PLAYERS], &stock).map(|p| p.name.as_str()),
            Some("Remastered")
        );
        assert_eq!(
            matching(&profiles, &CLASSIC.map(Some), &stock).map(|p| p.name.as_str()),
            Some("Classic")
        );
        let mut pink = [None; PLAYERS];
        pink[BLACK] = Some(PINK);
        assert_eq!(
            matching(&profiles, &pink, &stock).map(|p| p.name.as_str()),
            Some("Black to pink")
        );
        pink[0] = Some([1, 2, 3]);
        assert_eq!(matching(&profiles, &pink, &stock), None);
    }

    #[test]
    fn stock_falls_back_when_unread() {
        assert_eq!(stock_or_default([[0; 3]; PLAYERS]), REMASTERED);
        assert_eq!(stock_or_default(CLASSIC), CLASSIC);
    }

    #[test]
    fn delete_needs_both_boxes_and_never_mixes_with_save() {
        use ProfileAction::*;
        assert_eq!(profile_action(false, false, false), None);
        assert_eq!(profile_action(false, false, true), None);
        assert_eq!(profile_action(true, false, false), Save);
        assert_eq!(profile_action(false, true, false), DeleteNotConfirmed);
        assert_eq!(profile_action(false, true, true), Delete);
        assert_eq!(profile_action(true, true, true), Both);
    }

    #[test]
    fn dropdown_labels_fit() {
        let mut d = default_profiles();
        d.push(Profile {
            name: "x".repeat(40),
            colors: [None; PLAYERS],
            extra: Vec::new(),
        });
        let labels = choice_labels(&d);
        assert_eq!(labels[0], "Custom");
        assert_eq!(labels[1], "Game default");
        assert_eq!(labels[3], "Classic");
        assert!(labels.iter().all(|l| l.len() <= 64 && !l.is_empty()));
        let many: Vec<Profile> = (0..30)
            .map(|i| Profile {
                name: format!("p{i}"),
                colors: [None; PLAYERS],
                extra: Vec::new(),
            })
            .collect();
        assert_eq!(choice_labels(&many).len(), MAX_CHOICES);
    }

    #[test]
    fn dropdown_index_round_trips() {
        let listed = vec!["Remastered".to_string(), "Classic".to_string()];
        assert_eq!(
            choice_for(&listed, true, Some("Remastered")),
            CHOICE_GAME_DEFAULT
        );
        assert_eq!(choice_for(&listed, false, Some("classic")), 3);
        assert_eq!(
            choice_for(&listed, false, Some("Added later")),
            CHOICE_CUSTOM
        );
        assert_eq!(choice_for(&listed, false, None), CHOICE_CUSTOM);
        assert_eq!(choice_target(&listed, 3), Some(Some("Classic")));
        assert_eq!(choice_target(&listed, CHOICE_GAME_DEFAULT), Some(None));
        assert_eq!(choice_target(&listed, CHOICE_CUSTOM), None);
        assert_eq!(choice_target(&listed, 9), None);
        assert_eq!(choice_target(&listed, -4), None);
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  ok  ", "f"), "ok");
        assert_eq!(clean_name("", "f"), "f");
        assert_eq!(clean_name("é\n", "f"), "??");
        assert_eq!(clean_name(&"x".repeat(100), "f").len(), MAX_NAME);
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
