//! The keys a remote keeper may press, named by `KeyboardEvent.code` (a key's place on the
//! keyboard, not the character it makes). Each is pressed by place on this machine too, so this
//! machine's own layout decides the character, as at its own keyboard.
//!
//! The codes follow Chromium's `dom_code_data.inc`. Keys that act on the machine rather than on
//! what is in front of the keeper (power, sleep, media, browser and launch keys) are left out on
//! purpose.

#[derive(Debug, PartialEq, Eq)]
pub struct Key {
    pub code: &'static str,
    /// Windows: the set-1 scan code, `0xe0` in the high byte for an extended key.
    pub scan: u16,
    /// Windows: a virtual key for the few keys a scan code cannot press reliably, else 0.
    pub vk: u16,
    /// Linux: the evdev code; X11 adds 8.
    pub evdev: u16,
    /// macOS: the virtual key code, where the Mac keyboard has the key.
    pub mac: Option<u16>,
    pub modifier: Option<Modifier>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Shift,
    Control,
    Alt,
    Meta,
}

pub fn named(code: &str) -> Option<&'static Key> {
    KEYS.iter().find(|key| key.code == code)
}

const fn key(code: &'static str, scan: u16, evdev: u16, mac: u16) -> Key {
    Key { code, scan, vk: 0, evdev, mac: Some(mac), modifier: None }
}

const fn pc_only(code: &'static str, scan: u16, vk: u16, evdev: u16) -> Key {
    Key { code, scan, vk, evdev, mac: None, modifier: None }
}

const fn modifier(code: &'static str, scan: u16, evdev: u16, mac: u16, modifier: Modifier) -> Key {
    Key { code, scan, vk: 0, evdev, mac: Some(mac), modifier: Some(modifier) }
}

