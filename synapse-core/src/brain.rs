use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub features: Vec<f64>,
}

impl State {
    pub fn new(features: Vec<f64>) -> Self {
        Self { features }
    }

    pub fn quantize(&self, bins: usize) -> Vec<usize> {
        self.features
            .iter()
            .map(|f| {
                let clamped = f.clamp(0.0, 1.0);
                (clamped * (bins as f64 - 1.0)).round() as usize
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Forward,
    Backward,
    TurnLeft,
    TurnRight,
    Stop,
    Custom(u8),
}

impl Action {
    pub fn all() -> &'static [Action] {
        &[
            Action::Forward,
            Action::Backward,
            Action::TurnLeft,
            Action::TurnRight,
            Action::Stop,
        ]
    }

    pub fn to_index(self) -> usize {
        match self {
            Action::Forward => 0,
            Action::Backward => 1,
            Action::TurnLeft => 2,
            Action::TurnRight => 3,
            Action::Stop => 4,
            Action::Custom(i) => 5 + i as usize,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QTable {
    pub values: HashMap<Vec<usize>, Vec<f64>>,
    pub num_actions: usize,
    pub learning_rate: f64,
    pub discount_factor: f64,
    pub exploration_rate: f64,
    pub exploration_decay: f64,
    pub min_exploration: f64,
}

impl QTable {
    pub fn new(num_actions: usize, learning_rate: f64, discount_factor: f64) -> Self {
        Self {
            values: HashMap::new(),
            num_actions,
            learning_rate,
            discount_factor,
            exploration_rate: 1.0,
            exploration_decay: 0.999,
            min_exploration: 0.1,
        }
    }

    fn get_values(&mut self, state: &[usize]) -> Vec<f64> {
        self.values
            .entry(state.to_vec())
            .or_insert_with(|| vec![0.0; self.num_actions])
            .clone()
    }

    pub fn best_action(&mut self, state: &[usize]) -> usize {
        let values = self.get_values(state);
        values
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    pub fn update(
        &mut self,
        state: &[usize],
        action: usize,
        reward: f64,
        next_state: &[usize],
    ) {
        let current_values = self.get_values(state);
        let next_values = self.get_values(next_state);

        let max_next = next_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let current_q = current_values[action];

        let new_q = current_q
            + self.learning_rate
                * (reward + self.discount_factor * max_next - current_q);

        if let Some(values) = self.values.get_mut(state) {
            values[action] = new_q;
        }
    }

    pub fn decay_exploration(&mut self) {
        self.exploration_rate =
            (self.exploration_rate * self.exploration_decay).max(self.min_exploration);
    }

    pub fn should_explore(&self) -> bool {
        rand::random::<f64>() < self.exploration_rate
    }

    pub fn num_states(&self) -> usize {
        self.values.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainSnapshot {
    pub q_table_size: usize,
    pub exploration_rate: f64,
    pub total_updates: u64,
    pub num_states: usize,
}

pub struct Brain {
    pub q_table: QTable,
    pub total_updates: u64,
    pub state_bins: usize,
}

impl Brain {
    pub fn new(_state_features: usize, state_bins: usize) -> Self {
        let num_actions = Action::all().len();
        Self {
            q_table: QTable::new(num_actions, 0.2, 0.95),
            total_updates: 0,
            state_bins,
        }
    }

    pub fn decide(&mut self, state: &State) -> Action {
        let quantized = state.quantize(self.state_bins);

        if self.q_table.should_explore() {
            let idx = rand::random::<usize>() % Action::all().len();
            return Action::all()[idx];
        }

        let idx = self.q_table.best_action(&quantized);
        Action::all()[idx]
    }

    pub fn learn(&mut self, state: &State, action: Action, reward: f64, next_state: &State) {
        let quantized = state.quantize(self.state_bins);
        let next_quantized = next_state.quantize(self.state_bins);

        self.q_table
            .update(&quantized, action.to_index(), reward, &next_quantized);
        self.q_table.decay_exploration();
        self.total_updates += 1;
    }

    pub fn snapshot(&self) -> BrainSnapshot {
        BrainSnapshot {
            q_table_size: self.q_table.values.len(),
            exploration_rate: self.q_table.exploration_rate,
            total_updates: self.total_updates,
            num_states: self.q_table.num_states(),
        }
    }
}

pub trait QLearning {
    fn get_q(&self, state: &[usize], action: usize) -> f64;
    fn set_q(&mut self, state: &[usize], action: usize, value: f64);
}

impl QLearning for HashMap<Vec<usize>, Vec<f64>> {
    fn get_q(&self, state: &[usize], action: usize) -> f64 {
        self.get(state)
            .and_then(|v| v.get(action))
            .copied()
            .unwrap_or(0.0)
    }

    fn set_q(&mut self, state: &[usize], action: usize, value: f64) {
        self.entry(state.to_vec())
            .or_insert_with(|| vec![0.0; 5])
        [action] = value;
    }
}
