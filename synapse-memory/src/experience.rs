use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredExperience {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub state_features: Vec<f64>,
    pub action: String,
    pub reward: f64,
    pub next_state_features: Vec<f64>,
    pub description: String,
    pub tags: Vec<String>,
}

impl StoredExperience {
    pub fn similarity(&self, other_features: &[f64]) -> f64 {
        if self.state_features.len() != other_features.len() {
            return 0.0;
        }

        let diff_sum: f64 = self
            .state_features
            .iter()
            .zip(other_features.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum();

        let mse = diff_sum / self.state_features.len() as f64;
        1.0 - mse.min(1.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub id: i64,
    pub name: String,
    pub pattern_data: String,
    pub confidence: f64,
    pub times_matched: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub memory_type: String,
    pub content: String,
    pub importance: f64,
    pub access_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub id: i64,
    pub key: String,
    pub value: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub source: Option<String>,
}
