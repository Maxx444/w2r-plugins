//! The game-facing half (32-bit only). One detour on `HudMenuButtons` (the HUD menu buttons, drawn
//! every frame in a match; HudFoodCell, HudResourceCell and HudBeginFrame belong to other plugins
//! and a function takes one hook) is the per-frame tick: it calls the original, then makes the
//! eight entries of the HD colour table and the seven team ramps of the render palette match the
//! settings. Panics are caught before they unwind into the game (detours are outside the host's
//! crash guard, docs/abi.md).
//!
//! On a host with declared settings, the eight colours are colour pickers on the options screen
//! (Unit Color > Player Colors). The plugin's card in the plugin manager picks, saves and reloads
//! the profiles in `color_profiles.json`, which lives in the plugin's own data folder, the only
//! folder a plugin can write (docs/abi.md, `data_open`). On an older host the card also has one
//! text box per colour.

use core::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use w2r_abi::*;

use crate::model::{self, Colors, PLAYERS, Profile, RAMP, Rgb};
use crate::{ini, profiles};

/// Bytes per render palette entry (RGBA).
const PALETTE_STRIDE: usize = 4;
/// Floats per HD colour table entry (RGBA).
const TABLE_STRIDE: usize = 4;

/// The profiles file in the plugin's data folder.
const PROFILES_FILE: &str = "color_profiles.json";
/// The file earlier test builds wrote; converted once when only it exists.
const OLD_PROFILES_FILE: &str = "color_profiles.ini";
/// Largest file read (64 full profiles are about 20 KiB).
const MAX_FILE: usize = 1 << 20;
/// Edit buffers: `#RRGGBB` plus room for stray spaces; a profile name or author.
const HEX_CAP: usize = 16;
const NAME_CAP: usize = model::MAX_NAME + 1;

/// Where the colour pickers appear on the options screen.
const CATEGORY: &str = "Unit Color";
/// The heading doubles as the notice that the swatches catch up on the next visit (the screen
/// keeps its own copy of each value while it is open). At most 32 bytes.
const PROFILE_SUBCATEGORY: &str = "Reopen the Menu to see APPLIED";
const SAVE_SUBCATEGORY: &str = "Save Profile";
// The host refuses a category or subcategory over 32 bytes, and with it the whole declaration.
const _: () = assert!(
    CATEGORY.len() <= 32
        && PROFILE_SUBCATEGORY.len() <= 32
        && SAVE_SUBCATEGORY.len() <= 32
        && SUBCATEGORY.len() <= 32
);
const SUBCATEGORY: &str = "Player Colors";

type MenuButtonsFn = unsafe extern "C" fn(i32);

struct Game {
    table: *mut f32,
    palette: *const *mut u8,
}

// SAFETY: addresses of the game's globals, fixed for the session; used from the main thread.
unsafe impl Send for Game {}
unsafe impl Sync for Game {}

/// What we know about one colour's game entries, so the stock values are never lost and a foreign
/// change (the game rebuilding its palette) is noticed.
#[derive(Clone, Copy)]
struct Slot {
    stock_table: Option<[f32; 4]>,
    stock_ramp: [Rgb; RAMP],
    written_ramp: [Rgb; RAMP],
    owns_ramp: bool,
}

const EMPTY_SLOT: Slot = Slot {
    stock_table: None,
    stock_ramp: [[0; 3]; RAMP],
    written_ramp: [[0; 3]; RAMP],
    owns_ramp: false,
};

/// The plugin's card in the plugin manager.
struct Page {
    /// The profiles in `color_profiles.json`, in file order.
    profiles: Vec<Profile>,
    /// False while the file could not be read: saving would overwrite what is in it.
    file_ok: bool,
    /// Slider position: 0 is "Game default", n is `profiles[n - 1]`.
    selected: i32,
    hex: [[u8; HEX_CAP]; PLAYERS],
    name: [u8; NAME_CAP],
    author: [u8; NAME_CAP],
    status: String,
}

