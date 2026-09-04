use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub sensor_status: HashMap<String, SensorHealth>,
    pub uptime_seconds: u64,
    pub errors_count: u64,
    pub warnings_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorHealth {
    pub name: String,
    pub online: bool,
    pub last_reading: f64,
    pub error_count: u64,
    pub avg_response_ms: f64,
}

pub struct SelfMonitor {
    pub health: SystemHealth,
    pub start_time: std::time::Instant,
    pub error_log: Vec<ErrorEvent>,
    pub anomaly_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub timestamp: String,
    pub component: String,
    pub message: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

impl SelfMonitor {
    pub fn new() -> Self {
        Self {
            health: SystemHealth {
                cpu_usage: 0.0,
                memory_usage: 0.0,
                sensor_status: HashMap::new(),
                uptime_seconds: 0,
                errors_count: 0,
                warnings_count: 0,
            },
            start_time: std::time::Instant::now(),
            error_log: Vec::new(),
            anomaly_threshold: 0.8,
        }
    }

    pub fn update_uptime(&mut self) {
        self.health.uptime_seconds = self.start_time.elapsed().as_secs();
    }

    pub fn register_sensor(&mut self, name: &str) {
        self.health.sensor_status.insert(
            name.to_string(),
            SensorHealth {
                name: name.to_string(),
                online: true,
                last_reading: 0.0,
                error_count: 0,
                avg_response_ms: 0.0,
            },
        );
    }

    pub fn record_sensor_reading(&mut self, sensor_name: &str, reading: f64, response_ms: f64) {
        if let Some(sensor) = self.health.sensor_status.get_mut(sensor_name) {
            sensor.last_reading = reading;
            sensor.avg_response_ms = sensor.avg_response_ms * 0.9 + response_ms * 0.1;
            sensor.online = true;
        }
    }

    pub fn record_error(&mut self, component: &str, message: &str, severity: Severity) {
        let event = ErrorEvent {
            timestamp: chrono::Utc::now().to_rfc3339(),
            component: component.to_string(),
            message: message.to_string(),
            severity: severity,
        };

        match &event.severity {
            Severity::Error | Severity::Critical => self.health.errors_count += 1,
            Severity::Warning => self.health.warnings_count += 1,
            Severity::Info => {}
        }

        self.error_log.push(event);
        if self.error_log.len() > 1000 {
            self.error_log.drain(0..500);
        }
    }

    pub fn detect_anomaly(&self, sensor_name: &str, current_value: f64) -> bool {
        if let Some(sensor) = self.health.sensor_status.get(sensor_name) {
            let diff = (current_value - sensor.last_reading).abs();
            diff > self.anomaly_threshold
        } else {
            true
        }
    }

    pub fn overall_health_score(&self) -> f64 {
        let sensor_score = if self.health.sensor_status.is_empty() {
            1.0
        } else {
            let online_count = self
                .health
                .sensor_status
                .values()
                .filter(|s| s.online)
                .count();
            online_count as f64 / self.health.sensor_status.len() as f64
        };

        let error_penalty = (self.health.errors_count as f64 * 0.05).min(0.5);
        let uptime_bonus = (self.health.uptime_seconds as f64 / 3600.0).min(0.1);

        (sensor_score - error_penalty + uptime_bonus).clamp(0.0, 1.0)
    }

    pub fn diagnostic_report(&self) -> String {
        let mut report = String::new();
        report.push_str("=== Diagnostico del Sistema ===\n");
        report.push_str(&format!(
            "Uptime: {}s\n",
            self.health.uptime_seconds
        ));
        report.push_str(&format!(
            "Salud general: {:.0}%\n",
            self.overall_health_score() * 100.0
        ));
        report.push_str(&format!(
            "Errores: {} | Warnings: {}\n",
            self.health.errors_count, self.health.warnings_count
        ));

        report.push_str("\nSensores:\n");
        for (name, sensor) in &self.health.sensor_status {
            let status = if sensor.online { "ONLINE" } else { "OFFLINE" };
            report.push_str(&format!(
                "  {} [{}] ultimo: {:.2} (prom: {:.1}ms)\n",
                name, status, sensor.last_reading, sensor.avg_response_ms
            ));
        }

        if !self.error_log.is_empty() {
            report.push_str(&format!(
                "\nUltimos {} errores:\n",
                self.error_log.len().min(5)
            ));
            for event in self.error_log.iter().rev().take(5) {
                report.push_str(&format!(
                    "  [{:?}] {}: {}\n",
                    event.severity, event.component, event.message
                ));
            }
        }

        report
    }
}
