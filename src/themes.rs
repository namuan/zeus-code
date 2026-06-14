//! Color theme definitions.
//!
//! Each theme provides a complete color palette for the TUI.
//! Themes can be switched at runtime with `/themes`.

use ratatui::style::Color;

/// A complete color theme.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: &'static str,
    pub bg: Color,
    pub fg: Color,
    pub dim: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub surface: Color,
    pub border: Color,
}

impl Theme {
    /// Get the names of all available themes.
    pub fn all_names() -> Vec<&'static str> {
        ALL_THEMES.iter().map(|t| t.name).collect()
    }

    /// Find a theme by name. Returns default (gruvbox-dark) if not found.
    pub fn by_name(name: &str) -> &'static Theme {
        ALL_THEMES
            .iter()
            .find(|t| t.name == name)
            .unwrap_or(&GRUVBOX_DARK)
    }
}

// ── 16 built-in themes ───────────────────────────────────────────────────

const GRUVBOX_DARK: Theme = Theme {
    name: "gruvbox-dark",
    bg: Color::Rgb(40, 40, 40),
    fg: Color::Rgb(235, 219, 178),
    dim: Color::Rgb(102, 92, 84),
    accent: Color::Rgb(131, 165, 152),
    success: Color::Rgb(184, 187, 38),
    warning: Color::Rgb(250, 189, 47),
    error: Color::Rgb(251, 73, 52),
    surface: Color::Rgb(60, 56, 54),
    border: Color::Rgb(80, 73, 69),
};

const DRACULA: Theme = Theme {
    name: "dracula",
    bg: Color::Rgb(40, 42, 54),
    fg: Color::Rgb(248, 248, 242),
    dim: Color::Rgb(98, 114, 164),
    accent: Color::Rgb(189, 147, 249),
    success: Color::Rgb(80, 250, 123),
    warning: Color::Rgb(255, 184, 108),
    error: Color::Rgb(255, 85, 85),
    surface: Color::Rgb(68, 71, 90),
    border: Color::Rgb(98, 114, 164),
};

const NORD: Theme = Theme {
    name: "nord",
    bg: Color::Rgb(46, 52, 64),
    fg: Color::Rgb(216, 222, 233),
    dim: Color::Rgb(76, 86, 106),
    accent: Color::Rgb(136, 192, 208),
    success: Color::Rgb(163, 190, 140),
    warning: Color::Rgb(235, 203, 139),
    error: Color::Rgb(191, 97, 106),
    surface: Color::Rgb(59, 66, 82),
    border: Color::Rgb(76, 86, 106),
};

const TOKYO_NIGHT: Theme = Theme {
    name: "tokyo-night",
    bg: Color::Rgb(26, 27, 38),
    fg: Color::Rgb(169, 177, 214),
    dim: Color::Rgb(86, 95, 137),
    accent: Color::Rgb(122, 162, 247),
    success: Color::Rgb(158, 206, 106),
    warning: Color::Rgb(224, 175, 104),
    error: Color::Rgb(247, 118, 142),
    surface: Color::Rgb(41, 46, 66),
    border: Color::Rgb(86, 95, 137),
};

const CATPPUCCIN: Theme = Theme {
    name: "catppuccin",
    bg: Color::Rgb(30, 30, 46),
    fg: Color::Rgb(205, 214, 244),
    dim: Color::Rgb(88, 91, 112),
    accent: Color::Rgb(137, 180, 250),
    success: Color::Rgb(166, 227, 161),
    warning: Color::Rgb(249, 226, 175),
    error: Color::Rgb(243, 139, 168),
    surface: Color::Rgb(49, 50, 68),
    border: Color::Rgb(88, 91, 112),
};

const SOLARIZED_DARK: Theme = Theme {
    name: "solarized-dark",
    bg: Color::Rgb(0, 43, 54),
    fg: Color::Rgb(131, 148, 150),
    dim: Color::Rgb(88, 110, 117),
    accent: Color::Rgb(38, 139, 210),
    success: Color::Rgb(133, 153, 0),
    warning: Color::Rgb(181, 137, 0),
    error: Color::Rgb(220, 50, 47),
    surface: Color::Rgb(7, 54, 66),
    border: Color::Rgb(88, 110, 117),
};

const MONOKAI: Theme = Theme {
    name: "monokai",
    bg: Color::Rgb(39, 40, 34),
    fg: Color::Rgb(248, 248, 242),
    dim: Color::Rgb(117, 113, 94),
    accent: Color::Rgb(166, 226, 46),
    success: Color::Rgb(166, 226, 46),
    warning: Color::Rgb(230, 219, 116),
    error: Color::Rgb(249, 38, 114),
    surface: Color::Rgb(56, 58, 50),
    border: Color::Rgb(117, 113, 94),
};

const ONE_DARK: Theme = Theme {
    name: "one-dark",
    bg: Color::Rgb(40, 44, 52),
    fg: Color::Rgb(171, 178, 191),
    dim: Color::Rgb(92, 99, 112),
    accent: Color::Rgb(97, 175, 239),
    success: Color::Rgb(152, 195, 121),
    warning: Color::Rgb(229, 192, 123),
    error: Color::Rgb(224, 108, 117),
    surface: Color::Rgb(49, 54, 63),
    border: Color::Rgb(92, 99, 112),
};