static GAME: OnceLock<Game> = OnceLock::new();
static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<W2rHandle>> = Mutex::new(None);
/// The wanted colours (`None`: the game's own); read by the tick, written by the page and events.
static COLORS: Mutex<Colors> = Mutex::new([None; PLAYERS]);
/// The game's own colours, read from the HD table before anything was written.
static STOCK: Mutex<[Rgb; PLAYERS]> = Mutex::new(model::REMASTERED);
/// Whether the options screen shows the colour pickers (the host took the declarations).
static DECLARED: AtomicBool = AtomicBool::new(false);
/// The profile names in the options screen's dropdown (after Custom and Game default), as they
/// were at load; the host does not let a dropdown's list change later.
static LISTED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The dropdown index last stored, to tell a new pick from a colour edit on Apply.
static LAST_CHOICE: Mutex<i64> = Mutex::new(model::CHOICE_GAME_DEFAULT);
static SLOTS: Mutex<[Slot; PLAYERS]> = Mutex::new([EMPTY_SLOT; PLAYERS]);
static PAGE: Mutex<Option<Page>> = Mutex::new(None);

fn host() -> Option<&'static W2rHost> {
    unsafe { HOST.load(Ordering::Acquire).as_ref() }
}

fn s(text: &str) -> W2rStr {
    W2rStr {
        ptr: text.as_ptr().cast(),
        len: text.len() as u32,
    }
}

fn log(level: u32, msg: &str) {
    if let Some(h) = host() {
        unsafe { (h.log)(h.token, level, s(msg)) };
    }
}

fn handle_of(name: &str) -> Option<W2rHandle> {
    let h = host()?;
    let mut handle = 0;
    (unsafe { (h.symbol_lookup)(h.token, s(name), &mut handle) } == W2R_OK).then_some(handle)
}

fn lookup(name: &str) -> Option<usize> {
    let h = host()?;
    let handle = handle_of(name)?;
    let mut addr = 0usize;
    (unsafe { (h.symbol_address)(h.token, handle, &mut addr) } == W2R_OK).then_some(addr)
}

fn colors() -> Colors {
    *COLORS.lock().unwrap_or_else(|e| e.into_inner())
}

fn stock() -> [Rgb; PLAYERS] {
    *STOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- settings --------------------------------------------------------------------------------

/// The value under `key`, or empty when it is missing or unreadable.
fn get_str(key: &str) -> String {
    let Some(h) = host() else {
        return String::new();
    };
    let mut buf = [0u8; 128];
    let mut len = 0u32;
    let st = unsafe {
        (h.settings_get_str)(
            h.token,
            s(key),
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &mut len,
        )
    };
    if st != W2R_OK {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..(len as usize).min(buf.len())]).into_owned()
}

fn set_str(key: &str, value: &str) {
    let Some(h) = host() else { return };
    let st = unsafe { (h.settings_set_str)(h.token, s(key), s(value)) };
    if st != W2R_OK {
        log(W2R_LOG_WARN, &format!("saving {key}: status {st}"));
    }
}

/// The colours in the settings. A value equal to the game's own colour, or one mangled by hand,
/// means "the game's own".
fn load_colors() -> Colors {
    let stock = stock();
    let mut out = [None; PLAYERS];
    for (i, c) in out.iter_mut().enumerate() {
        *c = model::wanted(&get_str(&model::key(i)), stock[i]);
    }
    out
}

/// Stores colour `i` the way the options screen's picker does (`rrggbb`; the game's own colour
/// is stored as itself, so the picker shows it).
fn save_one(i: usize, c: Option<Rgb>) {
    let value = model::to_setting(c.unwrap_or(stock()[i]));
    if get_str(&model::key(i)) != value {
        set_str(&model::key(i), &value);
    }
}

/// Makes `next` the wanted colours and stores them.
fn set_colors(next: Colors) {
    for (i, c) in next.iter().enumerate() {
        save_one(i, *c);
    }
    *COLORS.lock().unwrap_or_else(|e| e.into_inner()) = next;
}

/// One declaration with every field the kind does not use left empty.
fn def(kind: u32, key: &str, label: &str, description: &str, subcategory: &str) -> W2rSettingDef {
    W2rSettingDef {
        struct_size: W2R_SETTING_DEF_V1_SIZE,
        kind,
        default_int: 0,
        min: 0,
        max: 0,
        flags: 0,
        choice_count: 0,
        choices: core::ptr::null(),
        reserved: 0,
        key: s(key),
        label: s(label),
        description: s(description),
        category: s(CATEGORY),
        subcategory: s(subcategory),
        default_str: s(""),
        active_when: s(""),
    }
}

