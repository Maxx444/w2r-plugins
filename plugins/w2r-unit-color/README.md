# Unit Color

Draws any of the eight team colors in a color you choose, on your screen only. Purely cosmetic and
local: nobody else sees it and the simulation is untouched. Grew out of the "Black2Pink" option of
[wc2r-plus](https://github.com/Luxray997/wc2r-plus) by Luxray997 (MIT), by way of the Black Recolor
plugin, which this one replaces.

**Turn Black Recolor off** before enabling Unit Color: both hook the same game function, so the
second one to load fails (the log says `hook_install returned` with a conflict). Black Recolor's
pink/cyan setting does not carry over; pick the "Black to pink" or "Black to cyan" profile.

## Settings page

| Row | What it does |
|---|---|
| `In use` | The profile last applied, with its author, or "Custom colors" once you edit a color. |
| `Player 1 Red` .. `Player 8 Yellow` | `#RRGGBB` for that team color; empty means the game's own color. A box applies as soon as it holds a valid color. |
| `Profile` + `Apply profile` | Game default, then every profile in `color_profiles.ini`, shown as "name by author". |
| `Profile name`, `Author`, `Save current colors as this profile` | Saves the colors on screen into `color_profiles.ini`, replacing a profile of the same name. Moving the slider fills both boxes with that profile's name and author. |
| `Reload color_profiles.ini` | Reads the file again after you edit it by hand. |
| `Import presets.json (warcraft2.site) into profiles` | Adds the presets in `presets.json` as profiles (same name: replaced). |
| `Export current colors to export.json` | Writes the colors on screen, all eight written out, as a warcraft2.site preset, using the name and author boxes. |
| `Reset all to game default` | Clears every color. |

The colors in use are kept as `color1` .. `color8` in the plugin's settings.

The color, name and author boxes need a w2r whose plugin UI has edit boxes; on an older one only
the slider and the buttons are shown.

## Files

All three files are in `plugins\w2r-unit-color\data\` inside your w2r folder
(`%USERPROFILE%\Saved Games\Warcraft2Remastered\w2r\`). That is the only folder the plugin API lets a
plugin read or write; it never sees a path.

### `color_profiles.ini`

Created with four profiles (Remastered, Classic, Black to pink, Black to cyan) the first time the
plugin loads. Edit it freely, then press Reload.

```ini
[Classic Colors + Black 2 Pink]
author = Maxx444#2154
player1 = #A40000 ; Red
player2 = #003CC0 ; Blue
player3 = #2CB494 ; Teal
player4 = #9848B0 ; Violet
player5 = #F08414 ; Orange
player6 = #FF00FF ; Black
player7 = -       ; White
player8 = #FCFC48 ; Yellow
```

- The section name is the profile name (printable ASCII, at most 40 characters; the author too).
- `playerN` is player N's team color as `#RRGGBB`. A player that is missing, empty or `-` keeps the
  game's own color.
- Lines starting with `;` or `#` are comments, and `;` after a color starts a comment. Other keys
  are ignored.
- A second profile with the same name (ignoring case), a bad color or a line that is not
  `key = value` is skipped; the rest of the file still loads, and the page and the log say which
  line. At most 64 profiles and 1 MiB are read.
- Saving from the page rewrites the whole file in this layout, so comments you added are lost.

### `presets.json` and `export.json`

`presets.json` may hold one preset as shared on
[warcraft2.site](https://warcraft2.site/color-presets) and by W2 Toolkit
(`{"name", "author", "colors": {"players": [{"player": 1, "color": "#A40000"}, ...]}}`), an array
of them, the site's listing (`{"items": [...]}`), or just the `colors` object. Fields the plugin
cannot apply (HP and mana bars, selection and resource highlights) are ignored.

`export.json` has the same shape, players only. Whether W2 Toolkit accepts a file without the bar
and highlight fields has not been checked.

## What changes, and what does not

"Player N" means the Nth team color, as on warcraft2.site; the game shuffles which player gets which
color at the start of a match.

- **HD color table** (`TeamColourRgba`), all eight entries: units, color boxes, and anything else
  that reads the table (the player color bar and chat names in `w2r-chat` follow it).
- **Render palette ramps** for Red to White (entries `0xd0` to `0xeb`, four per team, shaded by the
  stock ramp's brightness steps): classic-graphics sprites, and the minimap dots of the colors whose
  dot uses the ramp's first entry (Red, Teal, Violet, Orange, Black).
- **Not changed:** Yellow has no ramp of its own (`0xec`.. holds the gold mine highlight), and the
  minimap dots of Blue, White and Yellow use shared palette entries (1, 255 and 2) that other
  things use too, so those stay stock. The War_2_Remastered_Mods color tool has the same limits.
- **Restores:** choosing Game default, or unloading the plugin, writes the stock values back. The
  settings are kept.

## Capabilities

- `game.read`: symbol lookup for `TeamColourRgba`, `RenderPalette` and `HudMenuButtons`.
- `hooks.raw`: one detour on `HudMenuButtons` (drawn every frame in a match; original called first)
  as the per-frame tick, and direct writes to the two color tables. `affects_simulation` is false.
