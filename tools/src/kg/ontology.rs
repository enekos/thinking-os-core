use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Relation {
    #[serde(default)]
    pub domain: Vec<String>,
    #[serde(default)]
    pub range: Vec<String>,
    #[serde(default)]
    pub card: String,
    #[serde(default)]
    pub deprecated: bool,
}

impl Relation {
    pub fn accepts_domain(&self, kind: &str) -> bool {
        self.domain == ["*"] || self.domain.iter().any(|d| d == kind)
    }
    pub fn accepts_range(&self, kind: &str) -> bool {
        self.range == ["*"] || self.range.iter().any(|r| r == kind)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Provenance {
    pub conf: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Fusion {
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Gate {
    pub max_mentions_ratio: f64,
    pub stale_project_days: i64,
    #[serde(default = "default_exemplar_paths")]
    pub max_exemplar_paths: usize,
}

fn default_exemplar_paths() -> usize {
    25
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ontology {
    pub nodes: BTreeMap<String, serde_yaml::Value>,
    pub relations: BTreeMap<String, Relation>,
    pub provenance: BTreeMap<String, Provenance>,
    pub fusion: Fusion,
    pub gate: Gate,
}

impl Ontology {
    pub fn load(path: &Path) -> Result<Ontology, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_yaml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn canon(&self, id: &str) -> String {
        let id = id.trim();
        self.fusion.aliases.get(id).cloned().unwrap_or_else(|| id.to_string())
    }

    pub fn conf_for(&self, prov: &str) -> f64 {
        self.provenance.get(prov).map(|p| p.conf).unwrap_or(0.5)
    }

    pub fn has_relation(&self, rel: &str) -> bool {
        self.relations.contains_key(rel)
    }
}