/// Declares the profile dropdown and the eight colour pickers for the options screen; false on
/// an older host or if the host refused one (it logs which).
fn declare(stock: &[Rgb; PLAYERS], profiles: &[Profile]) -> bool {
    let Some(h) = host() else { return false };
    if h.struct_size < W2R_HOST_SETTINGS_DECLARE_SIZE {
        return false;
    }
    let mut all = true;
    let mut submit = |d: &W2rSettingDef, key: &str| {
        let st = unsafe { (h.settings_declare)(h.token, d) };
        if st != W2R_OK {
            log(W2R_LOG_WARN, &format!("declaring {key}: status {st}"));
            all = false;
        }
    };

    // The dropdown: its labels must outlive the call only, so they live on this stack frame.
    let labels = model::choice_labels(profiles);
    let strs: Vec<W2rStr> = labels.iter().map(|l| s(l)).collect();
    let mut d = def(
        W2R_SETTING_CHOICE,
        model::PROFILE_KEY,
        "Color Profile",
        "Choose a profile and press Apply. The swatches below show its colors the next time you open this screen. The list is read when the game starts.",
        PROFILE_SUBCATEGORY,
    );
    d.default_int = model::CHOICE_GAME_DEFAULT;
    d.choices = strs.as_ptr();
    d.choice_count = strs.len() as u32;
    submit(&d, model::PROFILE_KEY);
    *LISTED.lock().unwrap_or_else(|e| e.into_inner()) = profiles
        .iter()
        .take(model::MAX_LISTED)
        .map(|p| p.name.clone())
        .collect();

    // Saving from the options screen: a checkbox that makes the next Apply save, and the name and
    // author it saves under (greyed while the box is off). The plugin clears the box after.
    let d = def(
        W2R_SETTING_BOOL,
        model::SAVE_KEY,
        "Save Colors as Profile on Apply",
        "Check, fill in a name, then press Apply: the colors below are saved to color_profiles.json, replacing a profile of the same name. A new name joins the Color Profile list at the next start.",
        SAVE_SUBCATEGORY,
    );
    submit(&d, model::SAVE_KEY);
    let mut d = def(
        W2R_SETTING_STRING,
        model::NAME_KEY,
        "Profile Name",
        "The name the profile is saved under. Choosing a profile fills it in.",
        SAVE_SUBCATEGORY,
    );
    d.active_when = s(model::SAVE_KEY);
    submit(&d, model::NAME_KEY);
    let mut d = def(
        W2R_SETTING_STRING,
        model::AUTHOR_KEY,
        "Author",
        "Who made the profile; shown next to its name. Optional.",
        SAVE_SUBCATEGORY,
    );
    d.active_when = s(model::SAVE_KEY);
    submit(&d, model::AUTHOR_KEY);

    for (i, rgb) in stock.iter().enumerate() {
        let key = model::key(i);
        let label = format!("Player {} ({})", i + 1, model::NAMES[i]);
        let description = format!(
            "The {} team color on your screen only. Pick the game's own color to leave it unchanged.",
            model::NAMES[i]
        );
        let default = model::to_setting(*rgb);
        let mut d = def(W2R_SETTING_COLOR, &key, &label, &description, SUBCATEGORY);
        d.default_str = s(&default);
        submit(&d, &key);
    }
    all
}

/// Stores the dropdown index that describes the colours in use, so the options screen shows the
/// right entry the next time it opens.
fn sync_choice(profiles: &[Profile]) {
    if !DECLARED.load(Ordering::Relaxed) {
        return;
    }
    let cur = colors();
    let matched = model::matching(profiles, &cur, &stock()).map(|p| p.name.as_str());
    let listed = LISTED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let index = model::choice_for(&listed, cur == [None; PLAYERS], matched);
    let mut last = LAST_CHOICE.lock().unwrap_or_else(|e| e.into_inner());
    if *last != index {
        if let Some(h) = host() {
            unsafe { (h.settings_set_i64)(h.token, s(model::PROFILE_KEY), index) };
        }
        *last = index;
    }
}

/// The dropdown index stored now (the options screen writes it on Apply).
fn stored_choice() -> i64 {
    let Some(h) = host() else {
        return model::CHOICE_CUSTOM;
    };
    let mut v = model::CHOICE_CUSTOM;
    if unsafe { (h.settings_get_i64)(h.token, s(model::PROFILE_KEY), &mut v) } != W2R_OK {
        return model::CHOICE_CUSTOM;
    }
    v
}

// ---- the profiles file -----------------------------------------------------------------------

