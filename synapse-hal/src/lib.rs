pub mod actuator;
pub mod percepcion;
pub mod sensor;
pub mod virtual_actuator;
pub mod virtual_sensor;

#[cfg(feature = "rpi")]
pub mod rpi_actuator;

pub use actuator::{Actuator, ActuatorCommand};
pub use percepcion::{Oido, Percepcion, PercepcionVirtual, Vista};
pub use sensor::Sensor;
pub use virtual_actuator::VirtualActuator;
pub use virtual_sensor::VirtualSensor;

#[cfg(feature = "rpi")]
pub use rpi_actuator::{is_raspberry_pi, Pca9685, PiMotor, PiServo};

#[cfg(feature = "percepcion_real")]
pub use percepcion::PercepcionReal;
