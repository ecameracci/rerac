//! **Port-only** (not in the game): the main menu's "Mods" page. One row per mod in the mods folder, its name left
//! and On / Off right; ✕ toggles it. The engine owns the folder and the setting (rc-engine `mods`): it fills the rows
//! before each front-end menu tick ([`PageMenu::set_mod_rows`]) and reads the toggles after it
//! ([`PageMenu::mod_states`]).
//!
//! Built like the Port Options' main-menu page ([`super::port`]): the front end's Sound sub-page frames with the list
//! panel stretched ([`super::port::panel_stretch`]), a copy of the port title label, a list drawn and driven like
//! the camera list (Up/Down without wrap with sound 1, ✕ with sound 0, the generic Start/Select/R3 and △ keys).
//! Entry: one item "Mods" (action 3 → [`PAGE`]) appended to both of the front end's Options lists, after Port
//! Options. Records live at port-only addresses (0x7f00_xxxx), the text ids are negative ([`super::port::text`]).

use super::super::{text as print, MenuAssets, MenuDraw, MenuInput, MenuSound};
use super::options::{keys, right};
use super::{port, Data, Item, MenuOut, Page, PageMenu, Widget, LIGHT_BLUE, YELLOW};
use crate::pad::button;
use rc_formats::font::Font;

/// The page and its two own widgets (title label, mods list).
pub const PAGE: u32 = 0x7f00_0020;
pub const TITLE_W: u32 = 0x7f00_0110;
pub const LIST_W: u32 = 0x7f00_0210;
/// The list's callbacks (dispatched like the game's by address).
pub const UPDATE: u32 = 0x7f00_1008;
pub const DRAW: u32 = 0x7f00_100c;
/// The longest name drawn (the row's left part).
const NAME_MAX: usize = 32;

/// The list's run-time state: cursor, first row shown, each mod's name and whether it is on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModList {
    pub cursor: i32,
    pub top: i32,
    pub names: Vec<Vec<u8>>,
    pub on: Vec<bool>,
}

fn list(m: &PageMenu) -> Option<&ModList> {
    match m.widgets.get(&LIST_W).map(|w| &w.data) {
        Some(Data::Mods(p)) => Some(p),
        _ => None,
    }
}

fn list_mut(m: &mut PageMenu) -> Option<&mut ModList> {
    match m.widgets.get_mut(&LIST_W).map(|w| &mut w.data) {
        Some(Data::Mods(p)) => Some(p),
        _ => None,
    }
}

/// A name as the font can draw it: printable ASCII, at most [`NAME_MAX`] characters.
fn display_name(name: &str) -> Vec<u8> {
    let mut b: Vec<u8> = name.bytes().map(|c| if (0x20..0x7f).contains(&c) { c } else { b'?' }).collect();
    if b.len() > NAME_MAX {
        b.truncate(NAME_MAX - 3);
        b.extend_from_slice(b"...");
    }
    b
}

/// The rows a list panel `h` pixels high shows (the Port Options' spacing).
fn visible(h: i32, n: usize) -> usize { if h <= 0 { port::VISIBLE.min(n.max(1)) } else { ((h / port::ROW_STEP - 1).max(1) as usize).min(n.max(1)) } }

impl PageMenu {
    /// Adds the page, its widgets and the entry of both front-end Options lists. Needs the Port Options' main-menu
    /// page ([`PageMenu::install_port_page`]), whose frames and title it copies. False when it could not be added.
    pub fn install_mods_page(&mut self) -> bool {
        if self.pages.contains_key(&PAGE) { return true; }
        let Some(model) = self.pages.get(&port::FRONT_PAGE).cloned() else { return false };
        let Some(mut title) = self.widgets.get(&port::TITLE_W).cloned() else { return false };
        let Data::Label(l) = &mut title.data else { return false };
        l.id = port::text::MODS as u32;
        title.addr = TITLE_W;
        let mods = Widget {
            addr: LIST_W,
            update: UPDATE,
            draw: DRAW,
            enter: 0,
            leave: 0,
            dflags: 0,
            moby: 0,
            rect: [0; 4],
            raw: [0; 8],
            data: Data::Mods(ModList::default()),
        };
        let mut widgets = model.widgets;
        widgets[2] = TITLE_W;
        widgets[port::LIST_SLOT] = LIST_W;
        let page = Page { addr: PAGE, focus: LIST_W, widgets, ..model };
        for list in &mut self.saves.front_options {
            list.push(Item { label: port::text::MODS as i16, action: 3, arg: PAGE, sublabel: 0, hl: 0 });
        }
        self.widgets.insert(TITLE_W, title);
        self.widgets.insert(LIST_W, mods);
        self.pages.insert(PAGE, page);
        true
    }

