use crate::emotion::EmotionalState;
use crate::monitor::SelfMonitor;

#[derive(Debug, Clone)]
pub struct AdaptationRule {
    pub condition: String,
    pub action: String,
    pub confidence: f64,
    pub times_applied: u64,
}

pub struct AdaptationEngine {
    pub rules: Vec<AdaptationRule>,
    pub adaptation_rate: f64,
    pub total_adaptations: u64,
}

impl AdaptationEngine {
    pub fn new() -> Self {
        let default_rules = vec![
            AdaptationRule {
                condition: "stress_alto".to_string(),
                action: "reducir_velocidad".to_string(),
                confidence: 0.7,
                times_applied: 0,
            },
            AdaptationRule {
                condition: "bateria_baja".to_string(),
                action: "ahorrar_energia".to_string(),
                confidence: 0.9,
                times_applied: 0,
            },
            AdaptationRule {
                condition: "muchos_errores".to_string(),
                action: "ser_cauto".to_string(),
                confidence: 0.8,
                times_applied: 0,
            },
            AdaptationRule {
                condition: "exceso_exploracion".to_string(),
                action: "explotar_conocimiento".to_string(),
                confidence: 0.6,
                times_applied: 0,
            },
        ];

        Self {
            rules: default_rules,
            adaptation_rate: 0.1,
            total_adaptations: 0,
        }
    }

    pub fn evaluate(
        &mut self,
        emotion: &EmotionalState,
        monitor: &SelfMonitor,
    ) -> Vec<String> {
        let mut adaptations = Vec::new();

        if emotion.stress > 0.7 {
            if let Some(rule) = self.rules.iter_mut().find(|r| r.condition == "stress_alto") {
                rule.times_applied += 1;
                self.total_adaptations += 1;
                adaptations.push(rule.action.clone());
            }
        }

        if emotion.energy_level < 0.3 {
            if let Some(rule) = self
                .rules
                .iter_mut()
                .find(|r| r.condition == "bateria_baja")
            {
                rule.times_applied += 1;
                self.total_adaptations += 1;
                adaptations.push(rule.action.clone());
            }
        }

        if monitor.health.errors_count > 10 {
            if let Some(rule) = self
                .rules
                .iter_mut()
                .find(|r| r.condition == "muchos_errores")
            {
                rule.times_applied += 1;
                self.total_adaptations += 1;
                adaptations.push(rule.action.clone());
            }
        }

        if emotion.curiosity > 0.8 && emotion.confidence > 0.7 {
            adaptations.push("explorar_nuevo".to_string());
        }

        adaptations
    }

    pub fn learn_from_outcome(
        &mut self,
        rule_name: &str,
        success: bool,
    ) {
        if let Some(rule) = self.rules.iter_mut().find(|r| r.condition == rule_name) {
            if success {
                rule.confidence = (rule.confidence + self.adaptation_rate).min(1.0);
            } else {
                rule.confidence = (rule.confidence - self.adaptation_rate * 0.5).max(0.0);
            }
        }
    }

    pub fn add_rule(&mut self, condition: &str, action: &str, confidence: f64) {
        self.rules.push(AdaptationRule {
            condition: condition.to_string(),
            action: action.to_string(),
            confidence,
            times_applied: 0,
        });
    }

    pub fn report(&self) -> String {
        let mut report = String::new();
        report.push_str("=== Reglas de Adaptacion ===\n");
        for rule in &self.rules {
            report.push_str(&format!(
                "  {} -> {} (conf: {:.0}%, aplicada: {} veces)\n",
                rule.condition,
                rule.action,
                rule.confidence * 100.0,
                rule.times_applied
            ));
        }
        report.push_str(&format!(
            "\nTotal adaptaciones: {}\n",
            self.total_adaptations
        ));
        report
    }
}
