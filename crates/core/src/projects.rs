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
        let parsed = ProjectsFile::parse(&file.to_json_pretty());
        assert_eq!(parsed.projects.len(), 1);
        assert_eq!(parsed.projects[0].id, "renamed");
    }

    #[test]
    fn prefers_appdata_registry_path() {
        assert_eq!(
            projects_file_path(Some("C:/Users/me/AppData/Roaming"), Some("C:/Users/me")),
            "C:/Users/me/AppData/Roaming/mapkeeper/projects.json"
        );
    }
}
