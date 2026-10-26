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

- **Profile (Reopen to See Colors) > Color Profile:** a dropdown with Custom, Game default, then the
  profiles in `color_profiles.ini` (up to 14, by name) as they were when the game started. Choose one and
  press Apply. It shows the profile whose colors are in use, or Custom once you change a color by
  hand.
- **Save Profile:** check **Save Colors as Profile on Apply**, fill in **Profile Name** (and
  **Author** if you like), then press Apply: the colors below are saved to `color_profiles.ini`,
  replacing a profile of the same name, and the box clears itself. Choosing a profile fills in its
  name and author, so editing one is: choose it, Apply, reopen, change colors, check Save, Apply.
  A new name joins the dropdown at the next start (the card's list has it at once).
- **Player Colors:** one color picker per team color, `Player 1 (Red)` .. `Player 8 (Yellow)`. Each
  shows a swatch and its hex value; the button opens the color editor with the new and the current
  color side by side. Picking the game's own color means no change for that team.

The screen keeps its own copy of every value while it is open, so after choosing a profile and
pressing Apply the swatches show its colors only the next time you open Options; the Profile
heading says so. If you change the dropdown and a picker in the same visit, the profile wins.

**The plugin's card** (title screen > Manage), for what the options screen cannot do:

| Row | What it does |
|---|---|
| `>> IN USE: ... <<` | The profile whose colors are in use ("name by author"), "Game default", or "Custom colors". |
| `Player 1 Red` .. `Player 8 Yellow` | `#RRGGBB` for that team color; empty means the game's own color. Applies as soon as a box holds a valid color; the Options pickers show it the next time the screen opens. (Text boxes until the host offers a color swatch for plugin cards.) |
| `Profile` + `Apply profile` | Game default, then every profile in `color_profiles.ini`, including ones added since the game started. |
| `Profile name`, `Author`, `Save current colors as this profile` | Saves the colors in use into `color_profiles.ini`, replacing a profile of the same name. Moving the slider fills both boxes with that profile's name and author. |
| `Reload color_profiles.ini` | Reads the file again after you edit it; the card's list updates at once, the Options dropdown at the next start. |
| `Reset all to game default` | Clears every color. |

On an older W2R Mod Loader without the options screen, the card's text boxes are the only way to
set a color.

The colors are kept as `color1` .. `color8` (`rrggbb`) and the dropdown as `profile` in the
plugin's settings.

## `color_profiles.ini`

In `plugins\w2r-unit-color\data\` inside your w2r folder
(`%USERPROFILE%\Saved Games\Warcraft2Remastered\w2r\`). That is the only folder the plugin API lets a
plugin read or write; it never sees a path. Created with four profiles (Remastered, Classic, Black
to pink, Black to cyan) the first time the plugin loads.

```ini
[Classic Colors + Black 2 Pink]
author = Maxx444
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
  `key = value` is skipped; the rest of the file still loads, and the card and the log say which
  line. At most 64 profiles and 1 MiB are read.
- Saving from the card rewrites the whole file in this layout, so comments you added are lost.

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
