use crate::sensor::Sensor;
use anyhow::Result;

pub struct VirtualSensor {
    pub name: String,
    pub value: f64,
    pub noise_level: f64,
    pub online: bool,
}

impl VirtualSensor {
    pub fn new(name: &str, initial_value: f64) -> Self {
        Self {
            name: name.to_string(),
            value: initial_value,
            noise_level: 0.05,
            online: true,
        }
    }

    pub fn set_value(&mut self, value: f64) {
        self.value = value;
    }

    pub fn add_noise(&mut self, noise: f64) {
        self.noise_level = noise;
    }

    pub fn go_offline(&mut self) {
        self.online = false;
    }

    pub fn go_online(&mut self) {
        self.online = true;
    }
}

impl Sensor for VirtualSensor {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self) -> Result<f64> {
        if !self.online {
            return Ok(0.0);
        }

        let noise = rand::random::<f64>() * self.noise_level * 2.0 - self.noise_level;
        Ok((self.value + noise).clamp(0.0, 1.0))
    }

    fn is_online(&self) -> bool {
        self.online
    }

    fn unit(&self) -> &str {
        "virtual"
    }
}
