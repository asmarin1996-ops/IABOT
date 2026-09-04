use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmotionalState {
    pub confidence: f64,
    pub curiosity: f64,
    pub stress: f64,
    pub satisfaction: f64,
    pub caution: f64,
    pub energy_level: f64,
}

impl EmotionalState {
    pub fn new() -> Self {
        Self {
            confidence: 0.5,
            curiosity: 0.8,
            stress: 0.0,
            satisfaction: 0.5,
            caution: 0.3,
            energy_level: 1.0,
        }
    }

    pub fn on_success(&mut self) {
        self.confidence = (self.confidence + 0.08).min(1.0);
        self.satisfaction = (self.satisfaction + 0.1).min(1.0);
        self.stress = (self.stress - 0.05).max(0.0);
        self.curiosity = (self.curiosity + 0.02).min(1.0);
    }

    pub fn on_failure(&mut self) {
        self.confidence = (self.confidence - 0.05).max(0.0);
        self.stress = (self.stress + 0.08).min(1.0);
        self.satisfaction = (self.satisfaction - 0.03).max(0.0);
        self.caution = (self.caution + 0.05).min(1.0);
    }

    pub fn on_new_situation(&mut self) {
        self.curiosity = (self.curiosity + 0.05).min(1.0);
        self.caution = (self.caution + 0.03).min(1.0);
    }

    pub fn on_rest(&mut self) {
        self.stress = (self.stress - 0.1).max(0.0);
        self.energy_level = (self.energy_level + 0.05).min(1.0);
        self.confidence = (self.confidence + 0.01).min(1.0);
    }

    pub fn dominant_emotion(&self) -> &str {
        let emotions = [
            ("confianza", self.confidence),
            ("curiosidad", self.curiosity),
            ("estres", self.stress),
            ("satisfaccion", self.satisfaction),
            ("cautela", self.caution),
        ];

        emotions
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(name, _)| *name)
            .unwrap_or("neutral")
    }

    pub fn mood_summary(&self) -> String {
        let mood = self.dominant_emotion();
        let emoji = match mood {
            "confianza" => "[+]",
            "curiosidad" => "[?]",
            "estres" => "[!]",
            "satisfaccion" => "[*]",
            "cautela" => "[~]",
            _ => "[ ]",
        };

        format!(
            "{} {} | Conf: {:.0}% | Curios: {:.0}% | Estres: {:.0}% | Energia: {:.0}%",
            emoji,
            mood,
            self.confidence * 100.0,
            self.curiosity * 100.0,
            self.stress * 100.0,
            self.energy_level * 100.0,
        )
    }

    pub fn should_explore(&self) -> bool {
        self.curiosity > 0.6 && self.stress < 0.5 && self.energy_level > 0.3
    }

    pub fn should_be_cautious(&self) -> bool {
        self.caution > 0.6 || self.stress > 0.7 || self.confidence < 0.3
    }
}
