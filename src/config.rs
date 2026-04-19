use serde::{Deserialize, Serialize};
use shikumi::{ConfigDiscovery, ConfigStore, Format};
use std::path::PathBuf;

use crate::errors::{Result, SuzuriError};

/// Terminal configuration, hot-reloadable via shikumi.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub font: FontConfig,
    pub window: WindowConfig,
    pub terminal: TerminalConfig,
    pub colors: ColorScheme,
    pub shell: ShellConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub line_height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    pub padding: u32,
    pub opacity: f32,
    pub title: String,
    pub decorations: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub scrollback_lines: usize,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    pub program: String,
    pub args: Vec<String>,
}

/// Nord-inspired color scheme.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorScheme {
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub cursor: [u8; 3],
    pub selection: [u8; 3],

    // ANSI 16 colors
    pub black: [u8; 3],
    pub red: [u8; 3],
    pub green: [u8; 3],
    pub yellow: [u8; 3],
    pub blue: [u8; 3],
    pub magenta: [u8; 3],
    pub cyan: [u8; 3],
    pub white: [u8; 3],
    pub bright_black: [u8; 3],
    pub bright_red: [u8; 3],
    pub bright_green: [u8; 3],
    pub bright_yellow: [u8; 3],
    pub bright_blue: [u8; 3],
    pub bright_magenta: [u8; 3],
    pub bright_cyan: [u8; 3],
    pub bright_white: [u8; 3],
}

// ── Defaults ──────────────────────────────────────────────────

impl Default for Config {
    fn default() -> Self {
        Self {
            font: FontConfig::default(),
            window: WindowConfig::default(),
            terminal: TerminalConfig::default(),
            colors: ColorScheme::default(),
            shell: ShellConfig::default(),
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "JetBrains Mono".into(),
            size: 14.0,
            line_height: 1.2,
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 768,
            padding: 8,
            opacity: 1.0,
            title: "Suzuri".into(),
            decorations: true,
        }
    }
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            scrollback_lines: 10_000,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
        }
    }
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            program: std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()),
            args: vec!["--login".into()],
        }
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        // Nord palette
        Self {
            foreground: [0xD8, 0xDE, 0xE9], // nord4
            background: [0x2E, 0x34, 0x40], // nord0
            cursor: [0xD8, 0xDE, 0xE9],     // nord4
            selection: [0x43, 0x4C, 0x5E],   // nord3

            black: [0x3B, 0x42, 0x52],         // nord1
            red: [0xBF, 0x61, 0x6A],           // nord11
            green: [0xA3, 0xBE, 0x8C],         // nord14
            yellow: [0xEB, 0xCB, 0x8B],        // nord13
            blue: [0x81, 0xA1, 0xC1],          // nord9
            magenta: [0xB4, 0x8E, 0xAD],       // nord15
            cyan: [0x88, 0xC0, 0xD0],          // nord8
            white: [0xE5, 0xE9, 0xF0],         // nord5
            bright_black: [0x4C, 0x56, 0x6A],  // nord3
            bright_red: [0xBF, 0x61, 0x6A],    // nord11
            bright_green: [0xA3, 0xBE, 0x8C],  // nord14
            bright_yellow: [0xEB, 0xCB, 0x8B], // nord13
            bright_blue: [0x81, 0xA1, 0xC1],   // nord9
            bright_magenta: [0xB4, 0x8E, 0xAD], // nord15
            bright_cyan: [0x8F, 0xBC, 0xBB],   // nord7
            bright_white: [0xEC, 0xEF, 0xF4],  // nord6
        }
    }
}

impl ColorScheme {
    /// Look up an ANSI color index (0–15) to an RGB triple.
    pub fn ansi_color(&self, index: u8) -> [u8; 3] {
        match index {
            0 => self.black,
            1 => self.red,
            2 => self.green,
            3 => self.yellow,
            4 => self.blue,
            5 => self.magenta,
            6 => self.cyan,
            7 => self.white,
            8 => self.bright_black,
            9 => self.bright_red,
            10 => self.bright_green,
            11 => self.bright_yellow,
            12 => self.bright_blue,
            13 => self.bright_magenta,
            14 => self.bright_cyan,
            15 => self.bright_white,
            // 256-color cube and grayscale ramp
            16..=231 => {
                let idx = index - 16;
                let r = (idx / 36) * 51;
                let g = ((idx % 36) / 6) * 51;
                let b = (idx % 6) * 51;
                [r, g, b]
            }
            232..=255 => {
                let gray = 8 + (index - 232) * 10;
                [gray, gray, gray]
            }
        }
    }
}

// ── Config Loading via Shikumi ────────────────────────────────

/// Load configuration with hot-reload support using shikumi.
pub fn load_config() -> Result<ConfigStore<Config>> {
    let path = ConfigDiscovery::new("suzuri")
        .env_override("SUZURI_CONFIG")
        .formats(&[Format::Toml, Format::Yaml])
        .discover();

    match path {
        Ok(path) => {
            tracing::info!("Loading config from {}", path.display());
            ConfigStore::load_and_watch(&path, "SUZURI", |config: &Config| {
                tracing::info!("Config reloaded: font={} size={}", config.font.family, config.font.size);
            })
            .map_err(SuzuriError::from)
        }
        Err(_) => {
            tracing::info!("No config file found, using defaults");
            // Write default config for discovery next time
            let config_dir = dirs_config_path();
            if let Some(dir) = config_dir.parent() {
                std::fs::create_dir_all(dir).ok();
            }
            let defaults = Config::default();
            let toml_str = toml_string(&defaults);
            std::fs::write(&config_dir, &toml_str).ok();

            ConfigStore::load(&config_dir, "SUZURI").map_err(SuzuriError::from)
        }
    }
}

fn dirs_config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        });
    base.join("suzuri").join("suzuri.toml")
}

fn toml_string(config: &Config) -> String {
    // Minimal hand-crafted TOML for defaults
    format!(
        r#"# Suzuri terminal configuration

[font]
family = "{}"
size = {}
line_height = {}

[window]
width = {}
height = {}
padding = {}
opacity = {}
title = "{}"
decorations = {}

[terminal]
scrollback_lines = {}
cursor_style = "{}"
cursor_blink = {}

[shell]
program = "{}"
args = ["--login"]
"#,
        config.font.family,
        config.font.size,
        config.font.line_height,
        config.window.width,
        config.window.height,
        config.window.padding,
        config.window.opacity,
        config.window.title,
        config.window.decorations,
        config.terminal.scrollback_lines,
        match config.terminal.cursor_style {
            CursorStyle::Block => "block",
            CursorStyle::Underline => "underline",
            CursorStyle::Bar => "bar",
        },
        config.terminal.cursor_blink,
        config.shell.program,
    )
}
