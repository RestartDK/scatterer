use ratatui::style::Color;
use serde::Deserialize;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PickerPalette {
    pub accent: Color,
    pub panel_bg: Color,
    pub surface1: Color,
    pub surface_dim: Color,
    pub overlay0: Color,
    pub text: Color,
    pub subtext0: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub blue: Color,
    pub teal: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostAppearance {
    Dark,
    Light,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct HerdrConfig {
    theme: ThemeConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ThemeConfig {
    name: Option<String>,
    auto_switch: bool,
    dark_name: Option<String>,
    light_name: Option<String>,
    custom: Option<ThemeOverrides>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ThemeOverrides {
    accent: Option<String>,
    panel_bg: Option<String>,
    surface1: Option<String>,
    surface_dim: Option<String>,
    overlay0: Option<String>,
    text: Option<String>,
    subtext0: Option<String>,
    green: Option<String>,
    yellow: Option<String>,
    red: Option<String>,
    blue: Option<String>,
    teal: Option<String>,
}

impl PickerPalette {
    pub(super) fn load() -> Self {
        let Some(path) = herdr_config_path() else {
            return Self::named("catppuccin");
        };
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::named("catppuccin");
        };
        Self::from_config(&raw, host_appearance())
    }

    fn from_config(raw: &str, appearance: Option<HostAppearance>) -> Self {
        let Ok(config) = toml::from_str::<HerdrConfig>(raw) else {
            return Self::named("catppuccin");
        };
        let theme = &config.theme;
        let configured = theme.name.as_deref().unwrap_or("catppuccin");
        let name = if theme.auto_switch {
            match appearance {
                Some(HostAppearance::Dark) => theme.dark_name.as_deref().unwrap_or(configured),
                Some(HostAppearance::Light) => theme.light_name.as_deref().unwrap_or(configured),
                None => configured,
            }
        } else {
            configured
        };
        let mut palette = Self::named(name);
        if let Some(custom) = &theme.custom {
            palette.apply_overrides(custom);
        }
        palette
    }

    fn named(name: &str) -> Self {
        let normalized = name.trim().to_ascii_lowercase().replace([' ', '_'], "-");
        match normalized.as_str() {
            "catppuccin-latte" | "latte" | "light" => Self::from_rgb([
                0x1e66f5, 0xeff1f5, 0xbcc0cc, 0xe6e9ef, 0x9ca0b0, 0x4c4f69, 0x6c6f85, 0x40a02b,
                0xdf8e1d, 0xd20f39, 0x1e66f5, 0x179299,
            ]),
            "terminal" => Self {
                accent: Color::Blue,
                panel_bg: Color::Reset,
                surface1: Color::DarkGray,
                surface_dim: Color::DarkGray,
                overlay0: Color::Gray,
                text: Color::Reset,
                subtext0: Color::Gray,
                green: Color::Green,
                yellow: Color::Yellow,
                red: Color::LightRed,
                blue: Color::Blue,
                teal: Color::Cyan,
            },
            "tokyo-night" | "tokyonight" => Self::from_rgb([
                0x7aa2f7, 0x1a1b26, 0x414868, 0x1a1b26, 0x565f89, 0xc0caf5, 0xa9b1d6, 0x9ece6a,
                0xe0af68, 0xf7768e, 0x7aa2f7, 0x7dcfff,
            ]),
            "tokyo-night-day" | "tokyo-day" | "tokyonight-day" => Self::from_rgb([
                0x2e7de9, 0xe1e2e7, 0xa8aecb, 0xd2d3da, 0x8990b3, 0x3760bf, 0x6172b0, 0x587539,
                0x8c6c3e, 0xf52a65, 0x2e7de9, 0x118c74,
            ]),
            "dracula" => Self::from_rgb([
                0xbd93f9, 0x282a36, 0x6272a4, 0x282a36, 0x6272a4, 0xf8f8f2, 0xd2d2dc, 0x50fa7b,
                0xf1fa8c, 0xff5555, 0x8be9fd, 0x8be9fd,
            ]),
            "nord" => Self::from_rgb([
                0x88c0d0, 0x2e3440, 0x434c5e, 0x2e3440, 0x4c566a, 0xeceff4, 0xd8dee9, 0xa3be8c,
                0xebcb8b, 0xbf616a, 0x81a1c1, 0x8fbcbb,
            ]),
            "gruvbox" | "gruvbox-dark" => Self::from_rgb([
                0xd79921, 0x282828, 0x504945, 0x282828, 0x928374, 0xebdbb2, 0xd5c4a1, 0xb8bb26,
                0xfabd2f, 0xfb4934, 0x83a598, 0x8ec07c,
            ]),
            "gruvbox-light" => Self::from_rgb([
                0x076678, 0xfbf1c7, 0xd5c4a1, 0xf2e5bc, 0x928374, 0x3c3836, 0x504945, 0x79740e,
                0xb57614, 0x9d0006, 0x076678, 0x427b58,
            ]),
            "one-dark" | "onedark" => Self::from_rgb([
                0x61afef, 0x282c34, 0x3e4451, 0x282c34, 0x5c6370, 0xabb2bf, 0x969ca8, 0x98c379,
                0xe5c07b, 0xe06c75, 0x61afef, 0x56b6c2,
            ]),
            "one-light" | "onelight" => Self::from_rgb([
                0x4078f2, 0xfafafa, 0xe5e5e6, 0xf5f5f6, 0xa0a1a7, 0x383a42, 0x686b77, 0x50a14f,
                0xc18401, 0xe45649, 0x4078f2, 0x0184bc,
            ]),
            "solarized" | "solarized-dark" => Self::from_rgb([
                0x268bd2, 0x002b36, 0x586e75, 0x002b36, 0x586e75, 0x93a1a1, 0x839496, 0x859900,
                0xb58900, 0xdc322f, 0x268bd2, 0x2aa198,
            ]),
            "solarized-light" => Self::from_rgb([
                0x268bd2, 0xfdf6e3, 0x93a1a1, 0xeee8d5, 0x93a1a1, 0x657b83, 0x839496, 0x859900,
                0xb58900, 0xdc322f, 0x268bd2, 0x2aa198,
            ]),
            "kanagawa" => Self::from_rgb([
                0x7e9cd8, 0x1f1f28, 0x363646, 0x1f1f28, 0x727169, 0xdcd7ba, 0xc8c3aa, 0x76946a,
                0xc0a36e, 0xc34043, 0x7e9cd8, 0x7fb4ca,
            ]),
            "kanagawa-lotus" => Self::from_rgb([
                0x4d699b, 0xf2ecbc, 0xc9cbd1, 0xd5cea3, 0xa09cac, 0x545464, 0x43436c, 0x6f894e,
                0x77713f, 0xc84053, 0x4d699b, 0x4e8ca2,
            ]),
            "rose-pine" | "rosepine" => Self::from_rgb([
                0xc4a7e7, 0x191724, 0x26233a, 0x26233a, 0x6e6a86, 0xe0def4, 0xc8c5dc, 0x31748f,
                0xf6c177, 0xeb6f92, 0x31748f, 0x9ccfd8,
            ]),
            "rose-pine-dawn" | "rosepine-dawn" => Self::from_rgb([
                0x907aa9, 0xfaf4ed, 0xfffaf3, 0xf2e9e1, 0x9893a5, 0x464261, 0x797593, 0x286983,
                0xea9d34, 0xb4637a, 0x286983, 0x56949f,
            ]),
            "vesper" => Self::from_rgb([
                0xffc799, 0x1a1a1a, 0x282828, 0x101010, 0x5c5c5c, 0xffffff, 0xa0a0a0, 0x99ffe4,
                0xffc799, 0xff8080, 0xb0b0b0, 0x66ddcc,
            ]),
            _ => Self::from_rgb([
                0x89b4fa, 0x181825, 0x45475a, 0x1e1e2e, 0x6c7086, 0xcdd6f4, 0xa6adc8, 0xa6e3a1,
                0xf9e2af, 0xf38ba8, 0x89b4fa, 0x94e2d5,
            ]),
        }
    }

    fn from_rgb(values: [u32; 12]) -> Self {
        Self {
            accent: rgb(values[0]),
            panel_bg: rgb(values[1]),
            surface1: rgb(values[2]),
            surface_dim: rgb(values[3]),
            overlay0: rgb(values[4]),
            text: rgb(values[5]),
            subtext0: rgb(values[6]),
            green: rgb(values[7]),
            yellow: rgb(values[8]),
            red: rgb(values[9]),
            blue: rgb(values[10]),
            teal: rgb(values[11]),
        }
    }

    fn apply_overrides(&mut self, custom: &ThemeOverrides) {
        apply_color(&mut self.accent, custom.accent.as_deref());
        apply_color(&mut self.panel_bg, custom.panel_bg.as_deref());
        apply_color(&mut self.surface1, custom.surface1.as_deref());
        apply_color(&mut self.surface_dim, custom.surface_dim.as_deref());
        apply_color(&mut self.overlay0, custom.overlay0.as_deref());
        apply_color(&mut self.text, custom.text.as_deref());
        apply_color(&mut self.subtext0, custom.subtext0.as_deref());
        apply_color(&mut self.green, custom.green.as_deref());
        apply_color(&mut self.yellow, custom.yellow.as_deref());
        apply_color(&mut self.red, custom.red.as_deref());
        apply_color(&mut self.blue, custom.blue.as_deref());
        apply_color(&mut self.teal, custom.teal.as_deref());
    }

    pub(super) fn selected_fg(self) -> Color {
        if self.panel_bg == Color::Reset {
            self.surface_dim
        } else {
            self.panel_bg
        }
    }
}

fn herdr_config_path() -> Option<PathBuf> {
    std::env::var_os("HERDR_CONFIG_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".config/herdr/config.toml"))
        })
}

fn host_appearance() -> Option<HostAppearance> {
    if let Some(value) = std::env::var_os("COLORFGBG") {
        let background = value
            .to_string_lossy()
            .rsplit(';')
            .next()
            .and_then(|part| part.parse::<u8>().ok());
        if let Some(background) = background {
            return Some(if background < 8 {
                HostAppearance::Dark
            } else {
                HostAppearance::Light
            });
        }
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
            .ok()?;
        return Some(
            if output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .eq_ignore_ascii_case("dark")
            {
                HostAppearance::Dark
            } else {
                HostAppearance::Light
            },
        );
    }

    #[allow(unreachable_code)]
    None
}

fn rgb(value: u32) -> Color {
    Color::Rgb(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

fn apply_color(target: &mut Color, value: Option<&str>) {
    if let Some(color) = value.and_then(parse_color) {
        *target = color;
    }
}

fn parse_color(value: &str) -> Option<Color> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "reset" | "default" | "none" | "transparent" => Some(Color::Reset),
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" | "purple" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "lightred" => Some(Color::LightRed),
        "lightgreen" => Some(Color::LightGreen),
        "lightyellow" => Some(Color::LightYellow),
        "lightblue" => Some(Color::LightBlue),
        "lightmagenta" => Some(Color::LightMagenta),
        "lightcyan" => Some(Color::LightCyan),
        _ => parse_rgb(&value),
    }
}

fn parse_rgb(value: &str) -> Option<Color> {
    if let Some(hex) = value.strip_prefix('#') {
        let expanded;
        let hex = if hex.len() == 3 {
            expanded = hex
                .chars()
                .flat_map(|char| [char, char])
                .collect::<String>();
            expanded.as_str()
        } else {
            hex
        };
        if hex.len() == 6 {
            return u32::from_str_radix(hex, 16).ok().map(rgb);
        }
    }
    let parts = value
        .strip_prefix("rgb(")?
        .strip_suffix(')')?
        .split(',')
        .map(str::trim)
        .map(str::parse::<u8>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (parts.len() == 3).then(|| Color::Rgb(parts[0], parts[1], parts[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_switch_uses_the_configured_tokyo_night_variant() {
        let config = r#"
[theme]
name = "terminal"
auto_switch = true
dark_name = "tokyo-night"
light_name = "tokyo-night-day"
"#;
        assert_eq!(
            PickerPalette::from_config(config, Some(HostAppearance::Dark)).accent,
            Color::Rgb(122, 162, 247)
        );
        assert_eq!(
            PickerPalette::from_config(config, Some(HostAppearance::Light)).accent,
            Color::Rgb(46, 125, 233)
        );
    }

    #[test]
    fn custom_theme_colors_override_the_builtin_palette() {
        let config = r##"
[theme]
name = "tokyo-night"

[theme.custom]
accent = "#123456"
overlay0 = "rgb(10, 20, 30)"
"##;
        let palette = PickerPalette::from_config(config, Some(HostAppearance::Dark));
        assert_eq!(palette.accent, Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(palette.overlay0, Color::Rgb(10, 20, 30));
    }

    #[test]
    fn unknown_theme_falls_back_to_herdr_default() {
        assert_eq!(
            PickerPalette::named("not-a-theme").accent,
            Color::Rgb(137, 180, 250)
        );
    }
}
