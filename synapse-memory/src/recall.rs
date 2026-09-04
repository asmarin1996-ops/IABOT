use anyhow::Result;
use crate::database::MemoryDatabase;
use crate::experience::StoredExperience;

pub struct RecallEngine<'a> {
    db: &'a MemoryDatabase,
}

impl<'a> RecallEngine<'a> {
    pub fn new(db: &'a MemoryDatabase) -> Self {
        Self { db }
    }

    pub fn recall_similar(&self, features: &[f64], limit: usize) -> Result<Vec<StoredExperience>> {
        let recent = self.db.get_recent_experiences(limit * 3)?;
        let mut scored: Vec<(f64, String)> = recent
            .into_iter()
            .map(|desc| {
                let score = desc.len() as f64 * 0.01;
                (score, desc)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        Ok(scored
            .into_iter()
            .enumerate()
            .map(|(i, (score, desc))| StoredExperience {
                id: i as i64,
                timestamp: chrono::Utc::now(),
                state_features: features.to_vec(),
                action: "recalled".to_string(),
                reward: score,
                next_state_features: features.to_vec(),
                description: desc,
                tags: vec!["recalled".to_string()],
            })
            .collect())
    }

    pub fn what_do_i_know_about(&self, topic: &str) -> Result<Vec<(String, String)>> {
        let knowledge = self.db.search_knowledge(topic)?;
        Ok(knowledge)
    }

    pub fn recent_learning_summary(&self) -> Result<String> {
        let count = self.db.count_experiences()?;
        let k_count = self.db.count_knowledge()?;
        let recent = self.db.get_recent_experiences(5)?;

        let mut summary = format!(
            "=== Resumen de Memoria ===\nExperiencias: {} | Conocimientos: {}\n",
            count, k_count
        );

        if recent.is_empty() {
            summary.push_str("Sin experiencias recientes.\n");
        } else {
            summary.push_str("Ultimas experiencias:\n");
            for exp in &recent {
                summary.push_str(&format!("  {}\n", exp));
            }
        }

        Ok(summary)
    }
}
