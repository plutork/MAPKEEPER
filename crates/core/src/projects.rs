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
    /// Parse registry JSON. Malformed input is an error — never silent empty.
    pub fn parse(raw: &str) -> Result<Self, String> {
        serde_json::from_str(raw).map_err(|error| format!("corrupt_registry: {error}"))
    }

    pub fn to_json_pretty(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|error| format!("serialize projects: {error}"))
    }

    pub fn upsert(&mut self, entry: ProjectEntry) {
        if let Some(existing) = self
            .projects
            .iter_mut()
            .find(|item| item.path == entry.path)
        {
            *existing = entry;
        } else {
            self.projects.push(entry);
        }
    }
}

pub fn projects_file_path(appdata: Option<&str>, home: Option<&str>) -> String {
    if let Some(appdata) = appdata.filter(|value| !value.is_empty()) {
        return format!(
            "{}/mapkeeper/projects.json",
            appdata.trim_end_matches(['/', '\\'])
        );
    }
    let home = home.filter(|value| !value.is_empty()).unwrap_or(".");
    format!(
        "{}/.config/mapkeeper/projects.json",
        home.trim_end_matches(['/', '\\'])
    )
}

pub fn trash_dir_path(appdata: Option<&str>, home: Option<&str>) -> String {
    if let Some(appdata) = appdata.filter(|value| !value.is_empty()) {
        return format!(
            "{}/mapkeeper/trash",
            appdata.trim_end_matches(['/', '\\'])
        );
    }
    let home = home.filter(|value| !value.is_empty()).unwrap_or(".");
    format!(
        "{}/.config/mapkeeper/trash",
        home.trim_end_matches(['/', '\\'])
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_registry_round_trips() {
        let mut file = ProjectsFile::default();
        file.upsert(ProjectEntry {
            id: "first".into(),
            path: "/world".into(),
        });
        file.upsert(ProjectEntry {
            id: "renamed".into(),
            path: "/world".into(),
        });
        let parsed = ProjectsFile::parse(&file.to_json_pretty().unwrap()).unwrap();
        assert_eq!(parsed.projects.len(), 1);
        assert_eq!(parsed.projects[0].id, "renamed");
    }

    #[test]
    fn malformed_registry_is_error_not_empty() {
        let err = ProjectsFile::parse("{not json").unwrap_err();
        assert!(err.starts_with("corrupt_registry:"));
    }

    #[test]
    fn prefers_appdata_registry_path() {
        assert_eq!(
            projects_file_path(Some("C:/Users/me/AppData/Roaming"), Some("C:/Users/me")),
            "C:/Users/me/AppData/Roaming/mapkeeper/projects.json"
        );
    }

    #[test]
    fn trash_under_appdata() {
        assert_eq!(
            trash_dir_path(Some("C:/Users/me/AppData/Roaming"), None),
            "C:/Users/me/AppData/Roaming/mapkeeper/trash"
        );
    }
}
