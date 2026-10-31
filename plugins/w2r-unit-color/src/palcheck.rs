//! Palette check (personal build, diagnostic only; writes nothing to the game).
//!
//! Remastered ships the Battle.net Edition's four terrain palettes byte for byte, stored as VGA
//! values 0..63. This module answers two questions from the live render palette, for the log:
//! which of the four terrain palettes is loaded, and how the game turned 0..63 into 0..255
//! (`x * 4`, as the score screen skins do, or `x * 255 / 63`, as the HD team colours look like).
//! Pure rules only, tested on any machine; `plugin` reads the game and logs.

/// One of the four BNE terrain palettes, known by two entries no other palette shares.
pub struct Known {
    /// For the log. The terrain is a guess from the palette's ground colours, to be confirmed in
    /// game.
    pub label: &'static str,
    /// (palette index, VGA 0..63 RGB).
    pub samples: [(usize, [u8; 3]); 2],
}

pub const KNOWN: [Known; 4] = [
    Known {
        label: "palette 1 (greens; probably Forest)",
        samples: [(0x14, [9, 18, 1]), (0x48, [23, 23, 23])],
    },
    Known {
        label: "palette 2 (blue-greys; probably Winter)",
        samples: [(0x14, [24, 25, 32]), (0x48, [6, 21, 34])],
    },
    Known {
        label: "palette 3 (red-browns; probably Wasteland)",
        samples: [(0x14, [22, 8, 1]), (0x48, [4, 7, 0])],
    },
    Known {
        label: "palette 4 (dark browns; probably Swamp)",
        samples: [(0x14, [18, 9, 0]), (0x48, [5, 3, 0])],
    },
];

/// Whether a live 0..255 channel can be VGA value `vga` under either conversion (they differ by
/// at most 3).
fn near(live: u8, vga: u8) -> bool {
    let four = i32::from(vga) * 4;
    let scaled = (i32::from(vga) * 255 + 31) / 63;
    let v = i32::from(live);
    v >= four.min(scaled) && v <= four.max(scaled)
}

/// Which known palette `entry(index)` (0..255 RGB) is, if any.
pub fn identify(entry: impl Fn(usize) -> [u8; 3]) -> Option<&'static Known> {
    KNOWN.iter().find(|k| {
        k.samples
            .iter()
            .all(|&(i, vga)| entry(i).iter().zip(vga).all(|(&l, v)| near(l, v)))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conversion {
    /// Every value is a multiple of 4 and at most 252.
    Times4,
    /// Every value is round(x * 255 / 63) for some x, and some are not multiples of 4.
    Times255Over63,
    /// Neither fits every value.
    Unclear,
}

impl Conversion {
    pub fn text(self) -> &'static str {
        match self {
            Conversion::Times4 => "x4 (0..252)",
            Conversion::Times255Over63 => "x255/63 (0..255)",
            Conversion::Unclear => "unclear (neither fits every value)",
        }
    }
}

/// What the values say about the conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    pub conversion: Conversion,
    pub values: usize,
    pub not_multiple_of_4: usize,
    pub max: u8,
}

fn is_scaled(v: u8) -> bool {
    let x = (u32::from(v) * 63 + 127) / 255;
    (x * 255 + 31) / 63 == u32::from(v)
}

/// Looks at every channel value given (the plugin leaves out the ramps it writes).
pub fn scale(values: impl IntoIterator<Item = u8>) -> Scale {
    let mut s = Scale {
        conversion: Conversion::Unclear,
        values: 0,
        not_multiple_of_4: 0,
        max: 0,
    };
    let mut all_scaled = true;
    for v in values {
        s.values += 1;
        s.max = s.max.max(v);
        if v % 4 != 0 {
            s.not_multiple_of_4 += 1;
        }
        all_scaled &= is_scaled(v);
    }
    s.conversion = if s.values == 0 {
        Conversion::Unclear
    } else if s.not_multiple_of_4 == 0 && s.max <= 252 {
        Conversion::Times4
    } else if all_scaled {
        Conversion::Times255Over63
    } else {
        Conversion::Unclear
    };
    s
}

/// The graphics mode for the log, from the game's `GraphicsMode` word (a host symbol from W2R Mod
/// Loader 1.1.4: 0 classic, 1 HD, 2 and 3 while switching). `None` while switching, so nothing is
/// logged mid-switch; no symbol reads as unknown.
pub fn graphics_text(mode: Option<u32>) -> Option<&'static str> {
    match mode {
        None => Some("unknown (this loader has no GraphicsMode symbol)"),
        Some(0) => Some("classic (CG)"),
        Some(1) => Some("HD"),
        Some(2 | 3) => None,
        Some(_) => Some("unknown (unexpected GraphicsMode value)"),
    }
}

/// `#RRGGBB`.
pub fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette(conv: impl Fn(u8) -> u8, k: usize) -> impl Fn(usize) -> [u8; 3] {
        move |i| {
            KNOWN[k]
                .samples
                .iter()
                .find(|s| s.0 == i)
                .map_or([0; 3], |s| s.1.map(&conv))
        }
    }

    #[test]
    fn each_palette_is_known_under_both_conversions() {
        for (k, known) in KNOWN.iter().enumerate() {
            let four = identify(palette(|x| x * 4, k)).map(|p| p.label);
            let scaled = identify(palette(|x| ((u32::from(x) * 255 + 31) / 63) as u8, k));
            assert_eq!(four, Some(known.label));
            assert_eq!(scaled.map(|p| p.label), Some(known.label));
        }
        assert!(identify(|_| [1, 2, 3]).is_none());
    }

    #[test]
    fn the_conversion_is_told_apart() {
        let vga = 0u8..64;
        let four = scale(vga.clone().map(|x| x * 4));
        assert_eq!(four.conversion, Conversion::Times4);
        assert_eq!(four.max, 252);
        let scaled = scale(vga.map(|x| ((u32::from(x) * 255 + 31) / 63) as u8));
        assert_eq!(scaled.conversion, Conversion::Times255Over63);
        assert_eq!(scaled.max, 255);
        assert_eq!(scale([1u8, 2, 3]).conversion, Conversion::Unclear);
        assert_eq!(scale([]).conversion, Conversion::Unclear);
    }

    #[test]
    fn graphics_mode_is_named_and_switching_is_skipped() {
        assert_eq!(graphics_text(Some(0)), Some("classic (CG)"));
        assert_eq!(graphics_text(Some(1)), Some("HD"));
        assert_eq!(graphics_text(Some(2)), None);
        assert_eq!(graphics_text(Some(3)), None);
        assert!(graphics_text(None).unwrap().starts_with("unknown"));
        assert!(graphics_text(Some(7)).unwrap().starts_with("unknown"));
    }

    #[test]
    fn hex_is_upper_case() {
        assert_eq!(hex([0xa4, 0, 0x3c]), "#A4003C");
    }
}
