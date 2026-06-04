use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct DaemonConfig {
    pub agenticos: AgenticosConfig,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub intelligence: agenticos_intelligence::IntelligenceConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgenticosConfig {
    pub mode: String,
    pub event_store: String,
    pub db_path: String,
    pub policy: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SafetyConfig {
    #[serde(default = "default_privileged")]
    pub privileged_execution: bool,
    #[serde(default = "default_llm")]
    pub llm_enabled: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            privileged_execution: false,
            llm_enabled: false,
        }
    }
}

fn default_privileged() -> bool {
    false
}

fn default_llm() -> bool {
    false
}

impl DaemonConfig {
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: DaemonConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn db_path(&self) -> &str {
        &self.agenticos.db_path
    }

    pub fn event_store(&self) -> &str {
        &self.agenticos.event_store
    }

    pub fn policy_path(&self) -> &str {
        &self.agenticos.policy
    }

    pub fn mode(&self) -> &str {
        &self.agenticos.mode
    }
}