/// `Ok(None)` when the file does not exist.
fn read_file(name: &str) -> Result<Option<String>, String> {
    let h = host().ok_or("no host")?;
    let mut handle = 0;
    match unsafe { (h.data_open)(h.token, s(name), W2R_FILE_READ, &mut handle) } {
        W2R_OK => {}
        W2R_E_NOT_FOUND => return Ok(None),
        e => return Err(format!("cannot open {name} (status {e})")),
    }
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let result = loop {
        let mut got = 0u32;
        let st = unsafe {
            (h.data_read)(
                h.token,
                handle,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut got,
            )
        };
        if st != W2R_OK {
            break Err(format!("cannot read {name} (status {st})"));
        }
        if got == 0 {
            break Ok(());
        }
        out.extend_from_slice(&chunk[..(got as usize).min(chunk.len())]);
        if out.len() > MAX_FILE {
            break Err(format!("{name} is larger than 1 MiB"));
        }
    };
    unsafe { (h.data_close)(h.token, handle) };
    result?;
    String::from_utf8(out)
        .map(Some)
        .map_err(|_| format!("{name} is not UTF-8 text"))
}

fn write_file(name: &str, text: &str) -> Result<(), String> {
    let h = host().ok_or("no host")?;
    let mut handle = 0;
    let st = unsafe { (h.data_open)(h.token, s(name), W2R_FILE_WRITE, &mut handle) };
    if st != W2R_OK {
        return Err(format!("cannot create {name} (status {st})"));
    }
    let st = unsafe { (h.data_write)(h.token, handle, text.as_ptr().cast(), text.len() as u32) };
    unsafe { (h.data_close)(h.token, handle) };
    if st == W2R_OK {
        Ok(())
    } else {
        Err(format!("cannot write {name} (status {st})"))
    }
}

/// What reading the profiles file gave: the profiles, a status line, and whether the file may be
/// written (false when it exists but could not be read).
struct Loaded {
    profiles: Vec<Profile>,
    note: String,
    file_ok: bool,
}

/// Reads `color_profiles.json`. The first time, it is created from the old `color_profiles.ini`
/// if that exists, and from the default profiles otherwise.
fn load_profiles() -> Loaded {
    match read_file(PROFILES_FILE) {
        Ok(Some(text)) => match profiles::parse(&text) {
            Ok((profiles, warnings)) => {
                for w in warnings.iter().take(5) {
                    log(
                        W2R_LOG_WARN,
                        &format!("{PROFILES_FILE} profile {}: {}", w.entry, w.what),
                    );
                }
                let note = match warnings.first() {
                    None => format!("{} profile(s) in {PROFILES_FILE}", profiles.len()),
                    Some(w) => format!(
                        "{} profile(s); skipped {}, first: profile {} ({})",
                        profiles.len(),
                        warnings.len(),
                        w.entry,
                        w.what
                    ),
                };
                Loaded {
                    profiles,
                    note,
                    file_ok: true,
                }
            }
            Err(e) => {
                let note = format!("{PROFILES_FILE} could not be read ({e}); fix it, then Reload");
                log(W2R_LOG_WARN, &note);
                Loaded {
                    profiles: Vec::new(),
                    note,
                    file_ok: false,
                }
            }
        },
        Ok(None) => {
            let (profiles, from) = match read_file(OLD_PROFILES_FILE) {
                Ok(Some(text)) => (ini::parse(&text).0, format!("from {OLD_PROFILES_FILE}")),
                _ => (
                    model::default_profiles(),
                    "with the default profiles".to_string(),
                ),
            };
            let note = match save_profiles(&profiles) {
                Ok(()) => format!("Created {PROFILES_FILE} {from} ({})", profiles.len()),
                Err(e) => e,
            };
            Loaded {
                profiles,
                note,
                file_ok: true,
            }
        }
        Err(e) => Loaded {
            profiles: Vec::new(),
            note: e,
            file_ok: false,
        },
    }
}

/// Writes `profiles` to `color_profiles.json`.
fn save_profiles(profiles: &[Profile]) -> Result<(), String> {
    write_file(PROFILES_FILE, &profiles::write(profiles))
}

/// Reads the game's own colours from the HD table. Called in `on_load`, before the tick has
/// written anything.
fn read_stock(game: &Game) -> [Rgb; PLAYERS] {
    let mut out = [[0u8; 3]; PLAYERS];
    for (i, rgb) in out.iter_mut().enumerate() {
        // SAFETY: the table holds four floats for each of the eight colour indices.
        *rgb = unsafe {
            let t = game.table.add(i * TABLE_STRIDE);
            model::rgb_of([*t, *t.add(1), *t.add(2)])
        };
    }
    model::stock_or_default(out)
}

