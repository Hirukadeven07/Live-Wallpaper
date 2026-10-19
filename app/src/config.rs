// What the user picked: which wallpaper is on which screen, each wallpaper's settings, and the app's own options.
// Saved as config.json in the app's data folder.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{log, util};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// The wallpaper for any screen that has no choice of its own yet (a new monitor, say). None: leave it alone.
    pub default_wallpaper: Option<String>,
    /// Screen id -> wallpaper id. A null wallpaper means "keep the Windows wallpaper on this screen".
    pub displays: BTreeMap<String, Option<String>>,
    /// Wallpaper id -> the settings the user changed from their defaults.
    pub properties: BTreeMap<String, Map<String, Value>>,
    pub paused: bool,
    pub pause_when_covered: bool,
    pub pause_on_battery: bool,
    pub mouse_input: bool,
    pub developer: bool,
    pub autostart: bool,
    pub welcomed: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_wallpaper: None,
            displays: BTreeMap::new(),
            properties: BTreeMap::new(),
            paused: false,
            pause_when_covered: true,
            pause_on_battery: false,
            mouse_input: true,
            developer: false,
            autostart: true,
            welcomed: false,
        }
    }
}

impl Config {
    fn path() -> std::path::PathBuf {
        util::data_dir().join("config.json")
    }

    pub fn load() -> Config {
        match std::fs::read(Self::path()) {
            Ok(bytes) => match util::parse_json(&bytes).map(serde_json::from_value::<Config>) {
                Some(Ok(c)) => c,
                _ => {
                    log!("config.json couldn't be read, so starting fresh (the old one is kept as config.bad.json)");
                    let _ = std::fs::copy(Self::path(), util::data_dir().join("config.bad.json"));
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) {
        match serde_json::to_vec_pretty(self) {
            Ok(bytes) => {
                if let Err(e) = util::write_atomic(&Self::path(), &bytes) {
                    log!("couldn't save config.json: {e}");
                }
            }
            Err(e) => log!("couldn't write the settings: {e}"),
        }
    }

    /// The wallpaper a screen should show.
    pub fn wallpaper_for(&self, display: &str) -> Option<String> {
        match self.displays.get(display) {
            Some(choice) => choice.clone(),
            None => self.default_wallpaper.clone(),
        }
    }
}
