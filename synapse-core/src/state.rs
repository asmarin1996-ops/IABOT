use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentObservation {
    pub sensors: HashMap<String, f64>,
    pub obstacles_nearby: bool,
    pub light_level: f64,
    pub temperature: f64,
    pub battery_level: f64,
    pub timestamp: u64,
}

impl EnvironmentObservation {
    pub fn new() -> Self {
        Self {
            sensors: HashMap::new(),
            obstacles_nearby: false,
            light_level: 0.5,
            temperature: 25.0,
            battery_level: 100.0,
            timestamp: 0,
        }
    }

    pub fn to_features(&self) -> Vec<f64> {
        let obstacle_val = if self.obstacles_nearby { 1.0 } else { 0.0 };
        let mut features = vec![
            self.light_level,
            self.temperature / 100.0,
            self.battery_level / 100.0,
            obstacle_val,
        ];

        let mut sensor_keys: Vec<&String> = self.sensors.keys().collect();
        sensor_keys.sort();
        for key in sensor_keys {
            if let Some(v) = self.sensors.get(key) {
                features.push(*v);
            }
        }

        features.resize(8, 0.0);
        features
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub position: (f64, f64),
    pub orientation: f64,
    pub speed: f64,
    pub energy: f64,
    pub confidence: f64,
    pub curiosity: f64,
    pub stress: f64,
    pub total_actions: u64,
    pub successful_actions: u64,
}

impl AgentState {
    pub fn new() -> Self {
        Self {
            position: (0.0, 0.0),
            orientation: 0.0,
            speed: 0.0,
            energy: 100.0,
            confidence: 0.5,
            curiosity: 0.8,
            stress: 0.0,
            total_actions: 0,
            successful_actions: 0,
        }
    }

    pub fn to_features(&self) -> Vec<f64> {
        vec![
            self.position.0 / 100.0,
            self.position.1 / 100.0,
            self.orientation / (std::f64::consts::PI * 2.0),
            self.speed / 10.0,
            self.energy / 100.0,
            self.confidence,
            self.curiosity,
            self.stress,
        ]
    }

    pub fn record_action(&mut self, success: bool) {
        self.total_actions += 1;
        if success {
            self.successful_actions += 1;
            self.confidence = (self.confidence + 0.05).min(1.0);
            self.stress = (self.stress - 0.02).max(0.0);
        } else {
            self.confidence = (self.confidence - 0.03).max(0.0);
            self.stress = (self.stress + 0.05).min(1.0);
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_actions == 0 {
            0.0
        } else {
            self.successful_actions as f64 / self.total_actions as f64
        }
    }
}
