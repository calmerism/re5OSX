//! SDL scancodes to the Windows virtual-key codes the engine's input block
//! uses (by physical key position, like the page's `KeyboardEvent.code` table).

use sdl3::keyboard::Scancode as Sc;

#[allow(clippy::just_underscores_and_digits)] // SDL names the digit keys _0.._9
#[rustfmt::skip]
pub fn vk(sc: Sc) -> Option<u8> {
    use Sc::*;
    Some(match sc {
        A => b'A', B => b'B', C => b'C', D => b'D', E => b'E', F => b'F', G => b'G', H => b'H', I => b'I',
        J => b'J', K => b'K', L => b'L', M => b'M', N => b'N', O => b'O', P => b'P', Q => b'Q', R => b'R',
        S => b'S', T => b'T', U => b'U', V => b'V', W => b'W', X => b'X', Y => b'Y', Z => b'Z',
        _1 => b'1', _2 => b'2', _3 => b'3', _4 => b'4', _5 => b'5', _6 => b'6', _7 => b'7', _8 => b'8',
        _9 => b'9', _0 => b'0',
        Return => 0x0D, Escape => 0x1B, Backspace => 0x08, Tab => 0x09, Space => 0x20,
        Minus => 0xBD, Equals => 0xBB, LeftBracket => 0xDB, RightBracket => 0xDD, Backslash => 0xDC,
        Semicolon => 0xBA, Apostrophe => 0xDE, Grave => 0xC0, Comma => 0xBC, Period => 0xBE, Slash => 0xBF,
        CapsLock => 0x14, NonUsBackslash => 0xE2,
        F1 => 0x70, F2 => 0x71, F3 => 0x72, F4 => 0x73, F5 => 0x74, F6 => 0x75, F7 => 0x76, F8 => 0x77,
        F9 => 0x78, F10 => 0x79, F11 => 0x7A, F12 => 0x7B,
        PrintScreen => 0x2C, ScrollLock => 0x91, Pause => 0x13,
        Insert => 0x2D, Home => 0x24, PageUp => 0x21, Delete => 0x2E, End => 0x23, PageDown => 0x22,
        Right => 0x27, Left => 0x25, Down => 0x28, Up => 0x26,
        NumLockClear => 0x90, KpDivide => 0x6F, KpMultiply => 0x6A, KpMinus => 0x6D, KpPlus => 0x6B,
        KpEnter => 0x0D, KpPeriod => 0x6E,
        Kp0 => 0x60, Kp1 => 0x61, Kp2 => 0x62, Kp3 => 0x63, Kp4 => 0x64, Kp5 => 0x65, Kp6 => 0x66,
        Kp7 => 0x67, Kp8 => 0x68, Kp9 => 0x69,
        LCtrl => 0xA2, LShift => 0xA0, LAlt => 0xA4, LGui => 0x5B,
        RCtrl => 0xA3, RShift => 0xA1, RAlt => 0xA5, RGui => 0x5C, Application => 0x5D,
        _ => return None,
    })
}

/// Generic modifier codes the engine also checks (VK_SHIFT, VK_CONTROL, VK_MENU).
pub fn generic(vk: u8) -> Option<u8> {
    match vk {
        0xA0 | 0xA1 => Some(0x10),
        0xA2 | 0xA3 => Some(0x11),
        0xA4 | 0xA5 => Some(0x12),
        _ => None,
    }
}

/// Like the browser page: I/K/J/L/U/O also press numpad 8/2/4/6/7/9, the
/// aircraft's pitch, roll and targeting, on keyboards without a numpad
/// (every MacBook). K presses numpad 5 too, pitch back in PC GTA V's own
/// bindings.
pub fn numpad_alias(sc: Sc) -> &'static [u8] {
    match sc {
        Sc::I => &[0x68],
        Sc::K => &[0x62, 0x65],
        Sc::J => &[0x64],
        Sc::L => &[0x66],
        Sc::U => &[0x67],
        Sc::O => &[0x69],
        _ => &[],
    }
}

/// What a key can press in the game instead of its own: a key (a Windows
/// virtual-key code) or a mouse button (`re5osx_runtime::input::mouse`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Key(u8),
    Mouse(u32),
}

/// A key standing in for another, as Settings › Controls keeps it: by the
/// names in [`mac_keys`] and [`game_keys`], so the settings file reads
/// plainly.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Remap {
    pub from: String,
    pub to: String,
}

impl Remap {
    pub fn new(from: &str, to: &str) -> Remap {
        Remap { from: from.into(), to: to.into() }
    }
}

/// The remaps a new player starts with: none. Every key does what it does
/// on PC (Caps Lock is the special ability); aiming on a trackpad is the
/// aim toggle's.
pub fn default_remaps() -> Vec<Remap> {
    Vec::new()
}