// ---- the tick --------------------------------------------------------------------------------

unsafe fn read_ramp(live: *const u8) -> [Rgb; RAMP] {
    let mut out = [[0u8; 3]; RAMP];
    for (k, entry) in out.iter_mut().enumerate() {
        for (c, v) in entry.iter_mut().enumerate() {
            *v = unsafe { *live.add(k * PALETTE_STRIDE + c) };
        }
    }
    out
}

unsafe fn write_ramp(live: *mut u8, ramp: &[Rgb; RAMP]) {
    for (k, entry) in ramp.iter().enumerate() {
        for (c, v) in entry.iter().enumerate() {
            unsafe { *live.add(k * PALETTE_STRIDE + c) = *v };
        }
    }
}

/// Brings both tables in line with `wanted`. Cheap when nothing differs.
unsafe fn apply(game: &Game, wanted: &Colors) {
    let mut slots = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
    // The render palette is not allocated early on.
    let palette = unsafe { *game.palette };
    for (i, slot) in slots.iter_mut().enumerate() {
        let target = wanted[i];
        unsafe {
            // HD table entry. Nothing is written for a colour never chosen.
            let t = game.table.add(i * TABLE_STRIDE);
            if slot.stock_table.is_none() && target.is_some() {
                slot.stock_table = Some([*t, *t.add(1), *t.add(2), *t.add(3)]);
            }
            if let Some(stock) = slot.stock_table {
                let mut want = stock;
                if let Some(rgb) = target {
                    want[..3].copy_from_slice(&model::float_rgb(rgb));
                }
                for (c, w) in want.iter().enumerate() {
                    if *t.add(c) != *w {
                        *t.add(c) = *w;
                    }
                }
            }

            // Render palette ramp (red to white only, see `model::ramp_base`).
            let Some(base) = model::ramp_base(i) else {
                continue;
            };
            if palette.is_null() {
                continue;
            }
            let live = palette.add(base * PALETTE_STRIDE);
            if slot.owns_ramp && read_ramp(live) != slot.written_ramp {
                // The palette was rebuilt: what is there now is the new stock.
                slot.owns_ramp = false;
            }
            match target {
                Some(rgb) => {
                    if !slot.owns_ramp {
                        slot.stock_ramp = read_ramp(live);
                    }
                    let ramp = model::ramp(&slot.stock_ramp, rgb);
                    if !slot.owns_ramp || read_ramp(live) != ramp {
                        write_ramp(live, &ramp);
                        slot.written_ramp = ramp;
                        slot.owns_ramp = true;
                    }
                }
                None => {
                    if slot.owns_ramp {
                        write_ramp(live, &slot.stock_ramp);
                        slot.owns_ramp = false;
                    }
                }
            }
        }
    }
}

unsafe extern "C" fn menu_buttons_detour(view: i32) {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original != 0 {
        let original: MenuButtonsFn = unsafe { core::mem::transmute(original) };
        unsafe { original(view) };
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(game) = GAME.get() {
            unsafe { apply(game, &colors()) };
        }
    }));
}

// ---- the plugin manager card -----------------------------------------------------------------

fn fill(buf: &mut [u8], text: &str) {
    buf.fill(0);
    let n = text.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&text.as_bytes()[..n]);
}