pub static KEYS: &[Key] = &[
    key("KeyA", 0x1e, 30, 0x00),
    key("KeyB", 0x30, 48, 0x0b),
    key("KeyC", 0x2e, 46, 0x08),
    key("KeyD", 0x20, 32, 0x02),
    key("KeyE", 0x12, 18, 0x0e),
    key("KeyF", 0x21, 33, 0x03),
    key("KeyG", 0x22, 34, 0x05),
    key("KeyH", 0x23, 35, 0x04),
    key("KeyI", 0x17, 23, 0x22),
    key("KeyJ", 0x24, 36, 0x26),
    key("KeyK", 0x25, 37, 0x28),
    key("KeyL", 0x26, 38, 0x25),
    key("KeyM", 0x32, 50, 0x2e),
    key("KeyN", 0x31, 49, 0x2d),
    key("KeyO", 0x18, 24, 0x1f),
    key("KeyP", 0x19, 25, 0x23),
    key("KeyQ", 0x10, 16, 0x0c),
    key("KeyR", 0x13, 19, 0x0f),
    key("KeyS", 0x1f, 31, 0x01),
    key("KeyT", 0x14, 20, 0x11),
    key("KeyU", 0x16, 22, 0x20),
    key("KeyV", 0x2f, 47, 0x09),
    key("KeyW", 0x11, 17, 0x0d),
    key("KeyX", 0x2d, 45, 0x07),
    key("KeyY", 0x15, 21, 0x10),
    key("KeyZ", 0x2c, 44, 0x06),
    key("Digit1", 0x02, 2, 0x12),
    key("Digit2", 0x03, 3, 0x13),
    key("Digit3", 0x04, 4, 0x14),
    key("Digit4", 0x05, 5, 0x15),
    key("Digit5", 0x06, 6, 0x17),
    key("Digit6", 0x07, 7, 0x16),
    key("Digit7", 0x08, 8, 0x1a),
    key("Digit8", 0x09, 9, 0x1c),
    key("Digit9", 0x0a, 10, 0x19),
    key("Digit0", 0x0b, 11, 0x1d),
    key("Enter", 0x1c, 28, 0x24),
    key("Escape", 0x01, 1, 0x35),
    key("Backspace", 0x0e, 14, 0x33),
    key("Tab", 0x0f, 15, 0x30),
    key("Space", 0x39, 57, 0x31),
    key("Minus", 0x0c, 12, 0x1b),
    key("Equal", 0x0d, 13, 0x18),
    key("BracketLeft", 0x1a, 26, 0x21),
    key("BracketRight", 0x1b, 27, 0x1e),
    key("Backslash", 0x2b, 43, 0x2a),
    key("Semicolon", 0x27, 39, 0x29),
    key("Quote", 0x28, 40, 0x27),
    key("Backquote", 0x29, 41, 0x32),
    key("Comma", 0x33, 51, 0x2b),
    key("Period", 0x34, 52, 0x2f),
    key("Slash", 0x35, 53, 0x2c),
    key("CapsLock", 0x3a, 58, 0x39),
    key("F1", 0x3b, 59, 0x7a),
    key("F2", 0x3c, 60, 0x78),
    key("F3", 0x3d, 61, 0x63),
    key("F4", 0x3e, 62, 0x76),
    key("F5", 0x3f, 63, 0x60),
    key("F6", 0x40, 64, 0x61),
    key("F7", 0x41, 65, 0x62),
    key("F8", 0x42, 66, 0x64),
    key("F9", 0x43, 67, 0x65),
    key("F10", 0x44, 68, 0x6d),
    key("F11", 0x57, 87, 0x67),
    key("F12", 0x58, 88, 0x6f),
    key("F13", 0x64, 183, 0x69),
    key("F14", 0x65, 184, 0x6b),
    key("F15", 0x66, 185, 0x71),
    key("F16", 0x67, 186, 0x6a),
    key("F17", 0x68, 187, 0x40),
    key("F18", 0x69, 188, 0x4f),
    key("F19", 0x6a, 189, 0x50),
    key("F20", 0x6b, 190, 0x5a),
    pc_only("PrintScreen", 0xe037, 0x2c, 99),
    pc_only("ScrollLock", 0x46, 0, 70),
    pc_only("Pause", 0x45, 0x13, 119),
    key("Insert", 0xe052, 110, 0x72),
    key("Home", 0xe047, 102, 0x73),
    key("PageUp", 0xe049, 104, 0x74),
    key("Delete", 0xe053, 111, 0x75),
    key("End", 0xe04f, 107, 0x77),
    key("PageDown", 0xe051, 109, 0x79),
    key("ArrowRight", 0xe04d, 106, 0x7c),
    key("ArrowLeft", 0xe04b, 105, 0x7b),
    key("ArrowDown", 0xe050, 108, 0x7d),
    key("ArrowUp", 0xe048, 103, 0x7e),
    Key { code: "NumLock", scan: 0xe045, vk: 0x90, evdev: 69, mac: Some(0x47), modifier: None },
    key("NumpadDivide", 0xe035, 98, 0x4b),
    key("NumpadMultiply", 0x37, 55, 0x43),
    key("NumpadSubtract", 0x4a, 74, 0x4e),
    key("NumpadAdd", 0x4e, 78, 0x45),
    key("NumpadEnter", 0xe01c, 96, 0x4c),
    key("Numpad1", 0x4f, 79, 0x53),
    key("Numpad2", 0x50, 80, 0x54),
    key("Numpad3", 0x51, 81, 0x55),
    key("Numpad4", 0x4b, 75, 0x56),
    key("Numpad5", 0x4c, 76, 0x57),
    key("Numpad6", 0x4d, 77, 0x58),
    key("Numpad7", 0x47, 71, 0x59),
    key("Numpad8", 0x48, 72, 0x5b),
    key("Numpad9", 0x49, 73, 0x5c),
    key("Numpad0", 0x52, 82, 0x52),
    key("NumpadDecimal", 0x53, 83, 0x41),
    key("NumpadEqual", 0x59, 117, 0x51),
    key("IntlBackslash", 0x56, 86, 0x0a),
    key("IntlRo", 0x73, 89, 0x5e),
    key("IntlYen", 0x7d, 124, 0x5d),
    pc_only("ContextMenu", 0xe05d, 0, 127),
    modifier("ControlLeft", 0x1d, 29, 0x3b, Modifier::Control),
    modifier("ShiftLeft", 0x2a, 42, 0x38, Modifier::Shift),
    modifier("AltLeft", 0x38, 56, 0x3a, Modifier::Alt),
    modifier("MetaLeft", 0xe05b, 125, 0x37, Modifier::Meta),
    modifier("ControlRight", 0xe01d, 97, 0x3e, Modifier::Control),
    modifier("ShiftRight", 0x36, 54, 0x3c, Modifier::Shift),
    modifier("AltRight", 0xe038, 100, 0x3d, Modifier::Alt),
    modifier("MetaRight", 0xe05c, 126, 0x36, Modifier::Meta),
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn names_each_key_once_on_every_platform() {
        let distinct = |values: Vec<String>| values.len() == values.iter().collect::<HashSet<_>>().len();
        assert!(distinct(KEYS.iter().map(|key| key.code.to_owned()).collect()));
        assert!(distinct(KEYS.iter().map(|key| format!("{:x}/{:x}", key.scan, key.vk)).collect()));
        assert!(distinct(KEYS.iter().map(|key| key.evdev.to_string()).collect()));
        assert!(distinct(KEYS.iter().filter_map(|key| key.mac).map(|mac| mac.to_string()).collect()));
    }

    // Linux numbered the main block of keys after the PC's own scan codes, so for those keys the
    // two columns are written independently and must agree: a slip in either shows here.
    #[test]
    fn the_main_block_s_scan_codes_are_its_evdev_codes() {
        let main_block = KEYS.iter().filter(|key| key.scan < 0x59 && key.vk == 0);
        for key in main_block {
            assert_eq!(key.scan, key.evdev, "{}", key.code);
        }
    }

    #[test]
    fn knows_every_key_of_a_full_keyboard_and_nothing_that_acts_on_the_machine() {
        for code in ["KeyA", "Digit0", "ShiftRight", "NumpadEnter", "ArrowUp", "F12", "IntlBackslash"] {
            assert!(named(code).is_some(), "{code}");
        }
        for code in
            ["Power", "Sleep", "WakeUp", "Eject", "MediaPlayPause", "LaunchMail", "BrowserBack", "keya"]
        {
            assert!(named(code).is_none(), "{code}");
        }
    }
}
