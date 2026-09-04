use anyhow::Result;

pub trait Sensor: Send + Sync {
    fn name(&self) -> &str;
    fn read(&mut self) -> Result<f64>;
    fn is_online(&self) -> bool;
    fn unit(&self) -> &str;

    fn read_normalised(&mut self) -> Result<f64> {
        let value = self.read()?;
        Ok(value.clamp(0.0, 1.0))
    }
}

pub struct SensorArray {
    pub sensors: Vec<Box<dyn Sensor>>,
}

impl SensorArray {
    pub fn new() -> Self {
        Self {
            sensors: Vec::new(),
        }
    }

    pub fn add(&mut self, sensor: Box<dyn Sensor>) {
        self.sensors.push(sensor);
    }

    pub fn read_all(&mut self) -> Vec<(String, f64, bool)> {
        self.sensors
            .iter_mut()
            .map(|s| {
                let name = s.name().to_string();
                let online = s.is_online();
                let value = if online {
                    s.read().unwrap_or(0.0)
                } else {
                    0.0
                };
                (name, value, online)
            })
            .collect()
    }

    pub fn get_reading(&mut self, name: &str) -> Option<(f64, bool)> {
        self.sensors
            .iter_mut()
            .find(|s| s.name() == name)
            .map(|s| {
                let online = s.is_online();
                let value = if online {
                    s.read().unwrap_or(0.0)
                } else {
                    0.0
                };
                (value, online)
            })
    }

    pub fn online_count(&self) -> usize {
        self.sensors.iter().filter(|s| s.is_online()).count()
    }

    pub fn total_count(&self) -> usize {
        self.sensors.len()
    }
}