fn text_of(buf: &[u8]) -> String {
    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// Shows the colours in use in the card's boxes: the chosen colour, or the game's own where
/// there is none, as the options screen does. (Not called while the player types, so a box can be
/// emptied.)
fn refill(page: &mut Page, c: &Colors) {
    let stock = stock();
    for ((buf, rgb), own) in page.hex.iter_mut().zip(c).zip(stock) {
        fill(buf, &model::to_hex(rgb.unwrap_or(own)));
    }
}

fn selected(page: &Page) -> Option<&Profile> {
    let i = usize::try_from(page.selected).ok()?.checked_sub(1)?;
    page.profiles.get(i)
}

/// Puts the selected profile's name and author in their boxes, ready to save over it.
fn fill_names(page: &mut Page) {
    let (name, author) = match selected(page) {
        Some(p) => (p.name.clone(), p.author.clone()),
        None => (String::new(), String::new()),
    };
    fill(&mut page.name, &name);
    fill(&mut page.author, &author);
}

/// What the "In use" line says: Game default, the matching profile, or Custom colors.
fn in_use(page: &Page) -> String {
    let cur = colors();
    if cur == [None; PLAYERS] {
        return "Game default".into();
    }
    match model::matching(&page.profiles, &cur, &stock()) {
        Some(p) => model::title(p),
        None => "Custom colors".into(),
    }
}

fn new_page() -> Page {
    let loaded = load_profiles();
    let status = loaded.note;
    let mut page = Page {
        profiles: loaded.profiles,
        file_ok: loaded.file_ok,
        selected: 0,
        hex: [[0; HEX_CAP]; PLAYERS],
        name: [0; NAME_CAP],
        author: [0; NAME_CAP],
        status,
    };
    refill(&mut page, &colors());
    page
}

/// The host refuses UI strings over 128 bytes, so longer text is cut on a character boundary.
fn fit(text: &str) -> &str {
    let mut n = text.len().min(128);
    while !text.is_char_boundary(n) {
        n -= 1;
    }
    &text[..n]
}

unsafe fn label(ui: &W2rUi, text: &str) {
    unsafe { (ui.label)(ui.ctx, s(fit(text))) };
}

unsafe fn button(ui: &W2rUi, text: &str) -> bool {
    unsafe { (ui.button)(ui.ctx, s(text)) != 0 }
}

/// Applies colours chosen on the card and shows them in the boxes.
fn choose(page: &mut Page, c: Colors) {
    set_colors(c);
    refill(page, &c);
    sync_choice(&page.profiles);
}

/// Saves the colours in use as a profile named by the name and author boxes.
fn save_current(page: &mut Page) -> String {
    let name = model::clean_name(&text_of(&page.name), "");
    if name.is_empty() {
        return "Type a profile name first".into();
    }
    if !page.file_ok {
        return format!("Not saved: {PROFILES_FILE} could not be read; fix it, then Reload");
    }
    // Saving over a profile keeps the fields this plugin does not use (highlights, bars).
    let extra = page
        .profiles
        .iter()
        .find(|o| o.name.eq_ignore_ascii_case(&name))
        .map(|o| o.extra.clone())
        .unwrap_or_default();
    let p = Profile {
        name: name.clone(),
        author: model::clean_name(&text_of(&page.author), ""),
        colors: colors(),
        extra,
    };
    let title = model::title(&p);
    let (p_name, p_author) = (p.name.clone(), p.author.clone());
    let mut next = page.profiles.clone();
    if let Err(e) = profiles::upsert(&mut next, p) {
        return format!("Not saved: {e}");
    }
    if let Err(e) = save_profiles(&next) {
        return format!("Not saved: {e}");
    }
    page.profiles = next;
    if let Some(i) = page.profiles.iter().position(|o| o.name == name) {
        page.selected = i as i32 + 1;
    }
    sync_choice(&page.profiles);
    prefill_names(&p_name, &p_author);
    format!("Saved {title} to {PROFILES_FILE}")
}

unsafe fn page_ui(ui: &W2rUi, page: &mut Page) {
    let edits = ui.struct_size >= W2R_UI_TEXT_EDIT_SIZE;
    unsafe {
        // The host draws plain text only, so the line is set apart by its shape.
        label(ui, &format!(">>  IN USE:  {}  <<", in_use(page)));

        if DECLARED.load(Ordering::Relaxed) {
            label(
                ui,
                "Also in Options > Unit Color: color pickers and the Color Profile list.",
            );
        }
        // Text boxes until the host offers a color swatch for plugin cards. The options screen's
        // pickers show a change made here the next time it opens.
        if edits {
            label(ui, "Type #RRGGBB, or clear a box for the game's own color.");
            for i in 0..PLAYERS {
                let caption = format!("Player {} ({})", i + 1, model::NAMES[i]);
                let buf = &mut page.hex[i];
                if (ui.text_edit)(ui.ctx, s(&caption), buf.as_mut_ptr(), HEX_CAP as u32) != 0 {
                    match model::parse_entry(&text_of(buf)) {
                        Ok(c) => {
                            let mut next = colors();
                            next[i] = c;
                            set_colors(next);
                            sync_choice(&page.profiles);
                            page.status.clear();
                        }
                        Err(model::NotAColor) => {
                            page.status = format!("{}: not a #RRGGBB color yet", model::NAMES[i])
                        }
                    }
                }
            }
        } else {
            label(
                ui,
                "This w2r is too old for color boxes; use the profiles below.",
            );
        }

        // Profiles: 0 is the game's own colours, then color_profiles.json in file order. Reload
        // re-reads the file, so edits show here at once.
        let last = page.profiles.len() as i32;
        page.selected = page.selected.clamp(0, last);
        if last > 0 && (ui.slider_int)(ui.ctx, s("Profile"), &mut page.selected, 0, last) != 0 {
            fill_names(page);
        }
        let title = selected(page).map_or_else(|| "Game default".to_string(), model::title);
        label(ui, &format!("Profile: {title}"));
        if button(ui, "Apply profile") {
            let c = selected(page).map_or([None; PLAYERS], |p| p.colors);
            if let Some(p) = selected(page) {
                let (n, a) = (p.name.clone(), p.author.clone());
                prefill_names(&n, &a);
            }
            choose(page, c);
            page.status = format!("Applied {title}");
        }

        if edits {
            let buf = &mut page.name;
            (ui.text_edit)(ui.ctx, s("Profile name"), buf.as_mut_ptr(), NAME_CAP as u32);
            let buf = &mut page.author;
            (ui.text_edit)(ui.ctx, s("Author"), buf.as_mut_ptr(), NAME_CAP as u32);
            if button(ui, "Save current colors as this profile") {
                page.status = save_current(page);
            }
        }

        if button(ui, &format!("Reload {PROFILES_FILE}")) {
            let loaded = load_profiles();
            page.profiles = loaded.profiles;
            page.file_ok = loaded.file_ok;
            page.status = loaded.note;
        }
        if button(ui, "Reset all to game default") {
            choose(page, [None; PLAYERS]);
            page.status = "All colors back to the game's own".into();
        }
        if !page.status.is_empty() {
            label(ui, &page.status);
        }
        label(
            ui,
            "File: plugins\\w2r-unit-color\\data\\color_profiles.json in your w2r folder.",
        );
    }
}

unsafe extern "C" fn on_ui(ui: *const W2rUi) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(ui) = (unsafe { ui.as_ref() }) else {
            return;
        };
        let mut guard = PAGE.lock().unwrap_or_else(|e| e.into_inner());
        let page = guard.get_or_insert_with(new_page);
        unsafe { page_ui(ui, page) };
    }));
}

