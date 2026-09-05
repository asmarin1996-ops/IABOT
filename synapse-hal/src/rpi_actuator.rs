//! Actuadores fisicos para Raspberry Pi (feature "rpi").
//!
//! - Servos via controlador PCA9685 por I2C (estandar para robots con varios servos).
//! - Motores DC via PWM hardware de la GPIO + pines de direccion.
//!
//! Nunca crashea el proceso: si el bus I2C, la GPIO o el controlador no estan
//! disponibles, el actuador queda "offline" y `execute` devuelve un error que
//! el cerebro ignora, continuando con el robot virtual.

use crate::actuator::{Actuator, ActuatorCommand};
use anyhow::{anyhow, Result};
use std::time::Duration;

const PCA9685_ADDR: u16 = 0x40;
const MODE1: u8 = 0x00;
const PRESCALE: u8 = 0xFE;
const LED0_ON_L: u8 = 0x06;
const CHANNEL_STEP: u8 = 4;
const OSC_CLOCK_HZ: f64 = 25_000_000.0;
const PWM_COUNTS: f64 = 4096.0;
const SERVO_PERIOD_MS: f64 = 20.0;
const SERVO_MIN_MS: f64 = 0.5;
const SERVO_MAX_MS: f64 = 2.5;
const SERVO_MAX_ANGLE: f64 = 180.0;

/// Indica si el proceso esta corriendo en una Raspberry Pi real.
pub fn is_raspberry_pi() -> bool {
    std::fs::read_to_string("/proc/device-tree/model")
        .map(|m| m.contains("Raspberry Pi"))
        .unwrap_or(false)
}

/// Maneja el bus I2C hacia un PCA9685 (canales 0-15, 16 servos maximo).
pub struct Pca9685 {
    i2c: rppal::i2c::I2c,
}

impl Pca9685 {
    pub fn new() -> Result<Self> {
        let mut i2c = rppal::i2c::I2c::with_bus(1)?;
        i2c.set_slave_address(PCA9685_ADDR)?;
        // Reset (manda el chip a sleep) y despues fija frecuencia de 50 Hz.
        i2c.write(&[MODE1, 0x01])?;
        let prescale = ((OSC_CLOCK_HZ / (PWM_COUNTS * (1000.0 / SERVO_PERIOD_MS))) as u64) - 1;
        i2c.write(&[PRESCALE, prescale as u8])?;
        // MODE1: auto-increment + normal.
        i2c.write(&[MODE1, 0x21])?;
        Ok(Self { i2c })
    }

    pub fn set_pwm(&mut self, channel: u8, on: u16, off: u16) -> Result<()> {
        if channel > 15 {
            return Err(anyhow!("canal {} fuera de rango (0-15)", channel));
        }
        let reg = LED0_ON_L + channel * CHANNEL_STEP;
        self.i2c
            .write(&[
                reg,
                (on & 0xFF) as u8,
                (on >> 8) as u8,
                (off & 0xFF) as u8,
                (off >> 8) as u8,
            ])
            .map(|_| ())?;
        Ok(())
    }

    /// Escribe el pulso correspondiente a un angulo de servo (0-180 grados).
    pub fn set_servo_angle(&mut self, channel: u8, angle_deg: f64) -> Result<()> {
        let a = angle_deg.clamp(0.0, SERVO_MAX_ANGLE);
        let pulse_ms = SERVO_MIN_MS + (SERVO_MAX_MS - SERVO_MIN_MS) * (a / SERVO_MAX_ANGLE);
        let ticks = ((pulse_ms / SERVO_PERIOD_MS) * PWM_COUNTS) as u16;
        self.set_pwm(channel, 0, ticks)
    }
}

/// Un servo conectado a un canal del PCA9685.
pub struct PiServo {
    name: String,
    channel: u8,
    pca: Option<Pca9685>,
    online: bool,
    angle: f64,
}

impl PiServo {
    pub fn new(name: &str, channel: u8, home_angle: f64) -> Self {
        let pca = match Pca9685::new() {
            Ok(mut p) => {
                let _ = p.set_servo_angle(channel, home_angle);
                Some(p)
            }
            Err(err) => {
                log::warn!("servo '{}' sin bus I2C/PCA9685: {}", name, err);
                None
            }
        };
        let online = pca.is_some();
        Self {
            name: name.to_string(),
            channel,
            pca,
            online,
            angle: home_angle,
        }
    }

    fn apply_angle(&mut self, angle: f64) -> Result<()> {
        self.angle = angle.clamp(0.0, SERVO_MAX_ANGLE);
        log::info!("servo '{}' -> {} grados", self.name, self.angle);
        if let Some(pca) = &mut self.pca {
            pca.set_servo_angle(self.channel, self.angle)?;
        }
        Ok(())
    }
}

impl Actuator for PiServo {
    fn name(&self) -> &str {
        &self.name
    }

