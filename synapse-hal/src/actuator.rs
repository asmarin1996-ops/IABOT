use anyhow::Result;

#[derive(Debug, Clone)]
pub enum ActuatorCommand {
    MoveForward(f64),
    MoveBackward(f64),
    TurnLeft(f64),
    TurnRight(f64),
    Stop,
    SetSpeed(f64),
    Custom(String, Vec<f64>),
}

pub trait Actuator {
    fn name(&self) -> &str;
    fn execute(&mut self, command: ActuatorCommand) -> Result<()>;
    fn is_online(&self) -> bool;
    fn stop(&mut self) -> Result<()> {
        self.execute(ActuatorCommand::Stop)
    }
}

pub struct ActuatorArray {
    pub actuators: Vec<Box<dyn Actuator>>,
}

impl ActuatorArray {
    pub fn new() -> Self {
        Self {
            actuators: Vec::new(),
        }
    }

    pub fn add(&mut self, actuator: Box<dyn Actuator>) {
        self.actuators.push(actuator);
    }

    pub fn execute_all(&mut self, command: ActuatorCommand) -> Vec<(String, Result<()>)> {
        self.actuators
            .iter_mut()
            .map(|a| {
                let name = a.name().to_string();
                let result = a.execute(command.clone());
                (name, result)
            })
            .collect()
    }

    pub fn stop_all(&mut self) -> Vec<(String, Result<()>)> {
        self.actuators
            .iter_mut()
            .map(|a| {
                let name = a.name().to_string();
                let result = a.stop();
                (name, result)
            })
            .collect()
    }
}
