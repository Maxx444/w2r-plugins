//! The game-facing half (32-bit only). One detour on `HudMenuButtons` (the HUD menu buttons, drawn
//! every frame in a match; HudFoodCell, HudResourceCell and HudBeginFrame belong to other plugins
//! and a function takes one hook) is the per-frame tick: it calls the original, then makes the
//! eight entries of the HD colour table and the seven team ramps of the render palette match the
//! settings. Panics are caught before they unwind into the game (detours are outside the host's
//! crash guard, docs/abi.md).
//!
//! The settings page edits the colours (one box per colour) and the profiles kept in
//! `color_profiles.ini`, imports warcraft2.site presets from `presets.json` and exports
//! `export.json`. All three files live in the plugin's own data folder, the only folder a plugin
//! can write (docs/abi.md, `data_open`).

use core::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use w2r_abi::*;

use crate::ini;
use crate::model::{self, Colors, PLAYERS, Profile, RAMP, Rgb};

/// Bytes per render palette entry (RGBA).
const PALETTE_STRIDE: usize = 4;
/// Floats per HD colour table entry (RGBA).
const TABLE_STRIDE: usize = 4;

/// The profiles file, the warcraft2.site preset file read on Import, and the file Export writes.
const PROFILES_FILE: &str = "color_profiles.ini";
const IMPORT_FILE: &str = "presets.json";
const EXPORT_FILE: &str = "export.json";
/// Largest file read (64 full profiles are about 20 KiB).
const MAX_FILE: usize = 1 << 20;
/// Edit buffers: `#RRGGBB` plus room for stray spaces; a profile name or author.
const HEX_CAP: usize = 16;
const NAME_CAP: usize = model::MAX_NAME + 1;

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

/// The settings page.
struct Page {
    /// The profiles in `color_profiles.ini`, in file order.
    profiles: Vec<Profile>,
    /// Slider position: 0 is "Game default", n is `profiles[n - 1]`.
    selected: i32,
    /// The profile last applied, shown at the top; empty once a colour is edited by hand.
    active: String,
    hex: [[u8; HEX_CAP]; PLAYERS],
    name: [u8; NAME_CAP],
    author: [u8; NAME_CAP],
    status: String,
}

static GAME: OnceLock<Game> = OnceLock::new();
static HOST: AtomicPtr<W2rHost> = AtomicPtr::new(core::ptr::null_mut());
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<W2rHandle>> = Mutex::new(None);
/// The wanted colours; read by the tick, written by the page.
static COLORS: Mutex<Colors> = Mutex::new([None; PLAYERS]);
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

/// The colours in the settings; a value the user mangled by hand counts as "game default".
fn load_colors() -> Colors {
    let mut out = [None; PLAYERS];
    for (i, c) in out.iter_mut().enumerate() {
        *c = model::parse_entry(&get_str(&model::key(i))).unwrap_or(None);
    }
    out
}

/// Makes `next` the wanted colours, saving the ones that changed.
fn set_colors(next: Colors) {
    let mut cur = COLORS.lock().unwrap_or_else(|e| e.into_inner());
    for i in 0..PLAYERS {
        if cur[i] != next[i] {
            set_str(
                &model::key(i),
                &next[i].map(model::to_hex).unwrap_or_default(),
            );
        }
    }
    *cur = next;
}

// ---- data files ------------------------------------------------------------------------------

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

/// Reads `color_profiles.ini`, creating it with the default profiles the first time. Returns the
/// profiles and a status line.
fn load_profiles() -> (Vec<Profile>, String) {
    match read_file(PROFILES_FILE) {
        Ok(Some(text)) => {
            let (profiles, warnings) = ini::parse(&text);
            for w in warnings.iter().take(5) {
                log(
                    W2R_LOG_WARN,
                    &format!("{PROFILES_FILE} line {}: {}", w.line, w.what),
                );
            }
            let note = match warnings.first() {
                None => format!("{} profile(s) in {PROFILES_FILE}", profiles.len()),
                Some(w) => format!(
                    "{} profile(s); skipped {} line(s), first: line {} ({})",
                    profiles.len(),
                    warnings.len(),
                    w.line,
                    w.what
                ),
            };
            (profiles, note)
        }
        Ok(None) => {
            let profiles = model::default_profiles();
            let note = match write_file(PROFILES_FILE, &ini::write(&profiles)) {
                Ok(()) => format!("Created {PROFILES_FILE} with {} profiles", profiles.len()),
                Err(e) => e,
            };
            (profiles, note)
        }
        Err(e) => (Vec::new(), e),
    }
}