    fn execute(&mut self, command: ActuatorCommand) -> Result<()> {
        if !self.online {
            return Err(anyhow!("servo '{}' esta offline", self.name));
        }
        match command {
            ActuatorCommand::MoveForward(_) => self.apply_angle(90.0),
            ActuatorCommand::MoveBackward(_) => self.apply_angle(90.0),
            ActuatorCommand::TurnLeft(_) => self.apply_angle(135.0),
            ActuatorCommand::TurnRight(_) => self.apply_angle(45.0),
            ActuatorCommand::Stop => self.apply_angle(90.0),
            ActuatorCommand::SetSpeed(v) => {
                let t = v.clamp(0.0, 1.0);
                self.apply_angle(45.0 + t * 90.0)
            }
            ActuatorCommand::Custom(name, args) => {
                if name == self.name || name == self.name.trim_start_matches("servo_") {
                    let a = args.first().copied().unwrap_or(90.0);
                    self.apply_angle(a)
                } else {
                    Ok(())
                }
            }
        }
    }

    fn is_online(&self) -> bool {
        self.online
    }
}

/// Un motor DC con PWM de velocidad y dos pines de direccion (IN1/IN2).
pub struct PiMotor {
    name: String,
    pwm: Option<rppal::pwm::Pwm>,
    in_a: Option<rppal::gpio::OutputPin>,
    in_b: Option<rppal::gpio::OutputPin>,
    speed: f64,
}

impl PiMotor {
    /// `pwm_pin`: numero de GPIO con ALT PWM (12/18 dan Pwm0; 13/19 dan Pwm1).
    /// `pin_a`/`pin_b`: pines digitales de direccion del puente H.
    pub fn new(name: &str, pwm_pin: u8, pin_a: u8, pin_b: u8) -> Self {
        let channel = match pwm_pin {
            12 | 18 => rppal::pwm::Channel::Pwm0,
            13 | 19 => rppal::pwm::Channel::Pwm1,
            other => {
                log::warn!("pin PWM {} invalido para motor '{}'", other, name);
                return Self {
                    name: name.to_string(),
                    pwm: None,
                    in_a: None,
                    in_b: None,
                    speed: 0.0,
                };
            }
        };
        let pwm = rppal::pwm::Pwm::with_period(
            channel,
            Duration::from_millis(20),
            Duration::from_millis(10),
            rppal::pwm::Polarity::Normal,
            true,
        )
        .ok();
        let gpio = rppal::gpio::Gpio::new().ok();
        let in_a = gpio
            .as_ref()
            .and_then(|g| g.get(pin_a).ok())
            .map(|p| p.into_output());
        let in_b = gpio
            .as_ref()
            .and_then(|g| g.get(pin_b).ok())
            .map(|p| p.into_output());
        let online = pwm.is_some() && in_a.is_some() && in_b.is_some();
        if !online {
            log::warn!(
                "motor '{}' sin GPIO/PWM disponible (pwm={} a={} b={})",
                name,
                pwm_pin,
                pin_a,
                pin_b
            );
        }
        Self {
            name: name.to_string(),
            pwm,
            in_a,
            in_b,
            speed: 0.0,
        }
    }

    fn apply_speed(&mut self, signed: f64) -> Result<()> {
        self.speed = signed.clamp(-1.0, 1.0);
        let forward = self.speed >= 0.0;
        if let Some(p) = &mut self.in_a {
            if forward {
                p.set_high();
            } else {
                p.set_low();
            }
        }
        if let Some(p) = &mut self.in_b {
            if forward {
                p.set_low();
            } else {
                p.set_high();
            }
        }
        if let Some(p) = &mut self.pwm {
            p.set_duty_cycle(self.speed.abs())?;
        }
        log::info!("motor '{}' -> velocidad {:.2}", self.name, self.speed);
        Ok(())
    }
}

impl Actuator for PiMotor {
    fn name(&self) -> &str {
        &self.name
    }

    fn execute(&mut self, command: ActuatorCommand) -> Result<()> {
        if self.pwm.is_none() {
            return Err(anyhow!("motor '{}' esta offline", self.name));
        }
        match command {
            ActuatorCommand::MoveForward(v) => self.apply_speed(v.abs()),
            ActuatorCommand::MoveBackward(v) => self.apply_speed(-v.abs()),
            ActuatorCommand::Stop => self.apply_speed(0.0),
            ActuatorCommand::SetSpeed(v) => self.apply_speed(v.clamp(-1.0, 1.0)),
            ActuatorCommand::TurnLeft(_) | ActuatorCommand::TurnRight(_) => self.apply_speed(0.0),
            ActuatorCommand::Custom(name, args) => {
                if name == self.name {
                    let v = args.first().copied().unwrap_or(0.0);
                    self.apply_speed(v)
                } else {
                    Ok(())
                }
            }
        }
    }

    fn is_online(&self) -> bool {
        self.pwm.is_some()
    }
}