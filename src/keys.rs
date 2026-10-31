//! Key names, HID keyboard usages and the macro / shortcut mini-language.

use anyhow::{bail, Result};

pub const MACRO_HELP: &str = "\
A macro is a whitespace separated list of steps:

    ctrl+c          tap a chord (press in order, release in reverse)
    +shift / -shift hold / release a key
    50ms or 50      wait (default gap between steps is 15 ms, like the vendor tool)
    text:hello      tap each character (letters, digits, space and basic punctuation)
    mouse-left      buttons can be used like keys: mouse-left, mouse-right, mouse-middle";

pub const DEFAULT_GAP_MS: u64 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// HID keyboard usage (modifiers are 0xE0-0xE7)
    Kbd(u8),
    /// Bit in the mouse button mask
    Mouse(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Down(Key),
    Up(Key),
    Wait(u64),
}

const NAMED: &[(&str, u8)] = &[
    ("enter", 0x28), ("esc", 0x29), ("backspace", 0x2A), ("tab", 0x2B), ("space", 0x2C),
    ("minus", 0x2D), ("equal", 0x2E), ("leftbracket", 0x2F), ("rightbracket", 0x30),
    ("backslash", 0x31), ("semicolon", 0x33), ("apostrophe", 0x34), ("grave", 0x35),
    ("comma", 0x36), ("dot", 0x37), ("slash", 0x38), ("capslock", 0x39),
    ("printscreen", 0x46), ("scrolllock", 0x47), ("pause", 0x48), ("insert", 0x49),
    ("home", 0x4A), ("pageup", 0x4B), ("delete", 0x4C), ("end", 0x4D), ("pagedown", 0x4E),
    ("right", 0x4F), ("left", 0x50), ("down", 0x51), ("up", 0x52), ("menu", 0x65),
    ("ctrl", 0xE0), ("shift", 0xE1), ("alt", 0xE2), ("super", 0xE3),
    ("rctrl", 0xE4), ("rshift", 0xE5), ("altgr", 0xE6), ("rsuper", 0xE7),
];
const ALIASES: &[(&str, &str)] = &[
    ("control", "ctrl"), ("win", "super"), ("meta", "super"), ("return", "enter"),
    ("escape", "esc"), ("del", "delete"), ("ralt", "altgr"), ("-", "minus"), ("=", "equal"),
];
const MOUSE: &[(&str, u8)] = &[("mouse-left", 0x1), ("mouse-right", 0x2), ("mouse-middle", 0x4)];

const TEXT_CHARS: &[(char, &str)] = &[
    (' ', "space"), ('-', "minus"), ('=', "equal"), (',', "comma"), ('.', "dot"),
    ('/', "slash"), (';', "semicolon"), ('\'', "apostrophe"), ('[', "leftbracket"),
    (']', "rightbracket"), ('\\', "backslash"), ('`', "grave"),
];
const SHIFTED: &[(char, &str)] = &[
    ('!', "1"), ('@', "2"), ('#', "3"), ('$', "4"), ('%', "5"), ('^', "6"), ('&', "7"),
    ('*', "8"), ('(', "9"), (')', "0"), ('_', "minus"), ('+', "equal"), ('?', "slash"),
    (':', "semicolon"), ('"', "apostrophe"), ('<', "comma"), ('>', "dot"),
];

fn lookup<T: Copy>(table: &[(&str, T)], name: &str) -> Option<T> {
    table.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

pub fn key(name: &str) -> Result<Key> {
    let lower = name.to_lowercase();
    let name = lookup(ALIASES, &lower).unwrap_or(&lower);
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() {
            return Ok(Key::Kbd(0x04 + (c as u8 - b'a')));
        }
        if c == '0' {
            return Ok(Key::Kbd(0x27));
        }
        if c.is_ascii_digit() {
            return Ok(Key::Kbd(0x1E + (c as u8 - b'1')));
        }
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=12).contains(&n) {
            return Ok(Key::Kbd(0x3A + n - 1));
        }
    }
    if let Some(u) = lookup(NAMED, name) {
        return Ok(Key::Kbd(u));
    }
    if let Some(bit) = lookup(MOUSE, name) {
        return Ok(Key::Mouse(bit));
    }
    bail!("unknown key {name:?}")
}

pub fn parse_chord(text: &str) -> Result<Vec<Key>> {
    text.split('+').map(key).collect()
}

fn tap(chord: &[Key], steps: &mut Vec<Step>) {
    steps.extend(chord.iter().map(|&k| Step::Down(k)));
    steps.extend(chord.iter().rev().map(|&k| Step::Up(k)));
}

pub fn parse_macro(text: &str) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    for tok in text.split_whitespace() {
        let num = tok.strip_suffix("ms").unwrap_or(tok);
        if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) {
            steps.push(Step::Wait(num.parse()?));
        } else if let Some(t) = tok.strip_prefix("text:") {
            for ch in t.chars() {
                let shifted_base = SHIFTED.iter().find(|(c, _)| *c == ch).map(|(_, n)| *n);
                let base = match shifted_base.or_else(|| TEXT_CHARS.iter().find(|(c, _)| *c == ch).map(|(_, n)| *n)) {
                    Some(n) => n.to_string(),
                    None => ch.to_lowercase().to_string(),
                };
                let k = key(&base)?;
                if ch.is_uppercase() || shifted_base.is_some() {
                    tap(&[Key::Kbd(0xE1), k], &mut steps);
                } else {
                    tap(&[k], &mut steps);
                }
            }
        } else if tok.len() > 1 && (tok.starts_with('+') || tok.starts_with('-')) {
            let k = key(&tok[1..])?;
            steps.push(if tok.starts_with('+') { Step::Down(k) } else { Step::Up(k) });
        } else {
            tap(&parse_chord(tok)?, &mut steps);
        }
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        let c = Key::Kbd(0x06);
        let ctrl = Key::Kbd(0xE0);
        let x = Key::Kbd(0x1B);
        assert_eq!(
            parse_macro("ctrl+c 100ms -x").unwrap(),
            vec![Step::Down(ctrl), Step::Down(c), Step::Up(c), Step::Up(ctrl), Step::Wait(100), Step::Up(x)]
        );
        let shift = Key::Kbd(0xE1);
        let a = Key::Kbd(0x04);
        assert_eq!(parse_macro("text:A!").unwrap()[..4], [Step::Down(shift), Step::Down(a), Step::Up(a), Step::Up(shift)]);
        assert!(parse_macro("nosuchkey").is_err());
        assert_eq!(key("d").unwrap(), Key::Kbd(0x07));
        assert_eq!(key("0").unwrap(), Key::Kbd(0x27));
        assert_eq!(key("F12").unwrap(), Key::Kbd(0x45));
        assert_eq!(key("mouse-right").unwrap(), Key::Mouse(0x2));
    }
}