const GITHUB_DARK: Theme = Theme {
    name: "github-dark",
    bg: Color::Rgb(13, 17, 23),
    fg: Color::Rgb(230, 237, 243),
    dim: Color::Rgb(72, 79, 88),
    accent: Color::Rgb(88, 166, 255),
    success: Color::Rgb(63, 185, 80),
    warning: Color::Rgb(210, 153, 34),
    error: Color::Rgb(248, 81, 73),
    surface: Color::Rgb(22, 27, 34),
    border: Color::Rgb(48, 54, 61),
};

const EVERFOREST: Theme = Theme {
    name: "everforest",
    bg: Color::Rgb(45, 53, 59),
    fg: Color::Rgb(211, 198, 170),
    dim: Color::Rgb(74, 85, 93),
    accent: Color::Rgb(127, 187, 179),
    success: Color::Rgb(167, 192, 128),
    warning: Color::Rgb(219, 188, 127),
    error: Color::Rgb(230, 126, 128),
    surface: Color::Rgb(55, 65, 72),
    border: Color::Rgb(74, 85, 93),
};

const ROSEPINE: Theme = Theme {
    name: "rosepine",
    bg: Color::Rgb(25, 23, 36),
    fg: Color::Rgb(224, 222, 244),
    dim: Color::Rgb(111, 110, 133),
    accent: Color::Rgb(196, 167, 231),
    success: Color::Rgb(156, 207, 216),
    warning: Color::Rgb(246, 193, 119),
    error: Color::Rgb(235, 111, 146),
    surface: Color::Rgb(38, 35, 53),
    border: Color::Rgb(111, 110, 133),
};

const FLEXOKI: Theme = Theme {
    name: "flexoki",
    bg: Color::Rgb(16, 15, 14),
    fg: Color::Rgb(206, 205, 195),
    dim: Color::Rgb(95, 94, 87),
    accent: Color::Rgb(138, 165, 141),
    success: Color::Rgb(140, 138, 96),
    warning: Color::Rgb(200, 140, 63),
    error: Color::Rgb(193, 76, 52),
    surface: Color::Rgb(28, 27, 26),
    border: Color::Rgb(79, 78, 72),
};

const AYU: Theme = Theme {
    name: "ayu",
    bg: Color::Rgb(15, 20, 25),
    fg: Color::Rgb(191, 180, 171),
    dim: Color::Rgb(60, 67, 82),
    accent: Color::Rgb(57, 186, 230),
    success: Color::Rgb(134, 201, 67),
    warning: Color::Rgb(255, 173, 102),
    error: Color::Rgb(255, 69, 61),
    surface: Color::Rgb(23, 29, 35),
    border: Color::Rgb(60, 67, 82),
};

const KANAGAWA: Theme = Theme {
    name: "kanagawa",
    bg: Color::Rgb(31, 31, 40),
    fg: Color::Rgb(220, 215, 186),
    dim: Color::Rgb(84, 83, 97),
    accent: Color::Rgb(126, 156, 216),
    success: Color::Rgb(152, 187, 115),
    warning: Color::Rgb(230, 180, 64),
    error: Color::Rgb(228, 104, 118),
    surface: Color::Rgb(44, 44, 56),
    border: Color::Rgb(84, 83, 97),
};

const PALENIGHT: Theme = Theme {
    name: "palenight",
    bg: Color::Rgb(41, 45, 62),
    fg: Color::Rgb(167, 172, 204),
    dim: Color::Rgb(86, 81, 106),
    accent: Color::Rgb(130, 170, 255),
    success: Color::Rgb(195, 232, 141),
    warning: Color::Rgb(255, 203, 107),
    error: Color::Rgb(255, 83, 112),
    surface: Color::Rgb(57, 58, 81),
    border: Color::Rgb(86, 81, 106),
};

const ALL_THEMES: &[Theme] = &[
    GRUVBOX_DARK,
    DRACULA,
    NORD,
    TOKYO_NIGHT,
    CATPPUCCIN,
    SOLARIZED_DARK,
    MONOKAI,
    ONE_DARK,
    GITHUB_DARK,
    EVERFOREST,
    ROSEPINE,
    FLEXOKI,
    AYU,
    KANAGAWA,
    PALENIGHT,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_themes_have_names() {
        for theme in ALL_THEMES {
            assert!(!theme.name.is_empty());
        }
    }

    #[test]
    fn test_theme_by_name_found() {
        let theme = Theme::by_name("dracula");
        assert_eq!(theme.name, "dracula");
    }

    #[test]
    fn test_theme_by_name_default() {
        let theme = Theme::by_name("nonexistent");
        assert_eq!(theme.name, "gruvbox-dark");
    }

    #[test]
    fn test_all_names_count() {
        let names = Theme::all_names();
        assert!(names.len() >= 15);
    }
}
