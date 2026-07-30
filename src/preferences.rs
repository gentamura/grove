use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MapNodeOffset {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub groups: Vec<Group>,
    pub assignments: HashMap<String, String>,
    #[serde(default)]
    pub map_node_offsets: HashMap<String, MapNodeOffset>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            groups: vec![
                Group {
                    id: "focus".into(),
                    name: "Focus".into(),
                },
                Group {
                    id: "watching".into(),
                    name: "Watching".into(),
                },
            ],
            assignments: HashMap::new(),
            map_node_offsets: HashMap::new(),
        }
    }
}

impl Preferences {
    pub fn load() -> Self {
        let path = preferences_path();
        Self::load_from(&path).unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> Option<Self> {
        let bytes = fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn update_and_save(&mut self, update: impl FnOnce(&mut Self) -> bool) -> io::Result<bool> {
        self.update_and_save_to(&preferences_path(), update)
    }

    fn update_and_save_to(
        &mut self,
        path: &Path,
        update: impl FnOnce(&mut Self) -> bool,
    ) -> io::Result<bool> {
        let previous = self.clone();
        if !update(self) {
            return Ok(false);
        }
        if let Err(error) = self.save_to(path) {
            *self = previous;
            return Err(error);
        }
        Ok(true)
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(&temporary, bytes)?;
        fs::rename(temporary, path)
    }

    pub fn create_group(&mut self, name: &str) -> Option<String> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }

        let stem = slugify(name);
        let mut id = stem.clone();
        let mut suffix = 2;
        while self.groups.iter().any(|group| group.id == id) {
            id = format!("{stem}-{suffix}");
            suffix += 1;
        }
        self.groups.push(Group {
            id: id.clone(),
            name: name.to_owned(),
        });
        Some(id)
    }

    pub fn delete_group(&mut self, group_id: &str) {
        self.groups.retain(|group| group.id != group_id);
        self.assignments.retain(|_, value| value != group_id);
    }

    pub fn assign(&mut self, session_id: &str, group_id: Option<&str>) {
        match group_id {
            Some(group_id) if self.groups.iter().any(|group| group.id == group_id) => {
                self.assignments
                    .insert(session_id.to_owned(), group_id.to_owned());
            }
            _ => {
                self.assignments.remove(session_id);
            }
        }
    }
}

pub fn preferences_path() -> PathBuf {
    dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Grove")
        .join("preferences.json")
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            slug.push(character);
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "group".into()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("preferences.json");
        let mut preferences = Preferences::default();
        let group = preferences.create_group("Release Train").unwrap();
        preferences.assign("session-1", Some(&group));
        preferences
            .map_node_offsets
            .insert("session:session-1".into(), MapNodeOffset { x: 42, y: -18 });
        preferences.save_to(&path).unwrap();

        assert_eq!(Preferences::load_from(&path), Some(preferences));
    }

    #[test]
    fn old_preferences_without_map_offsets_still_load() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("preferences.json");
        fs::write(
            &path,
            r#"{"groups":[{"id":"focus","name":"Focus"}],"assignments":{}}"#,
        )
        .unwrap();

        let preferences = Preferences::load_from(&path).unwrap();

        assert!(preferences.map_node_offsets.is_empty());
    }

    #[test]
    fn deleting_a_group_returns_sessions_to_ungrouped() {
        let mut preferences = Preferences::default();
        preferences.assign("session-1", Some("focus"));
        preferences.delete_group("focus");

        assert!(!preferences.assignments.contains_key("session-1"));
        assert!(preferences.groups.iter().all(|group| group.id != "focus"));
    }

    #[test]
    fn group_ids_are_stable_and_unique() {
        let mut preferences = Preferences::default();
        assert_eq!(
            preferences.create_group("Release Train").as_deref(),
            Some("release-train")
        );
        assert_eq!(
            preferences.create_group("Release Train").as_deref(),
            Some("release-train-2")
        );
        assert_eq!(preferences.create_group("開発").as_deref(), Some("開発"));
    }

    #[test]
    fn failed_transaction_rolls_back_in_memory_changes() {
        let temp = tempfile::tempdir().unwrap();
        let parent_file = temp.path().join("not-a-directory");
        fs::write(&parent_file, "occupied").unwrap();
        let path = parent_file.join("preferences.json");
        let mut preferences = Preferences::default();
        let before = preferences.clone();

        let result = preferences.update_and_save_to(&path, |preferences| {
            preferences.create_group("Will not persist").is_some()
        });

        assert!(result.is_err());
        assert_eq!(preferences, before);
    }
}
