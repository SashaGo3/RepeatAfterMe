//! The few things worth configuring. Saved as settings.json next to the progress.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnEnd {
    /// ends soon after you've said about as much as the teacher; waits through pauses before that
    #[default]
    Smart,
    /// the same, with longer waits (for slow or thoughtful speakers)
    Patient,
    /// only when you press Space
    Manual,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Playback {
    /// the teacher, then you: compare
    #[default]
    TeacherThenMe,
    /// just your take (quicker)
    MeOnly,
    /// nothing: go straight to deciding
    Nothing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// microphone by name, None = the system's
    pub input: Option<String>,
    /// speakers/headphones by name, None = the system's
    pub output: Option<String>,
    pub turn_end: TurnEnd,
    pub playback: Playback,
    pub theme: Theme,
    /// keep a log of every turn, with its audio, in the logs folder (for finding problems)
    pub diagnostics: bool,
    /// keep every take (on this computer), so you can export your answers for your teacher
    pub save_takes: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input: None,
            output: None,
            turn_end: TurnEnd::default(),
            playback: Playback::default(),
            theme: Theme::default(),
            diagnostics: false,
            save_takes: true,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        crate::progress::write_atomic(path, serde_json::to_string_pretty(self)?.as_bytes())
    }
    pub fn path(data: &Path) -> PathBuf {
        data.join("settings.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = Settings::path(dir.path());
        assert_eq!(Settings::load(&p), Settings::default());
        std::fs::write(&p, r#"{"turnEnd":"patient"}"#).unwrap();
        let s = Settings::load(&p);
        assert_eq!(s.turn_end, TurnEnd::Patient);
        assert_eq!(s.playback, Playback::TeacherThenMe);
        let s2 = Settings { input: Some("USB Mic".into()), ..s };
        s2.save(&p).unwrap();
        assert_eq!(Settings::load(&p), s2);
    }
}
