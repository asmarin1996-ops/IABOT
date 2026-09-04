use crate::actuator::{Actuator, ActuatorCommand};
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct VirtualActuator {
    pub name: String,
    pub online: bool,
    pub last_command: Option<ActuatorCommand>,
    pub total_executions: u64,
    pub execution_log: Vec<String>,
}

impl VirtualActuator {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            online: true,
            last_command: None,
            total_executions: 0,
            execution_log: Vec::new(),
        }
    }

    pub fn last_command_str(&self) -> String {
        match &self.last_command {
            Some(cmd) => format!("{:?}", cmd),
            None => "ninguno".to_string(),
        }
    }
}

impl Actuator for VirtualActuator {
    fn name(&self) -> &str {
        &self.name
    }

    fn execute(&mut self, command: ActuatorCommand) -> Result<()> {
        if !self.online {
            return Err(anyhow::anyhow!("Actuator {} is offline", self.name));
        }

        self.last_command = Some(command.clone());
        self.total_executions += 1;

        let log_entry = format!(
            "[{}] {:?}",
            chrono::Utc::now().format("%H:%M:%S"),
            command
        );
        self.execution_log.push(log_entry);

        if self.execution_log.len() > 100 {
            self.execution_log.drain(0..50);
        }

        Ok(())
    }

    fn is_online(&self) -> bool {
        self.online
    }
}