    /// The rows: each mod's name (as found) and whether it is on. The cursor stays on the list.
    pub fn set_mod_rows(&mut self, names: &[&str], on: &[bool]) {
        let Some(p) = list_mut(self) else { return };
        if p.names.len() != names.len() || p.names.iter().zip(names).any(|(a, b)| *a != display_name(b)) {
            p.names = names.iter().map(|n| display_name(n)).collect();
        }
        p.on.clear();
        p.on.extend((0..names.len()).map(|i| on.get(i).copied().unwrap_or(false)));
        p.cursor = p.cursor.clamp(0, (names.len() as i32 - 1).max(0));
    }

    /// Whether each row is on (None without the page).
    pub fn mod_states(&self) -> Option<&[bool]> { list(self).map(|p| p.on.as_slice()) }
}

/// The list update, as the Port Options' (the camera list 0x294cc0); ✕ toggles the row's mod.
pub fn update(m: &mut PageMenu, w: u32, inp: &MenuInput, out: &mut MenuOut) -> i32 {
    if !m.is_focus(w) { return 0; }
    if let Some(r) = keys(m, inp, 1) { return r; }
    let h = m.widgets.get(&w).map_or(0, |wd| wd.rect[3]);
    let Some(p) = list_mut(m) else { return 0 };
    let n = p.names.len();
    if n == 0 { return 0; }
    let shown = visible(h, n) as i32;
    let start = p.cursor;
    if inp.pressed_u & button::UP != 0 && p.cursor != 0 { p.cursor -= 1; }
    if inp.pressed_u & button::DOWN != 0 && (p.cursor as usize + 1) < n { p.cursor += 1; }
    if p.cursor != start { out.sounds.push(MenuSound::Cursor); }
    p.top = p.top.clamp(p.cursor - shown + 1, p.cursor).max(0);
    if inp.pressed_u & button::CROSS != 0 {
        if let Some(v) = p.on.get_mut(p.cursor as usize) {
            *v = !*v;
            out.sounds.push(MenuSound::Confirm);
        }
    }
    0
}

/// The list draw, as the Port Options' (panel-local); without mods, where to put them.
pub fn draw(m: &mut PageMenu, w: u32, a: &MenuAssets, out: &mut Vec<MenuDraw>) -> u32 {
    let Some(wd) = m.widgets.get(&w) else { return 1 };
    let [_, _, ww, wh] = wd.rect;
    let Data::Mods(p) = &wd.data else { return 1 };
    if p.names.is_empty() {
        let step = wh / 3;
        print(out, Font::Regular, 0xc, step - 8, YELLOW, a.msg(port::text::MODS_NONE));
        print(out, Font::Regular, 0xc, 2 * step - 8, LIGHT_BLUE, a.msg(port::text::MODS_WHERE));
        return 2;
    }
    let shown = visible(wh, p.names.len());
    let step = wh / (shown as i32 + 1);
    let mut y = step - 8;
    let top = (p.top.max(0) as usize).min(p.names.len() - shown);
    for (i, name) in p.names.iter().enumerate().skip(top).take(shown) {
        let col = if i as i32 == p.cursor { YELLOW } else { LIGHT_BLUE };
        print(out, Font::Regular, 0xc, y, col, name);
        let v = if p.on.get(i).copied().unwrap_or(false) { port::text::ON } else { port::text::OFF };
        right(out, a, ww - 0xc, y, LIGHT_BLUE, a.msg(v));
        y += step;
    }
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_drawable_and_bounded() {
        assert_eq!(display_name("HD UI"), b"HD UI");
        assert_eq!(display_name("caf\u{e9}"), b"caf??");
        let long = "x".repeat(40);
        let d = display_name(&long);
        assert_eq!((d.len(), &d[NAME_MAX - 3..]), (NAME_MAX, &b"..."[..]));
    }

    #[test]
    fn rows_fit_the_panel() {
        assert_eq!(visible(0, 1), 1);
        assert_eq!(visible(0, 9), port::VISIBLE);
        assert_eq!(visible(220, 3), 3);
        assert_eq!(visible(220, 30), 10);
    }
}