/// The options screen stored new colours (Apply or Save and Exit).
unsafe extern "C" fn on_event(event: *const W2rEvent) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(event) = (unsafe { event.as_ref() }) else {
            return;
        };
        if event.id == W2R_EVT_SETTINGS_CHANGED {
            settings_changed();
        }
    }));
}

/// After Apply on the options screen. A new pick in the profile dropdown wins over colour edits
/// made in the same visit (the screen cannot show a profile's colours until it is opened again);
/// otherwise the colours are taken as edited and the dropdown is set to what they match.
fn settings_changed() {
    let mut guard = PAGE.lock().unwrap_or_else(|e| e.into_inner());
    let page = guard.get_or_insert_with(new_page);
    let picked = stored_choice();
    let last = *LAST_CHOICE.lock().unwrap_or_else(|e| e.into_inner());
    let listed = LISTED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let profile = if picked != last {
        model::choice_target(&listed, picked)
    } else {
        None
    };
    let next = match profile {
        Some(None) => [None; PLAYERS],
        Some(Some(name)) => match page
            .profiles
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
        {
            Some(p) => p.colors,
            None => {
                page.status = format!("{name} is no longer in {PROFILES_FILE}");
                log(W2R_LOG_WARN, &page.status);
                load_colors()
            }
        },
        None => load_colors(),
    };
    // The screen just stored the index it shows; start from that so a later sync compares right.
    *LAST_CHOICE.lock().unwrap_or_else(|e| e.into_inner()) = picked;
    set_colors(next);
    refill(page, &next);
    if let Some(Some(name)) = profile
        && let Some(p) = page
            .profiles
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    {
        let (n, a) = (p.name.clone(), p.author.clone());
        prefill_names(&n, &a);
    }
    if get_i64(model::SAVE_KEY) == 1 {
        set_i64(model::SAVE_KEY, 0);
        let name = get_str(model::NAME_KEY);
        let author = get_str(model::AUTHOR_KEY);
        fill(&mut page.name, &name);
        fill(&mut page.author, &author);
        page.status = save_current(page);
        log(W2R_LOG_INFO, &page.status);
    }
    sync_choice(&page.profiles);
}

