use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::brain::{Action, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reward {
    Positive(f64),
    Negative(f64),
    Zero,
}

impl Reward {
    pub fn value(&self) -> f64 {
        match self {
            Reward::Positive(v) => *v,
            Reward::Negative(v) => -*v,
            Reward::Zero => 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experience {
    pub id: u64,
    pub timestamp: DateTime<Utc>,
    pub state: State,
    pub action: Action,
    pub reward: Reward,
    pub next_state: State,
    pub description: String,
}

#[derive(Debug)]
pub struct LearningEngine {
    pub experiences: Vec<Experience>,
    pub total_reward: f64,
    pub episodes: u64,
    pub current_episode_reward: f64,
    pub next_id: u64,
}

impl LearningEngine {
    pub fn new() -> Self {
        Self {
            experiences: Vec::new(),
            total_reward: 0.0,
            episodes: 0,
            current_episode_reward: 0.0,
            next_id: 0,
        }
    }

    pub fn record_experience(
        &mut self,
        state: State,
        action: Action,
        reward: Reward,
        next_state: State,
        description: String,
    ) -> Experience {
        let exp = Experience {
            id: self.next_id,
            timestamp: Utc::now(),
            state,
            action,
            reward: reward.clone(),
            next_state,
            description,
        };

        self.total_reward += reward.value();
        self.current_episode_reward += reward.value();
        self.next_id += 1;
        self.experiences.push(exp.clone());

        exp
    }

    pub fn end_episode(&mut self) -> f64 {
        let episode_reward = self.current_episode_reward;
        self.episodes += 1;
        self.current_episode_reward = 0.0;
        episode_reward
    }

    pub fn recent_experiences(&self, n: usize) -> &[Experience] {
        let start = self.experiences.len().saturating_sub(n);
        &self.experiences[start..]
    }

    pub fn best_action_ever(&self, state: &State) -> Option<Action> {
        self.experiences
            .iter()
            .filter(|e| {
                let similar = e
                    .state
                    .features
                    .iter()
                    .zip(state.features.iter())
                    .all(|(a, b)| (a - b).abs() < 0.15);
                similar
            })
            .max_by(|a, b| a.reward.value().partial_cmp(&b.reward.value()).unwrap_or(std::cmp::Ordering::Equal))
            .map(|e| e.action)
    }

    pub fn avg_reward(&self) -> f64 {
        if self.experiences.is_empty() {
            0.0
        } else {
            self.total_reward / self.experiences.len() as f64
        }
    }

    pub fn stats(&self) -> LearningStats {
        LearningStats {
            total_experiences: self.experiences.len(),
            total_reward: self.total_reward,
            avg_reward: self.avg_reward(),
            episodes: self.episodes,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningStats {
    pub total_experiences: usize,
    pub total_reward: f64,
    pub avg_reward: f64,
    pub episodes: u64,
}