/// Writes `profiles` to `color_profiles.ini`.
fn save_profiles(profiles: &[Profile]) -> Result<(), String> {
    write_file(PROFILES_FILE, &ini::write(profiles))
}

/// The colour on screen for each index: the chosen one, or the game's own.
fn shown() -> [Rgb; PLAYERS] {
    let wanted = colors();
    let slots = *SLOTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = model::REMASTERED;
    for i in 0..PLAYERS {
        out[i] = match (wanted[i], slots[i].stock_table, GAME.get()) {
            (Some(rgb), _, _) => rgb,
            (None, Some(stock), _) => model::rgb_of([stock[0], stock[1], stock[2]]),
            // Never touched, so the live entry is the game's own.
            (None, None, Some(game)) => unsafe {
                let t = game.table.add(i * TABLE_STRIDE);
                model::rgb_of([*t, *t.add(1), *t.add(2)])
            },
            (None, None, None) => model::REMASTERED[i],
        };
    }
    out
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

// ---- the settings page -----------------------------------------------------------------------

fn fill(buf: &mut [u8], text: &str) {
    buf.fill(0);
    let n = text.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&text.as_bytes()[..n]);
}

fn text_of(buf: &[u8]) -> String {
    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// Shows `c` in the colour boxes (after a profile, an import or a reset).
fn refill(page: &mut Page, c: &Colors) {
    for (buf, rgb) in page.hex.iter_mut().zip(c) {
        fill(buf, &rgb.map(model::to_hex).unwrap_or_default());
    }
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

fn selected(page: &Page) -> Option<&Profile> {
    let i = usize::try_from(page.selected).ok()?.checked_sub(1)?;
    page.profiles.get(i)
}

/// Remembers which profile is in use (empty: none, the colours were edited by hand).
fn set_active(page: &mut Page, title: String) {
    set_str(model::PROFILE_KEY, &title);
    page.active = title;
}

fn new_page() -> Page {
    let (profiles, status) = load_profiles();
    let mut page = Page {
        profiles,
        selected: 0,
        active: model::clean_name(&get_str(model::PROFILE_KEY), ""),
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

/// Applies colours chosen on the page and shows them in the boxes.
fn choose(page: &mut Page, c: Colors, title: String) {
    set_colors(c);
    refill(page, &c);
    set_active(page, title);
}

/// Saves the colours on screen as a profile named by the name and author boxes.
fn save_current(page: &mut Page) -> String {
    let name = model::clean_name(&text_of(&page.name), "");
    if name.is_empty() {
        return "Type a profile name first".into();
    }
    let p = Profile {
        name: name.clone(),
        author: model::clean_name(&text_of(&page.author), ""),
        colors: colors(),
    };
    let title = model::title(&p);
    let mut next = page.profiles.clone();
    if let Err(e) = ini::upsert(&mut next, p) {
        return format!("Not saved: {e}");
    }
    if let Err(e) = save_profiles(&next) {
        return format!("Not saved: {e}");
    }
    page.profiles = next;
    if let Some(i) = page.profiles.iter().position(|o| o.name == name) {
        page.selected = i as i32 + 1;
    }
    set_active(page, title.clone());
    format!("Saved {title} to {PROFILES_FILE}")
}

/// Adds the presets in `presets.json` to the profiles (same name: replaced) and saves the file.
fn import(page: &mut Page) -> String {
    let list = match read_file(IMPORT_FILE) {
        Ok(Some(text)) => match model::profiles_from_json(&text) {
            Ok(list) => list,
            Err(e) => return format!("{IMPORT_FILE}: {e}"),
        },
        Ok(None) => return format!("Put {IMPORT_FILE} in the plugin's data folder first"),
        Err(e) => return e,
    };
    let mut next = page.profiles.clone();
    let mut added = 0;
    for p in list {
        if ini::upsert(&mut next, p).is_ok() {
            added += 1;
        }
    }
    if let Err(e) = save_profiles(&next) {
        return format!("Not imported: {e}");
    }
    page.profiles = next;
    format!("Imported {added} preset(s) into {PROFILES_FILE}")
}

fn export(page: &Page) -> String {
    let name = model::clean_name(&text_of(&page.name), "Exported colors");
    let author = model::clean_name(&text_of(&page.author), "");
    match write_file(EXPORT_FILE, &model::export_json(&name, &author, &shown())) {
        Ok(()) => format!("Wrote {EXPORT_FILE} to the plugin's data folder"),
        Err(e) => e,
    }
}

unsafe fn page_ui(ui: &W2rUi, page: &mut Page) {
    let edits = ui.struct_size >= W2R_UI_TEXT_EDIT_SIZE;
    unsafe {
        label(
            ui,
            "Team colors on your screen only; nobody else sees them.",
        );
        let in_use = if !page.active.is_empty() {
            page.active.clone()
        } else if colors() == [None; PLAYERS] {
            "Game default".into()
        } else {
            "Custom colors".into()
        };
        label(ui, &format!("In use: {in_use}"));

        if edits {
            for i in 0..PLAYERS {
                let caption = format!(
                    "Player {} {} (#RRGGBB, empty = game default)",
                    i + 1,
                    model::NAMES[i]
                );
                let buf = &mut page.hex[i];
                if (ui.text_edit)(ui.ctx, s(&caption), buf.as_mut_ptr(), HEX_CAP as u32) != 0 {
                    match model::parse_entry(&text_of(buf)) {
                        Ok(c) => {
                            let mut next = colors();
                            next[i] = c;
                            set_colors(next);
                            set_active(page, String::new());
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

        // Profiles: 0 is the game's own colours, then color_profiles.ini in file order.
        let last = page.profiles.len() as i32;
        page.selected = page.selected.clamp(0, last);
        if last > 0 && (ui.slider_int)(ui.ctx, s("Profile"), &mut page.selected, 0, last) != 0 {
            fill_names(page);
        }
        let title = selected(page).map_or_else(|| "Game default".to_string(), model::title);
        label(ui, &title);
        if button(ui, "Apply profile") {
            let c = selected(page).map_or([None; PLAYERS], |p| p.colors);
            choose(page, c, title.clone());
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

        if button(ui, "Reload color_profiles.ini") {
            let (profiles, note) = load_profiles();
            page.profiles = profiles;
            page.status = note;
        }
        if button(ui, "Import presets.json (warcraft2.site) into profiles") {
            page.status = import(page);
        }
        if button(ui, "Export current colors to export.json") {
            page.status = export(page);
        }
        if button(ui, "Reset all to game default") {
            choose(page, [None; PLAYERS], String::new());
            page.status = "All colors back to the game's own".into();
        }
        if !page.status.is_empty() {
            label(ui, &page.status);
        }
        label(
            ui,
            "Files: plugins\\w2r-unit-color\\data\\ in your w2r folder.",
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

unsafe extern "C" fn on_event(event: *const W2rEvent) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(event) = (unsafe { event.as_ref() }) else {
            return;
        };
        if event.id == W2R_EVT_SETTINGS_CHANGED {
            let next = load_colors();
            let changed = {
                let mut cur = COLORS.lock().unwrap_or_else(|e| e.into_inner());
                let changed = *cur != next;
                *cur = next;
                changed
            };
            if changed && let Some(page) = PAGE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                refill(page, &next);
            }
        }
    }));
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
    let _ = GAME.set(Game {
        table: table as *mut f32,
        palette: palette as *const *mut u8,
    });
    *COLORS.lock().unwrap_or_else(|e| e.into_inner()) = load_colors();
    // Reads (or creates) color_profiles.ini now, so a broken file shows in the log at start.
    let page = new_page();
    log(W2R_LOG_INFO, &page.status);
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