/// Puts a profile's name and author in the options screen's save fields (shown on the next
/// visit), so editing a profile is: choose it, reopen, change colors, check Save, Apply.
fn prefill_names(name: &str, author: &str) {
    if !DECLARED.load(Ordering::Relaxed) {
        return;
    }
    set_str(model::NAME_KEY, name);
    set_str(model::AUTHOR_KEY, author);
}

fn get_i64(key: &str) -> i64 {
    let Some(h) = host() else { return 0 };
    let mut v = 0i64;
    if unsafe { (h.settings_get_i64)(h.token, s(key), &mut v) } != W2R_OK {
        return 0;
    }
    v
}

fn set_i64(key: &str, value: i64) {
    if let Some(h) = host() {
        unsafe { (h.settings_set_i64)(h.token, s(key), value) };
    }
}

// ---- load and unload -------------------------------------------------------------------------

fn remove_hook() {
    let hook = HOOK.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let (Some(h), Some(hook)) = (host(), hook) {
        unsafe { (h.hook_remove)(h.token, hook) };
    }
}

unsafe extern "C" fn on_load(host: *const W2rHost) -> W2rStatus {
    catch_unwind(AssertUnwindSafe(|| unsafe { load(host) })).unwrap_or(W2R_E_STATE)
}

unsafe fn load(host: *const W2rHost) -> W2rStatus {
    HOST.store(host.cast_mut(), Ordering::Release);
    let (Some(table), Some(palette)) = (lookup("TeamColourRgba"), lookup("RenderPalette")) else {
        log(
            W2R_LOG_ERROR,
            "TeamColourRgba or RenderPalette not available; no recolor",
        );
        return W2R_E_STATE;
    };
    let game = GAME.get_or_init(|| Game {
        table: table as *mut f32,
        palette: palette as *const *mut u8,
    });
    // The stock colours first: they are the pickers' defaults and mean "no change".
    let stock = read_stock(game);
    *STOCK.lock().unwrap_or_else(|e| e.into_inner()) = stock;
    // Reads (or creates) color_profiles.json now: the dropdown lists its profiles, and a broken
    // file shows in the log at start.
    let page = new_page();
    log(W2R_LOG_INFO, &page.status);
    // Declarations are only accepted while on_load runs.
    DECLARED.store(declare(&stock, &page.profiles), Ordering::Relaxed);
    *LAST_CHOICE.lock().unwrap_or_else(|e| e.into_inner()) = stored_choice();
    let wanted = load_colors();
    // Rewrite anything stored in another form (`#RRGGBB` from 0.1.0) the way the picker stores it.
    set_colors(wanted);
    sync_choice(&page.profiles);
    let mut page = page;
    refill(&mut page, &wanted);
    *PAGE.lock().unwrap_or_else(|e| e.into_inner()) = Some(page);
    let Some(h) = self::host() else {
        return W2R_E_STATE;
    };
    unsafe { (h.subscribe)(h.token, W2R_EVT_SETTINGS_CHANGED) };
    let Some(target) = handle_of("HudMenuButtons") else {
        log(
            W2R_LOG_ERROR,
            "symbol HudMenuButtons not available; no recolor",
        );
        return W2R_E_STATE;
    };
    let mut trampoline: *mut c_void = core::ptr::null_mut();
    let mut hook = 0;
    let status = unsafe {
        (h.hook_install)(
            h.token,
            target,
            menu_buttons_detour as *mut c_void,
            &mut trampoline,
            &mut hook,
        )
    };
    if status != W2R_OK {
        // W2R_E_CONFLICT: most likely Black Recolor is still enabled; it hooks the same function.
        log(
            W2R_LOG_ERROR,
            &format!("HudMenuButtons: hook_install returned {status}; no recolor"),
        );
        return W2R_E_STATE;
    }
    ORIGINAL.store(trampoline as usize, Ordering::Release);
    *HOOK.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    log(W2R_LOG_INFO, "unit color ready");
    W2R_OK
}

unsafe extern "C" fn on_unload() {
    remove_hook();
    // Put the stock colours back (without touching the saved settings).
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(game) = GAME.get() {
            unsafe { apply(game, &[None; PLAYERS]) };
        }
    }));
}

static TABLE: W2rPlugin = W2rPlugin {
    struct_size: W2R_PLUGIN_V1_SIZE,
    on_load: Some(on_load),
    on_event: Some(on_event),
    on_ui: Some(on_ui),
    on_unload: Some(on_unload),
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w2r_plugin_main(_host: *const W2rHost) -> *const W2rPlugin {
    &TABLE
}
