//! The host's display as its Direct3D 9 sees it (brief L34).
//!
//! The game builds its resolution list from IDirect3D9::GetAdapterModeCount
//! and EnumAdapterModes (adapter 0, D3DFMT_X8R8G8B8) and its desktop size
//! from GetAdapterDisplayMode (0x00DB1480 and the mode helpers it calls).
//! The runtime answers those calls from [`query`]: the host's own Direct3D 9
//! asked once, so the list is the player's monitor's, in the driver's
//! order. Measured on the maintainer's host (research/runtime/windows/
//! display_modes.py): the list is user32's EnumDisplaySettingsW 32-bit
//! modes, each (width, height, refresh) once in first-seen order, and the
//! desktop is ENUM_CURRENT_SETTINGS; asking Direct3D 9 itself needs no such
//! rule.

/// D3DFMT_X8R8G8B8, the only adapter format the game asks for.
pub const X8R8G8B8: u32 = 22;

/// A D3DDISPLAYMODE: width, height, refresh rate (Hz), format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub refresh: u32,
    pub format: u32,
}

/// Adapter 0's desktop mode and its X8R8G8B8 modes in EnumAdapterModes order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Display {
    pub desktop: Mode,
    pub modes: Vec<Mode>,
}

impl Display {
    /// Whether a full-screen back buffer of this size and refresh rate (0:
    /// the adapter's default) names one of the modes.
    pub fn has(&self, width: u32, height: u32, refresh: u32) -> bool {
        self.modes.iter().any(|m| m.width == width && m.height == height && (refresh == 0 || m.refresh == refresh))
    }

    /// Parse the text form [`Display::to_text`] writes: a `desktop w h hz`
    /// line, then one `mode w h hz` line per mode (X8R8G8B8).
    pub fn parse(text: &str) -> Option<Display> {
        let mut desktop = None;
        let mut modes = Vec::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let f: Vec<&str> = line.split_whitespace().collect();
            let n = |k: usize| f.get(k).and_then(|v| v.parse::<u32>().ok());
            let mode = Mode { width: n(1)?, height: n(2)?, refresh: n(3)?, format: X8R8G8B8 };
            match f[0] {
                "desktop" => desktop = Some(mode),
                "mode" => modes.push(mode),
                _ => return None,
            }
        }
        Some(Display { desktop: desktop?, modes })
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("desktop {} {} {}\n", self.desktop.width, self.desktop.height, self.desktop.refresh);
        for m in &self.modes {
            s += &format!("mode {} {} {}\n", m.width, m.height, m.refresh);
        }
        s
    }
}

/// Ask the host's Direct3D 9 (adapter 0). None when there is no Direct3D 9
/// or no adapter (or not on Windows).
#[cfg(windows)]
pub fn query() -> Option<Display> {
    use windows::Win32::Graphics::Direct3D9::{Direct3DCreate9, D3DDISPLAYMODE, D3DFMT_X8R8G8B8, D3D_SDK_VERSION};
    // SAFETY: plain COM calls on an object this function owns; the mode
    // structures are local.
    unsafe {
        let d3d = Direct3DCreate9(D3D_SDK_VERSION)?;
        let mode = |m: D3DDISPLAYMODE| Mode { width: m.Width, height: m.Height, refresh: m.RefreshRate, format: m.Format.0 };
        let mut current = D3DDISPLAYMODE::default();
        d3d.GetAdapterDisplayMode(0, &mut current).ok()?;
        let count = d3d.GetAdapterModeCount(0, D3DFMT_X8R8G8B8);
        let mut modes = Vec::with_capacity(count as usize);
        for k in 0..count {
            let mut m = D3DDISPLAYMODE::default();
            d3d.EnumAdapterModes(0, D3DFMT_X8R8G8B8, k, &mut m).ok()?;
            modes.push(mode(m));
        }
        Some(Display { desktop: mode(current), modes })
    }
}

#[cfg(not(windows))]
pub fn query() -> Option<Display> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_form_round_trips() {
        let d = Display {
            desktop: Mode { width: 2560, height: 1440, refresh: 240, format: X8R8G8B8 },
            modes: vec![Mode { width: 640, height: 480, refresh: 59, format: X8R8G8B8 }, Mode { width: 2560, height: 1440, refresh: 240, format: X8R8G8B8 }],
        };
        assert_eq!(Display::parse(&d.to_text()), Some(d.clone()));
        assert!(d.has(640, 480, 0) && d.has(640, 480, 59) && !d.has(640, 480, 60) && !d.has(800, 600, 0));
    }

    #[cfg(windows)]
    #[test]
    fn the_host_answers_with_its_desktop_among_its_modes() {
        let Some(d) = query() else { eprintln!("skipped: no Direct3D 9 adapter"); return };
        assert!(!d.modes.is_empty());
        assert!(d.modes.iter().all(|m| m.format == X8R8G8B8));
        assert!(d.has(d.desktop.width, d.desktop.height, d.desktop.refresh), "the desktop is one of the modes: {:?}", d.desktop);
        let mut seen = std::collections::HashSet::new();
        assert!(d.modes.iter().all(|m| seen.insert(*m)), "each mode once");
    }
}
