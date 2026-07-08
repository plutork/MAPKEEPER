//! Projects list shape (roadmap D-12): a portable pointer file so the
//! launcher can list known worlds. Truth is always the world folder +
//! `mapkeeper.toml`; this file is just links. `server`/`cli` own the actual
//! path resolution (`%APPDATA%`/home dir) and filesystem reads/writes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectsFile {
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
}

impl ProjectsFile {
    pub fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{\"projects\":[]}".to_string())
    }

    /// Insert or update by path (a world folder has exactly one entry).
    pub fn upsert(&mut self, entry: ProjectEntry) {
        if let Some(existing) = self.projects.iter_mut().find(|p| p.path == entry.path) {
            *existing = entry;
        } else {
            self.projects.push(entry);
        }
    }
}

/// `%APPDATA%/mapkeeper/projects.json` on Windows, `~/.config/mapkeeper/projects.json`
/// elsewhere. Pure function of env values so it is testable without touching real env.
pub fn projects_file_path(appdata: Option<&str>, home: Option<&str>) -> String {
    if let Some(appdata) = appdata.filter(|s| !s.is_empty()) {
        return format!(
            "{}/mapkeeper/projects.json",
            appdata.trim_end_matches(['/', '\\'])
        );
    }
    let home = home.filter(|s| !s.is_empty()).unwrap_or(".");
    format!(
        "{}/.config/mapkeeper/projects.json",
        home.trim_end_matches(['/', '\\'])
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_appdata_over_home() {
        let p = projects_file_path(Some("C:/Users/me/AppData/Roaming"), Some("C:/Users/me"));
        assert_eq!(p, "C:/Users/me/AppData/Roaming/mapkeeper/projects.json");
    }

    #[test]
    fn falls_back_to_home_config() {
        let p = projects_file_path(None, Some("/home/me"));
        assert_eq!(p, "/home/me/.config/mapkeeper/projects.json");
    }

    #[test]
    fn upsert_replaces_by_path_not_id() {
        let mut file = ProjectsFile::default();
        file.upsert(ProjectEntry {
            id: "a".into(),
            path: "/w".into(),
        });
        file.upsert(ProjectEntry {
            id: "renamed".into(),
            path: "/w".into(),
        });
        assert_eq!(file.projects.len(), 1);
        assert_eq!(file.projects[0].id, "renamed");
    }

    #[test]
    fn parse_missing_or_bad_json_is_empty() {
        assert!(ProjectsFile::parse("not json").projects.is_empty());
        assert!(ProjectsFile::parse("").projects.is_empty());
    }
}