/// The keys a Mac keyboard has that can stand in for another (not ⌘: its
/// shortcuts are the app's).
pub fn mac_keys() -> Vec<(String, Sc)> {
    let mut keys: Vec<(String, Sc)> = [
        ("Caps Lock", Sc::CapsLock),
        ("Tab", Sc::Tab),
        ("`", Sc::Grave),
        ("Left Option", Sc::LAlt),
        ("Right Option", Sc::RAlt),
        ("Left Control", Sc::LCtrl),
        ("Right Shift", Sc::RShift),
        ("Return", Sc::Return),
        ("Space", Sc::Space),
        ("Left Arrow", Sc::Left),
        ("Right Arrow", Sc::Right),
        ("Up Arrow", Sc::Up),
        ("Down Arrow", Sc::Down),
        ("-", Sc::Minus),
        ("=", Sc::Equals),
        ("[", Sc::LeftBracket),
        ("]", Sc::RightBracket),
        ("\\", Sc::Backslash),
        (";", Sc::Semicolon),
        ("'", Sc::Apostrophe),
        (",", Sc::Comma),
        (".", Sc::Period),
        ("/", Sc::Slash),
    ]
    .map(|(n, s)| (n.to_string(), s))
    .into();
    keys.extend(LETTERS.iter().map(|&(c, sc)| (c.to_string(), sc)));
    keys.extend(DIGITS.iter().map(|&(c, sc)| (c.to_string(), sc)));
    keys.extend(FUNCTION.iter().enumerate().map(|(i, &sc)| (format!("F{}", i + 1), sc)));
    keys
}

/// What a Mac key can press in the game: the mouse's buttons, the keys
/// MacBooks lack (numpad, Insert, Home, Page Up...), and every other key.
pub fn game_keys() -> Vec<(String, Target)> {
    use re5osx_runtime::input::mouse;
    let mut keys: Vec<(String, Target)> = vec![
        ("Aim (right mouse)".into(), Target::Mouse(mouse::RIGHT)),
        ("Shoot (left mouse)".into(), Target::Mouse(mouse::LEFT)),
        ("Middle mouse".into(), Target::Mouse(mouse::MIDDLE)),
    ];
    keys.extend((0..10).map(|n| (format!("Numpad {n}"), Target::Key(0x60 + n))));
    keys.extend(
        [
            ("Numpad +", 0x6B),
            ("Numpad -", 0x6D),
            ("Numpad *", 0x6A),
            ("Numpad /", 0x6F),
            ("Numpad .", 0x6E),
            ("Insert", 0x2D),
            ("Delete", 0x2E),
            ("Home", 0x24),
            ("End", 0x23),
            ("Page Up", 0x21),
            ("Page Down", 0x22),
            ("Left Alt", 0xA4),
            ("Left Control", 0xA2),
            ("Left Shift", 0xA0),
            ("Space", 0x20),
            ("Tab", 0x09),
            ("Enter", 0x0D),
            ("Escape", 0x1B),
            ("Backspace", 0x08),
            ("Left Arrow", 0x25),
            ("Right Arrow", 0x27),
            ("Up Arrow", 0x26),
            ("Down Arrow", 0x28),
        ]
        .map(|(n, vk)| (n.to_string(), Target::Key(vk))),
    );
    keys.extend(LETTERS.iter().map(|&(c, _)| (c.to_string(), Target::Key(c as u8))));
    keys.extend(DIGITS.iter().map(|&(c, _)| (c.to_string(), Target::Key(c as u8))));
    keys.extend((0..12).map(|i| (format!("F{}", i + 1), Target::Key(0x70 + i as u8))));
    keys
}

/// What `sc` presses in the game under `remaps`, if anything stands in for it.
pub fn remapped(sc: Sc, remaps: &[Remap]) -> Option<Target> {
    let from = mac_keys().into_iter().find(|&(_, s)| s == sc)?.0;
    let to = &remaps.iter().find(|r| r.from == from)?.to;
    game_keys().into_iter().find(|(n, _)| n == to).map(|(_, t)| t)
}

const LETTERS: [(char, Sc); 26] = [
    ('A', Sc::A), ('B', Sc::B), ('C', Sc::C), ('D', Sc::D), ('E', Sc::E), ('F', Sc::F), ('G', Sc::G),
    ('H', Sc::H), ('I', Sc::I), ('J', Sc::J), ('K', Sc::K), ('L', Sc::L), ('M', Sc::M), ('N', Sc::N),
    ('O', Sc::O), ('P', Sc::P), ('Q', Sc::Q), ('R', Sc::R), ('S', Sc::S), ('T', Sc::T), ('U', Sc::U),
    ('V', Sc::V), ('W', Sc::W), ('X', Sc::X), ('Y', Sc::Y), ('Z', Sc::Z),
];

const DIGITS: [(char, Sc); 10] = [
    ('1', Sc::_1), ('2', Sc::_2), ('3', Sc::_3), ('4', Sc::_4), ('5', Sc::_5),
    ('6', Sc::_6), ('7', Sc::_7), ('8', Sc::_8), ('9', Sc::_9), ('0', Sc::_0),
];

const FUNCTION: [Sc; 12] =
    [Sc::F1, Sc::F2, Sc::F3, Sc::F4, Sc::F5, Sc::F6, Sc::F7, Sc::F8, Sc::F9, Sc::F10, Sc::F11, Sc::F12];
