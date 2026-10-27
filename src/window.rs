//! Структуры данных окна. Сериализуются в JSON для отправки клиентам.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BinaryKind {
    Native,
    Wine,
    Flatpak,
    Unknown,
}

impl BinaryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BinaryKind::Native => "native",
            BinaryKind::Wine => "wine",
            BinaryKind::Flatpak => "flatpak",
            BinaryKind::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub instance: String,
    #[serde(default)]
    pub exe_path: String,
    #[serde(default = "default_kind")]
    pub kind: BinaryKind,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub icon_path: Option<String>,
}

impl WindowInfo {
    /// Стабильный ключ для группировки (наш «AppId»).
    pub fn app_key(&self) -> String {
        if !self.exe_path.is_empty() {
            self.exe_path.clone()
        } else if !self.class.is_empty() {
            format!("class:{}", self.class)
        } else {
            format!("id:{}", self.id)
        }
    }

    /// Человекочитаемый лейбл для UI.
    pub fn label(&self) -> String {
        if !self.class.is_empty() {
            self.class.clone()
        } else if !self.title.is_empty() {
            self.title.clone()
        } else {
            format!("0x{:x}", self.id)
        }
    }
}

fn default_kind() -> BinaryKind {
    BinaryKind::Unknown
}