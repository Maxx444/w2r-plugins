# Unit Color

Draws any of the eight team colors in a color you choose, on your screen only. Purely cosmetic and
local: nobody else sees it and the simulation is untouched. Grew out of the "Black2Pink" option of
[wc2r-plus](https://github.com/Luxray997/wc2r-plus) by Luxray997 (MIT), by way of the Black Recolor
plugin, which this one replaces.

**Turn Black Recolor off** before enabling Unit Color: both hook the same game function, so the
second one to load fails (the log says `hook_install returned` with a conflict). Black Recolor's
pink/cyan setting does not carry over; pick the "Black to pink" or "Black to cyan" profile.

## Settings

**Options > Unit Color** (needs a W2R Mod Loader with the options screen):

- **Reopen the Menu to see APPLIED > Color Profile:** a dropdown with **Custom** and **Game
  default**, then the profiles in `color_profiles.json` by name, as they were when the game
  started. Choose one and press Apply. It shows the profile whose colors are in use, or Custom once
  you change a color by hand.
  - **The dropdown lists up to 14 profiles** (the host allows 16 entries, and Custom and Game
    default take two). The four profiles the plugin starts with count toward the 14. Profiles after
    the 14th stay in the file and can be applied from the plugin card; the file holds up to 64.
- **Save or Delete Profile:**
  - **Profile Name:** the profile to save or delete. Choosing a profile fills it in the next time
    the screen opens.
  - **Save Colors as Profile on Apply:** check it and press Apply: the colors below are saved under
    that name, replacing a profile of the same name. Editing a profile is: choose it, Apply, reopen,
    change colors, check Save, Apply.
  - **Delete This Profile on Apply** and **Yes, Delete It (Cannot Be Undone):** check both and press
    Apply to remove the profile named above. Delete alone does nothing (the "Yes" box is greyed
    until it is checked), and Save with Delete does nothing either. The colors in use are not
    changed.
  - The boxes clear themselves after Apply. A saved or deleted profile joins or leaves the dropdown
    at the next start (the card's list changes at once).
- **Player Colors:** one color picker per team color, `Player 1 (Red)` .. `Player 8 (Yellow)`. Each
  shows a swatch and its hex value; the button opens the color editor with the new and the current
  color side by side. Picking the game's own color means no change for that team.

The screen keeps its own copy of every value while it is open, so after choosing a profile and
pressing Apply the swatches show its colors only the next time you open Options; the heading
above the dropdown says so. If you change the dropdown and a picker in the same visit, the profile wins.

**The plugin's card** (title screen > Manage), for what the options screen cannot do:

| Row | What it does |
|---|---|
| `>> IN USE: ... <<` | The profile whose colors are in use, "Game default", or "Custom colors". |
| `Player 1 (Red)` .. `Player 8 (Yellow)` | The color in use for that team as `#RRGGBB`, the same value the Options picker shows. Type a new one to apply it at once; clear the box for the game's own color. The Options pickers show a change the next time the screen opens. (Text boxes until the host offers a color swatch for plugin cards.) |
| `Profile` + `Apply profile` | Game default, then every profile in `color_profiles.json`, including ones added since the game started. |
| `Profile name`, `Save current colors as this profile` | Saves the colors in use into `color_profiles.json`, replacing a profile of the same name. Moving the slider fills in that profile's name. |
| `Delete this profile` | Asks "Delete "name"? This cannot be undone." for the profile chosen with the slider, then `Yes, delete it` removes it from `color_profiles.json` and `No, keep it` does nothing. The colors in use are not changed. |
| `Reload color_profiles.json` | Reads the file again after you edit it; the card's list updates at once, the Options dropdown at the next start. |
| `Reset all to game default` | Clears every color. |

On an older W2R Mod Loader without the options screen, the card's text boxes are the only way to
set a color.

The colors are kept as `color1` .. `color8` (`rrggbb`) and the dropdown as `profile` in the
plugin's settings.

## `color_profiles.json`

In `plugins\w2r-unit-color\data\` inside your w2r folder
(`%USERPROFILE%\Saved Games\Warcraft2Remastered\w2r\`). That is the only folder the plugin API lets a
plugin read or write; it never sees a path. Created the first time the plugin loads: from an older
`color_profiles.ini` if there is one (that file is then no longer used), otherwise with four
profiles (Remastered, Classic, Black to pink, Black to cyan).

Each profile is a warcraft2.site / W2 Toolkit preset with a name:

```json
{
  "profiles": [
    {
      "name": "Classic Colors + Black 2 Pink",
      "colors": {
        "players": [
          { "player": 1, "color": "#A40000" },
          { "player": 2, "color": "#003CC0" },
          { "player": 6, "color": "#FF00FF" }
        ],
        "selectionHighlight": "#00FF00",
        "manaBar": "#3470A4"
      }
    }
  ]
}
```

- `player` is the team color's number (1 Red, 2 Blue, 3 Teal, 4 Violet, 5 Orange, 6 Black,
  7 White, 8 Yellow) and `color` is `#RRGGBB`. A player left out keeps the game's own color.
- The name is printable ASCII, at most 40 characters. An `author` in a pasted site preset is ignored.
- Other fields in `colors` (selection, ally, critter, gold mine and oil highlights, HP and mana
  bars) are not applied, but are kept when the plugin saves the file again.
- A preset copied from warcraft2.site or W2 Toolkit can be pasted in as it is: the file may also be
  a plain list of profiles, the site's listing (`{"items": [...]}`), one profile, or just a
  `colors` object (named "Profile 1").
- A profile with a bad color, a player number outside 1 to 8, or a name used earlier (ignoring case)
  is skipped; the rest still load, and the card and the log say which profile (counting from 1).
  At most 64 profiles and 1 MiB are read.
- If the file is not valid JSON at all, no profiles load and saving is refused, so nothing is
  written over it; fix it and press Reload.
- Saving rewrites the whole file in this layout (two-space indents).

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
- **Score screen:** the colored cells after a match come from the game's skin file
  (`fe_endgame_stats_bar_0` .. `_7` in `Data\skins\skins.json`), not from the color table, and a
  plugin can neither edit game files nor reach those skins in memory. They stay stock until the
  W2R Mod Loader offers a way (requested from the maintainer).
- **Restores:** choosing Game default, or unloading the plugin, writes the stock values back. The
  settings are kept.

## Capabilities

- `game.read`: symbol lookup for `TeamColourRgba`, `RenderPalette` and `HudMenuButtons`.
- `hooks.raw`: one detour on `HudMenuButtons` (drawn every frame in a match; original called first)
  as the per-frame tick, and direct writes to the two color tables. `affects_simulation` is false.
