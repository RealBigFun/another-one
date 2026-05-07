use alacritty_terminal::vte::ansi::{NamedColor, Rgb};

use crate::theme::ResolvedTheme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalThemeId {
    GitHubLight,
    GitHubDark,
}

impl TerminalThemeId {
    pub(crate) fn for_resolved_theme(theme: ResolvedTheme) -> Self {
        match theme {
            ResolvedTheme::Light => Self::GitHubLight,
            ResolvedTheme::Dark => Self::GitHubDark,
        }
    }
}

#[derive(Clone, Copy)]
struct TerminalThemeColors {
    foreground: Rgb,
    background: Rgb,
    normal: [Rgb; 8],
    bright: [Rgb; 8],
}

pub(crate) fn default_named_color(theme: ResolvedTheme, named: NamedColor) -> Rgb {
    let palette = colors(TerminalThemeId::for_resolved_theme(theme));
    match named {
        NamedColor::Black => palette.normal[0],
        NamedColor::Red => palette.normal[1],
        NamedColor::Green => palette.normal[2],
        NamedColor::Yellow => palette.normal[3],
        NamedColor::Blue => palette.normal[4],
        NamedColor::Magenta => palette.normal[5],
        NamedColor::Cyan => palette.normal[6],
        NamedColor::White => palette.normal[7],
        NamedColor::BrightBlack => palette.bright[0],
        NamedColor::BrightRed => palette.bright[1],
        NamedColor::BrightGreen => palette.bright[2],
        NamedColor::BrightYellow => palette.bright[3],
        NamedColor::BrightBlue => palette.bright[4],
        NamedColor::BrightMagenta => palette.bright[5],
        NamedColor::BrightCyan => palette.bright[6],
        NamedColor::BrightWhite => palette.bright[7],
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => {
            palette.foreground
        }
        NamedColor::Background => palette.background,
        NamedColor::DimBlack => scale_rgb(palette.normal[0], 0.72),
        NamedColor::DimRed => scale_rgb(palette.normal[1], 0.72),
        NamedColor::DimGreen => scale_rgb(palette.normal[2], 0.72),
        NamedColor::DimYellow => scale_rgb(palette.normal[3], 0.72),
        NamedColor::DimBlue => scale_rgb(palette.normal[4], 0.72),
        NamedColor::DimMagenta => scale_rgb(palette.normal[5], 0.72),
        NamedColor::DimCyan => scale_rgb(palette.normal[6], 0.72),
        NamedColor::DimWhite => scale_rgb(palette.normal[7], 0.72),
        NamedColor::DimForeground => scale_rgb(palette.foreground, 0.72),
    }
}

pub(crate) fn default_indexed_color(theme: ResolvedTheme, index: u8) -> Option<Rgb> {
    let palette = colors(TerminalThemeId::for_resolved_theme(theme));
    match index {
        0..=7 => Some(palette.normal[index as usize]),
        8..=15 => Some(palette.bright[(index - 8) as usize]),
        _ => None,
    }
}

fn colors(id: TerminalThemeId) -> TerminalThemeColors {
    match id {
        // From https://github.com/alacritty/alacritty-theme/blob/master/themes/github_light.toml
        TerminalThemeId::GitHubLight => TerminalThemeColors {
            background: rgb(0xffffff),
            foreground: rgb(0x24292f),
            normal: [
                rgb(0x24292e), rgb(0xd73a49), rgb(0x28a745), rgb(0xdbab09),
                rgb(0x0366d6), rgb(0x5a32a3), rgb(0x0598bc), rgb(0x6a737d),
            ],
            bright: [
                rgb(0x959da5), rgb(0xcb2431), rgb(0x22863a), rgb(0xb08800),
                rgb(0x005cc5), rgb(0x5a32a3), rgb(0x3192aa), rgb(0xd1d5da),
            ],
        },
        // From https://github.com/alacritty/alacritty-theme/blob/master/themes/github_dark.toml
        TerminalThemeId::GitHubDark => TerminalThemeColors {
            background: rgb(0x24292e),
            foreground: rgb(0xd1d5da),
            normal: [
                rgb(0x586069), rgb(0xea4a5a), rgb(0x34d058), rgb(0xffea7f),
                rgb(0x2188ff), rgb(0xb392f0), rgb(0x39c5cf), rgb(0xd1d5da),
            ],
            bright: [
                rgb(0x959da5), rgb(0xf97583), rgb(0x85e89d), rgb(0xffea7f),
                rgb(0x79b8ff), rgb(0xb392f0), rgb(0x56d4dd), rgb(0xfafbfc),
            ],
        },
    }
}

fn scale_rgb(rgb: Rgb, factor: f32) -> Rgb {
    Rgb {
        r: (f32::from(rgb.r) * factor).round().clamp(0.0, 255.0) as u8,
        g: (f32::from(rgb.g) * factor).round().clamp(0.0, 255.0) as u8,
        b: (f32::from(rgb.b) * factor).round().clamp(0.0, 255.0) as u8,
    }
}

const fn rgb(hex: u32) -> Rgb {
    Rgb { r: ((hex >> 16) & 0xff) as u8, g: ((hex >> 8) & 0xff) as u8, b: (hex & 0xff) as u8 }
}
