use anyhow::Result;
use std::path::PathBuf;
use std::io::{self, Write};
use std::collections::HashMap;

mod commands;
mod voice;
mod web;

use commands::parse_web_command;
use commands::WebCommand;
use synapse_core::brain::{Action, Brain};
use synapse_core::learning::{LearningEngine, Reward};
use synapse_core::state::AgentState;
use synapse_consciousness::adaptation::AdaptationEngine;
use synapse_consciousness::emotion::EmotionalState;
use synapse_consciousness::monitor::SelfMonitor;
use synapse_hal::virtual_actuator::VirtualActuator;
use synapse_hal::virtual_sensor::VirtualSensor;
use synapse_hal::sensor::SensorArray;
use synapse_hal::actuator::ActuatorArray;
use synapse_hal::percepcion::{Percepcion, PercepcionVirtual, Vista, Oido};
use synapse_memory::database::MemoryDatabase;
use synapse_memory::recall::RecallEngine;
use synapse_sim::world::World;
use synapse_sim::robot::VirtualRobot;
use synapse_sim::renderer::AsciiRenderer;

struct MemEntry {
    key: String,
    value: String,
    cat: String,
    source: String,
    tokens: Vec<String>,
}

struct Rule {
    text: String,
    tokens: Vec<String>,
}

struct SynapseMind {
    brain: Brain,
    learning: LearningEngine,
    emotion: EmotionalState,
    monitor: SelfMonitor,
    adaptation: AdaptationEngine,
    robot: VirtualRobot,
    world: World,
    sensors: SensorArray,
    actuators: ActuatorArray,
    percepcion: Box<dyn Percepcion>,
    vista: Vista,
    oido: Oido,
    last_vision_at: std::time::Instant,
    last_audio_at: std::time::Instant,
    last_motor_izq: f64,
    last_motor_der: f64,
    last_cabeza: f64,
    memory_db: MemoryDatabase,
    agent_state: AgentState,
    episode: u64,
    total_goals: u64,
    name: String,
    wake_word: String,
    knowledge: Vec<MemEntry>,
    token_index: HashMap<String, Vec<usize>>,
    rules: Vec<Rule>,
    paused: bool,
    last_message: String,
    last_wake_at: std::time::Instant,
}

/// Registra el "cuerpo" del robot: en una Raspberry Pi usa actuadores reales
/// (servos via PCA9685/I2C y motores DC via GPIO PWM); en cualquier otra
/// plataforma usa actuadores virtuales para no romper nada.
#[cfg(feature = "rpi")]
fn register_cuerpo(actuators: &mut ActuatorArray) {
    use synapse_hal::rpi_actuator::{is_raspberry_pi, PiMotor, PiServo};
    use synapse_hal::virtual_actuator::VirtualActuator;
    if is_raspberry_pi() {
        log::info!("cuerpo fisico detectado: servos PCA9685 + motores DC");
        actuators.add(Box::new(PiServo::new("servo_cabezal", 0, 90.0)));
        actuators.add(Box::new(PiMotor::new("motor_izq", 12, 20, 21)));
        actuators.add(Box::new(PiMotor::new("motor_der", 13, 26, 19)));
    } else {
        actuators.add(Box::new(VirtualActuator::new("motor_izq")));
        actuators.add(Box::new(VirtualActuator::new("motor_der")));
        actuators.add(Box::new(VirtualActuator::new("servo_cabezal")));
    }
}

#[cfg(not(feature = "rpi"))]
fn register_cuerpo(actuators: &mut ActuatorArray) {
    actuators.add(Box::new(VirtualActuator::new("motor_izq")));
    actuators.add(Box::new(VirtualActuator::new("motor_der")));
    actuators.add(Box::new(VirtualActuator::new("servo_cabezal")));
}

/// Registra los "sentidos" de susana. Sin hardware (VM) usa backends virtuales,
/// triviales en recursos; en una Raspberry Pi usa camara V4L2 y microfono ALSA.
#[cfg(feature = "percepcion_real")]
fn registrar_percepcion() -> Box<dyn Percepcion> {
    use synapse_hal::percepcion::PercepcionReal;
    use synapse_hal::rpi_actuator::is_raspberry_pi;
    if is_raspberry_pi() {
        log::info!("sentidos reales: camara V4L2 + microfono ALSA");
        Box::new(PercepcionReal::new())
    } else {
        Box::new(PercepcionVirtual::new())
    }
}

#[cfg(not(feature = "percepcion_real"))]
fn registrar_percepcion() -> Box<dyn Percepcion> {
    Box::new(PercepcionVirtual::new())
}

impl SynapseMind {
    fn new() -> Result<Self> {
        let memory_path = PathBuf::from("synapse_memory.db");
        let memory_db = if memory_path.exists() {
            MemoryDatabase::open(&memory_path)?
        } else {
            MemoryDatabase::open(&memory_path)?
        };

        let mut sensors = SensorArray::new();
        sensors.add(Box::new(VirtualSensor::new("ultrasonido_frontal", 0.5)));
        sensors.add(Box::new(VirtualSensor::new("ultrasonido_izq", 0.7)));
        sensors.add(Box::new(VirtualSensor::new("ultrasonido_der", 0.7)));
        sensors.add(Box::new(VirtualSensor::new("luz", 0.5)));
        sensors.add(Box::new(VirtualSensor::new("temperatura", 0.25)));

        let mut actuators = ActuatorArray::new();
        register_cuerpo(&mut actuators);

        let vista = Vista {
            fuente: "inicial".to_string(),
            ancho: 0,
            alto: 0,
            brillo: 0.5,
            movimiento: 0.0,
            texto: "Todavia no he mirado.".to_string(),
        };
        let oido = Oido {
            fuente: "inicial".to_string(),
            nivel: 0.0,
            voz: false,
            duracion_ms: 0,
            texto: "Todavia no he escuchado.".to_string(),
        };
        let hace_rato = std::time::Instant::now() - std::time::Duration::from_secs(3600);

        let mut monitor = SelfMonitor::new();
        monitor.register_sensor("ultrasonido_frontal");
        monitor.register_sensor("ultrasonido_izq");
        monitor.register_sensor("ultrasonido_der");
        monitor.register_sensor("luz");
        monitor.register_sensor("temperatura");

        let world = World::new_empty(12, 8);

        let name = match memory_db.get_config("nombre")? {
            Some(v) => v,
            None => "synapse".to_string(),
        };
        let wake_word = match memory_db.get_config("activacion")? {
            Some(v) => v,
            None => "synapse".to_string(),
        };

        // Migrar conocimiento legado (config "learned_phrases") a la tabla knowledge
        if let Some(legacy) = memory_db.get_config("learned_phrases")? {
            if !legacy.trim().is_empty() {
                for entry in legacy.split(";;") {
                    let mut parts = entry.splitn(2, '|');
                    let ph = parts.next().unwrap_or("").trim();
                    let me = parts.next().unwrap_or("").trim();
                    if !ph.is_empty() {
                        let norm = normalize_key(ph);
                        if !norm.is_empty() {
                            let _ = memory_db.store_knowledge(&norm, me, Some("hecho"), Some("chat"));
                        }
                    }
                }
            }
            let _ = memory_db.delete_config("learned_phrases");
        }

        // Re-etiquetar frases de material como categoria "material" con su documento
        if let Ok(Some(material)) = memory_db.get_config("learned_material") {
            let mut cur_doc = String::new();
            for line in material.lines() {
                let t = line.trim();
                if t.starts_with('[') && t.ends_with(']') && t.len() > 2 {
                    cur_doc = t[1..t.len() - 1].to_string();
                    continue;
                }
                if t.is_empty() {
                    continue;
                }
                for sentence in split_sentences(t) {
                    let words: Vec<&str> = sentence.split_whitespace().collect();
                    if words.len() < 4 {
                        continue;
                    }
                    let norm = normalize_key(&words[..4].join(" "));
                    let _ = memory_db.store_knowledge(
                        &norm,
                        sentence.trim(),
                        Some("material"),
                        Some(&cur_doc),
                    );
                }
            }
        }

        let knowledge = load_knowledge(&memory_db);
        let rules = load_rules(&memory_db);
        let mut token_index: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in knowledge.iter().enumerate() {
            if e.tokens.is_empty() {
                continue;
            }
            for t in &e.tokens {
                token_index.entry(t.clone()).or_default().push(i);
            }
        }

        Ok(Self {
            brain: Brain::new(8, 4),
            learning: LearningEngine::new(),
            emotion: EmotionalState::new(),
            monitor,
            adaptation: AdaptationEngine::new(),
            robot: VirtualRobot::new(),
            world,
            sensors,
            actuators,
            percepcion: registrar_percepcion(),
            vista,
            oido,
            last_vision_at: hace_rato,
            last_audio_at: hace_rato,
            last_motor_izq: 0.0,
            last_motor_der: 0.0,
            last_cabeza: 90.0,
            memory_db,
            agent_state: AgentState::new(),
            episode: 0,
            total_goals: 0,
            name,
            wake_word,
            knowledge,
            token_index,
            rules,
            paused: false,
            last_message: "Listo para aprender.".to_string(),
            last_wake_at: std::time::Instant::now(),
        })
    }

    fn run_step(&mut self) -> Result<bool> {
        self.run_step_impl(None)
    }

    fn run_step_impl(&mut self, forced: Option<Action>) -> Result<bool> {
        self.monitor.update_uptime();

        let readings = self.world.sensor_readings();
        for (name, _value) in &readings {
            if let Some((v, _)) = self.sensors.get_reading(name) {
                self.monitor.record_sensor_reading(name, v, 0.1);
            }
        }

        self.agent_state.position = (
            self.world.robot_pos.0 as f64,
            self.world.robot_pos.1 as f64,
        );

        let state_features = self.build_state_features(&readings);

        let mut action = match forced {
            Some(a) => a,
            None => self.brain.decide(&synapse_core::brain::State::new(state_features.clone())),
        };

        if self.action_forbidden(action).is_some() {
            action = synapse_core::brain::Action::Stop;
        }

        let actuator_cmd = match action {
            synapse_core::brain::Action::Forward => {
                synapse_hal::actuator::ActuatorCommand::MoveForward(0.5)
            }
            synapse_core::brain::Action::Backward => {
                synapse_hal::actuator::ActuatorCommand::MoveBackward(0.5)
            }
            synapse_core::brain::Action::TurnLeft => {
                synapse_hal::actuator::ActuatorCommand::TurnLeft(30.0)
            }
            synapse_core::brain::Action::TurnRight => {
                synapse_hal::actuator::ActuatorCommand::TurnRight(30.0)
            }
            synapse_core::brain::Action::Stop => synapse_hal::actuator::ActuatorCommand::Stop,
            synapse_core::brain::Action::Custom(_) => synapse_hal::actuator::ActuatorCommand::Stop,
        };
        self.actuators.execute_all(actuator_cmd);

        let success = self.robot.execute_action(action, &mut self.world);
        let reward_val = self.robot.compute_reward(&self.world, action);

        let reward = if reward_val > 50.0 {
            Reward::Positive(reward_val)
        } else if reward_val < -1.0 {
            Reward::Negative(reward_val.abs())
        } else {
            Reward::Zero
        };

        self.agent_state.position = (
            self.world.robot_pos.0 as f64,
            self.world.robot_pos.1 as f64,
        );

        let next_readings = self.world.sensor_readings();
        let next_features = self.build_state_features(&next_readings);

        self.brain.learn(
            &synapse_core::brain::State::new(state_features.clone()),
            action,
            reward_val,
            &synapse_core::brain::State::new(next_features.clone()),
        );

        self.learning.record_experience(
            synapse_core::brain::State::new(state_features.clone()),
            action,
            reward,
            synapse_core::brain::State::new(next_features.clone()),
            format!("Enviroment step at {:?}", self.robot.state.position),
        );

        if self.emotion.should_explore() {
            self.emotion.on_new_situation();
        }
        if success {
            self.emotion.on_success();
        } else {
            self.emotion.on_failure();
        }

        let adaptations = self.adaptation.evaluate(&self.emotion, &self.monitor);
        for adapt in &adaptations {
            log::debug!("Adaptacion activada: {}", adapt);
        }

        if self.robot.at_goal(&self.world) {
            self.total_goals += 1;
            let episode_reward = self.robot.reset_episode(&mut self.world);
            self.learning.end_episode();
            self.episode += 1;

            self.memory_db.store_experience(
                &state_features,
                &format!("{:?}", action),
                episode_reward,
                &next_features,
                &format!("Meta alcanzada en episodio {}", self.episode),
                &vec!["goal".to_string(), "success".to_string()],
            )?;

            self.memory_db.store_knowledge(
                &format!("episodio_{}", self.episode),
                &format!("Meta alcanzada. Reward: {:.1}", episode_reward),
                Some("episodios"),
                Some("brain"),
            )?;

            self.emotion.on_success();

            println!("{}", AsciiRenderer::render_stats(
                self.episode,
                self.total_goals,
                self.learning.total_reward,
                self.brain.q_table.exploration_rate,
                self.emotion.confidence,
            ));

            self.world.randomize();
            return Ok(true);
        }

        if self.robot.steps_in_episode >= self.robot.max_steps {
            self.robot.reset_episode(&mut self.world);
            self.learning.end_episode();
            self.episode += 1;

            self.memory_db.store_experience(
                &state_features,
                "timeout",
                -10.0,
                &next_features,
                &format!("Timeout en episodio {}", self.episode),
                &vec!["timeout".to_string()],
            )?;

            self.world.randomize();
            return Ok(true);
        }

        Ok(false)
    }

    fn build_state_features(&self, _readings: &[(String, f64)]) -> Vec<f64> {
        let mut features = vec![
            self.world.robot_pos.0 as f64 / self.world.width as f64,
            self.world.robot_pos.1 as f64 / self.world.height as f64,
            (self.world.goal_pos.0 as f64 - self.world.robot_pos.0 as f64)
                / self.world.width as f64,
            (self.world.goal_pos.1 as f64 - self.world.robot_pos.1 as f64)
                / self.world.height as f64,
        ];

        features.resize(4, 0.0);
        features
    }

    fn apply_action(&mut self, action: Action) {
        use synapse_hal::actuator::ActuatorCommand;
        let (izq, der, cabeza) = match action {
            Action::Forward => (1.0, 1.0, 90.0),
            Action::Backward => (-1.0, -1.0, 90.0),
            Action::TurnLeft => (-0.4, 1.0, 135.0),
            Action::TurnRight => (1.0, -0.4, 45.0),
            Action::Stop | Action::Custom(_) => (0.0, 0.0, 90.0),
        };
        self.actuators
            .execute_all(ActuatorCommand::Custom("motor_izq".to_string(), vec![izq]));
        self.actuators
            .execute_all(ActuatorCommand::Custom("motor_der".to_string(), vec![der]));
        self.actuators
            .execute_all(ActuatorCommand::Custom("servo_cabezal".to_string(), vec![cabeza]));
        self.last_motor_izq = izq;
        self.last_motor_der = der;
        self.last_cabeza = cabeza;
        self.robot.execute_action(action, &mut self.world);
    }

    fn memoria_baja(&self, umbral_mb: u64) -> bool {
        free_memory_mb().map(|libre| libre < umbral_mb).unwrap_or(false)
    }

    fn percepcion_hints(&mut self) -> (f64, f64) {
        let lecturas = self.sensors.read_all();
        let brillo = lecturas
            .iter()
            .find(|(n, _, _)| n == "luz")
            .map(|(_, v, _)| *v)
            .unwrap_or(0.5);
        let mov = lecturas
            .iter()
            .find(|(n, _, _)| n == "ultrasonido_frontal")
            .map(|(_, v, _)| *v)
            .unwrap_or(0.0);
        (brillo.clamp(0.0, 1.0), mov.clamp(0.0, 1.0))
    }

    fn process_message(&mut self, msg: &str) -> String {
        let latch_seconds: u64 = 10;
        let grace_active = self.last_wake_at.elapsed().as_secs() < latch_seconds;
        let command = parse_web_command(msg, &self.wake_word, grace_active);

        let wake_lower = self.wake_word.trim().to_lowercase();
        let msg_lower = msg.trim().to_lowercase();
        let used_wake = !wake_lower.is_empty() && msg_lower.contains(&wake_lower);
        if used_wake {
            self.last_wake_at = std::time::Instant::now();
        }

        if let Some(blocked) = self.rule_blocks_command(msg, &command) {
            return format!(
                "No puedo hacer eso: tengo una regla absoluta que debo cumplir siempre: '{}'.",
                blocked
            );
        }

        match command {
            WebCommand::Ignore => {
                return "Estoy esperando mi palabra de activacion.".to_string();
            }
            WebCommand::Greeting => {
                return format!("Hola! Soy {}. En que te ayudo?", self.name);
            }
            WebCommand::Action(a) => {
                self.apply_action(a);
                return "Orden recibida.".to_string();
            }
            WebCommand::Pause => {
                self.paused = true;
                return "Pausado.".to_string();
            }
            WebCommand::Resume => {
                self.paused = false;
                return "Reanudado.".to_string();
            }
            WebCommand::Train(n) => {
                let _r = self.run_training(n);
                return format!("Entrenamiento de {} episodios terminado.", n);
            }
            WebCommand::SetName(v) => {
                self.set_name(v.as_str());
                return format!("Mi nombre ahora es {}", self.name);
            }
            WebCommand::SetWakeWord(v) => {
                self.set_wake_word(v.as_str());
                return format!("Palabra de activacion cambiada a '{}'", self.wake_word);
            }
            WebCommand::Learn(phrase, meaning) => {
                let added = self.learn_phrase(&phrase, &meaning);
                if added {
                    format!("He aprendido que {}", meaning)
                } else {
                    "Ya se eso.".to_string()
                }
            }
            WebCommand::DoStatus => {
                let vista = format!("brillo {:.0}%", self.vista.brillo * 100.0);
                let oido = format!("nivel {:.0}%", self.oido.nivel * 100.0);
                return format!(
                    "Episodio {}, metas {}, posicion {:?}, emocion {} | veo: {} | oigo: {}",
                    self.episode, self.total_goals, self.robot.state.position,
                    self.emotion.dominant_emotion(), vista, oido,
                );
            }
            WebCommand::DoDiagnostic => {
                return format!(
                    "Salud {:.0}%, errores {}, sensores {}",
                    self.monitor.overall_health_score() * 100.0,
                    self.monitor.health.errors_count,
                    self.monitor.health.sensor_status.len(),
                );
            }
            WebCommand::Help => {
                return format!(
                    "Puedes decirme: adelante, atras, izquierda, derecha, stop, pausa, reanudar, estado, diagnostico, entrenar <n>, nombre <x>, activacion <x>"
                );
            }
            WebCommand::DoKnowledge => {
                let facts: Vec<&MemEntry> = self.knowledge.iter().filter(|e| e.cat == "hecho").collect();
                let mats: Vec<&MemEntry> = self.knowledge.iter().filter(|e| e.cat == "material").collect();
                if facts.is_empty() && mats.is_empty() {
                    return "Todavia no he aprendido nada. Dime: aprende que X es Y, o sube un PDF.".to_string();
                }
                let mut out = String::new();
                if !facts.is_empty() {
                    out.push_str(&format!("Se {} cosas: ", facts.len()));
                    for (i, e) in facts.iter().take(12).enumerate() {
                        let k = e.key.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
                        out.push_str(&format!("({}) {}; ", i + 1, k));
                    }
                }
                if !mats.is_empty() {
                    let mut docs: Vec<String> = Vec::new();
                    for e in &mats {
                        if !e.source.is_empty() && !docs.contains(&e.source) {
                            docs.push(e.source.clone());
                        }
                    }
                    out.push_str(&format!(
                        " Y tengo material de {} documento(s): {}.",
                        docs.len(),
                        if docs.is_empty() { "sin nombre".to_string() } else { docs.join(", ") }
                    ));
                }
                out
            }
            WebCommand::SetBrain(v) => {
                let _ = self.memory_db.set_config("llm", if v { "1" } else { "0" });
                return if v {
                    "Cerebro activado: ahora pienso con mi modelo de lenguaje.".to_string()
                } else {
                    "Cerebro desactivado: vuelvo a responder solo con lo que he aprendido.".to_string()
                };
            }
            WebCommand::Think(text) => {
                return self.ask_llm(&text).unwrap_or_else(|| {
                    "Todavia no se eso y prefiero no inventar. Puedes ensenarme con 'aprende que X es Y' o subir un PDF."
                        .to_string()
                });
            }
            WebCommand::MoveServo(name, angle) => {
                use synapse_hal::actuator::ActuatorCommand;
                let mut found = false;
                let mut moved = false;
                for a in self.actuators.actuators.iter_mut() {
                    let nm = a.name().to_string();
                    if nm == name {
                        found = true;
                        match a.execute(ActuatorCommand::Custom(name.clone(), vec![angle])) {
                            Ok(()) => moved = true,
                            Err(e) => log::warn!("actuador '{}': {}", nm, e),
                        }
                        match nm.as_str() {
                            "motor_izq" => self.last_motor_izq = angle,
                            "motor_der" => self.last_motor_der = angle,
                            "servo_cabezal" => self.last_cabeza = angle,
                            _ => {}
                        }
                    }
                }
                if !found {
                    return format!("No tengo ningún servo llamado {}.", name);
                }
                if !moved {
                    return format!("El servo {} no está conectado en este momento.", name);
                }
                return format!("Moviendo {} a {} grados.", name, angle);
            }
            WebCommand::QueVes => {
                if self.last_vision_at.elapsed() < std::time::Duration::from_secs(5) {
                    return self.vista.texto.clone();
                }
                if self.memoria_baja(300) {
                    return "Prefiero no encender la camara ahora para no agotar la memoria de la VM."
                        .to_string();
                }
                let (hint_b, hint_m) = self.percepcion_hints();
                self.vista = self.percepcion.ver(hint_b, hint_m);
                self.last_vision_at = std::time::Instant::now();
                return self.vista.texto.clone();
            }
            WebCommand::Escucha => {
                if self.last_audio_at.elapsed() < std::time::Duration::from_secs(3) {
                    return self.oido.texto.clone();
                }
                if self.memoria_baja(300) {
                    return "Prefiero no activar el microfono ahora para no agotar la memoria de la VM."
                        .to_string();
                }
                self.oido = self.percepcion.oir();
                self.last_audio_at = std::time::Instant::now();
                return self.oido.texto.clone();
            }
            WebCommand::Unknown => {
                let query = query_without_wake(msg, &self.wake_word);
                match self.retrieve(&query) {
                    Some(e) if e.cat == "material" => {
                        let src = if e.source.is_empty() {
                            "el PDF".to_string()
                        } else {
                            e.source.clone()
                        };
                        format!("Del documento {}, recuerdo: {}", src, e.value)
                    }
                    Some(e) => natural_answer(&normalize_key(&query), &e.key, &e.value),
                    None => {
                        if self.llm_enabled() {
                            if let Some(ai) = self.ask_llm(&query) {
                                return ai;
                            }
                        }
                        "Todavia no se eso y prefiero no inventar. Puedes ensenarme con 'aprende que X es Y' o subir un PDF."
                            .to_string()
                    }
                }
            }
        }
    }

    fn llm_enabled(&self) -> bool {
        match self.memory_db.get_config("llm") {
            Ok(Some(v)) => {
                let v = v.trim().to_lowercase();
                v != "0" && v != "off" && v != "no" && v != "false"
            }
            _ => true,
        }
    }

    fn ask_llm(&self, prompt: &str) -> Option<String> {
        use std::io::Read;
        use std::io::Write;
        use std::net::TcpStream;

        const MIN_AVAILABLE_MB: u64 = 400;
        if let Some(avail) = free_memory_mb() {
            if avail < MIN_AVAILABLE_MB {
                log::debug!("memoria baja ({} MB), cerebro degradado a respuestas aprendidas", avail);
                return None;
            }
        }

        let clipped: String = prompt.chars().take(600).collect();
        let body = serde_json::json!({
            "model": "qwen",
            "messages": [
                {
                    "role": "system",
                    "content": "Eres susana, una asistente amigable y honesta que responde en espanol de forma breve y natural. Si no sabes algo, dilo sin inventar."
                },
                { "role": "user", "content": clipped }
            ],
            "max_tokens": 200,
            "temperature": 0.7
        });
        let payload = serde_json::to_string(&body).ok()?;
        let mut sock = TcpStream::connect(("127.0.0.1", 9091)).ok()?;
        let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(90)));
        let req = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:9091\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        sock.write_all(req.as_bytes()).ok()?;
        let mut raw = Vec::new();
        sock.read_to_end(&mut raw).ok()?;
        let text = String::from_utf8_lossy(&raw);
        let body_str = match text.find("\r\n\r\n") {
            Some(i) => &text[i + 4..],
            None => return None,
        };
        let v: serde_json::Value = serde_json::from_str(body_str).ok()?;
        let content = v["choices"][0]["message"]["content"].as_str()?.trim();
        if content.is_empty() {
            return None;
        }
        let cleaned = content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        Some(cleaned)
    }

    fn set_name(&mut self, value: &str) {
        self.name = value.to_string();
        let _ = self.memory_db.set_config("nombre", value);
    }

    fn set_wake_word(&mut self, value: &str) {
        let clean = value.trim().split_whitespace().next().unwrap_or_default();
        self.wake_word = clean.to_string();
        let _ = self.memory_db.set_config("activacion", clean);
    }

    fn learn_phrase(&mut self, phrase: &str, meaning: &str) -> bool {
        let clean_p: String = phrase
            .trim()
            .replace('|', " ")
            .replace(';', " ")
            .chars()
            .map(|c| if c == '\n' { ' ' } else { c })
            .collect();
        let clean_m: String = meaning
            .trim()
            .replace('|', " ")
            .replace(';', " ")
            .chars()
            .map(|c| if c == '\n' { ' ' } else { c })
            .collect();
        if clean_p.is_empty() || clean_m.is_empty() {
            return false;
        }
        self.add_knowledge(&clean_p, &clean_m, "hecho", "chat")
    }

    fn add_knowledge(&mut self, key: &str, value: &str, cat: &str, source: &str) -> bool {
        let norm = normalize_key(key);
        if norm.is_empty() || value.trim().is_empty() {
            return false;
        }
        let key_tokens = tokenize(key);
        if self
            .knowledge
            .iter()
            .any(|e| e.tokens == key_tokens || e.key.eq_ignore_ascii_case(key))
        {
            return false;
        }
        let _ = self
            .memory_db
            .store_knowledge(&norm, value.trim(), Some(cat), Some(source));
        let mut entry = MemEntry {
            key: key.trim().to_string(),
            value: value.trim().to_string(),
            cat: cat.to_string(),
            source: source.to_string(),
            tokens: tokenize(key),
        };
        let idx = self.knowledge.len();
        if entry.tokens.is_empty() {
            entry.tokens = tokenize(&norm);
        }
        self.knowledge.push(entry);
        for t in self.knowledge[idx].tokens.clone() {
            self.token_index
                .entry(t)
                .or_insert_with(Vec::new)
                .push(idx);
        }
        true
    }

    fn retrieve(&self, query: &str) -> Option<&MemEntry> {
        let q_tokens = tokenize(query);
        if q_tokens.is_empty() {
            return None;
        }
        let mut best: Option<(i64, usize)> = None;
        for (i, e) in self.knowledge.iter().enumerate() {
            let tokens_lower: Vec<String> = e.tokens.iter().map(|t| t.to_lowercase()).collect();
            let mut match_count = 0usize;
            let mut seq_hint = false;
            for qt in &q_tokens {
                let ql = qt.to_lowercase();
                if tokens_lower.contains(&ql) {
                    match_count += 1;
                }
            }
            for w in tokens_lower.windows(2) {
                let ql: Vec<String> = q_tokens.iter().map(|t| t.to_lowercase()).collect();
                if ql.windows(2).any(|p| p == w) {
                    seq_hint = true;
                }
            }
            let seq_bonus: i64 = if seq_hint { 8 } else { 0 };
            let need = if q_tokens.len() == 1 { 1 } else { 2 };
            if match_count < need {
                continue;
            }
            let cat_bias: i64 = if e.cat == "material" { 2 } else { 0 };
            let score = (match_count as i64) * 10 + cat_bias + seq_bonus;
            if score > 0 && (best.is_none() || score > best.unwrap().0) {
                best = Some((score, i));
            }
        }
        best.map(|(_, i)| &self.knowledge[i])
    }

    fn add_rule(&mut self, text: &str) -> bool {
        let clean: String = text
            .trim()
            .replace('|', " ")
            .replace(';', " ")
            .chars()
            .map(|c| if c == '\n' { ' ' } else { c })
            .collect();
        if clean.trim().is_empty() {
            return false;
        }
        let norm = normalize_key(&clean);
        if norm.is_empty() {
            return false;
        }
        let tokens = tokenize(&clean);
        if self
            .rules
            .iter()
            .any(|r| r.tokens == tokens || r.text.eq_ignore_ascii_case(clean.trim()))
        {
            return false;
        }
        let _ = self
            .memory_db
            .store_knowledge(&norm, clean.trim(), Some("regla"), Some("regla"));
        self.rules.push(Rule {
            text: clean.trim().to_string(),
            tokens,
        });
        true
    }

    fn remove_rule(&mut self, key: &str) -> bool {
        let _ = self.memory_db.delete_knowledge(key);
        let norm = normalize_key(key);
        let before = self.rules.len();
        self.rules
            .retain(|r| normalize_key(&r.text) != norm && r.text != key);
        before != self.rules.len()
    }

    fn shared_token(&self, text: &str) -> Option<String> {
        let q_tokens = tokenize(text);
        if q_tokens.is_empty() {
            return None;
        }
        for r in &self.rules {
            if r.tokens.iter().any(|rt| q_tokens.iter().any(|qt| qt == rt)) {
                return Some(r.text.clone());
            }
        }
        None
    }

    fn action_forbidden(&self, action: Action) -> Option<String> {
        let all = vec![
            Action::Forward,
            Action::Backward,
            Action::TurnLeft,
            Action::TurnRight,
        ];
        for r in &self.rules {
            let t = r.text.to_lowercase();
            let has_left = ["izquierda", "izq", "left", "izquierdo"]
                .iter()
                .any(|k| t.contains(k));
            let has_right = ["derecha", "der", "right", "derecho"]
                .iter()
                .any(|k| t.contains(k));
            let has_fwd = ["adelante", "avanza", "avanzar", "forward", "recto", "al frente"]
                .iter()
                .any(|k| t.contains(k));
            let has_back = ["atras", "reversa", "backward", "retrocede", "hacia atras"]
                .iter()
                .any(|k| t.contains(k));
            let has_move = ["moverse", "moverte", "mover", "movimiento", "caminar", "desplazar", "desplazarte", "desplazamiento", "andar", "correr", "corre"]
                .iter()
                .any(|k| t.contains(k));
            let has_turn = ["girar", "gira", "torcer", "torce", "vira", "rota", "rotacion"]
                .iter()
                .any(|k| t.contains(k));

            let mut blocked: Vec<Action> = Vec::new();
            if has_left {
                blocked.push(Action::TurnLeft);
            }
            if has_right {
                blocked.push(Action::TurnRight);
            }
            if has_fwd {
                blocked.push(Action::Forward);
            }
            if has_back {
                blocked.push(Action::Backward);
            }
            if blocked.is_empty() && has_move {
                blocked = all.clone();
            }
            if blocked.is_empty() && has_turn {
                blocked = vec![Action::TurnLeft, Action::TurnRight];
            }
            if blocked.contains(&action) {
                return Some(r.text.clone());
            }
        }
        None
    }

    fn rule_blocks_command(&self, text: &str, cmd: &WebCommand) -> Option<String> {
        match cmd {
            WebCommand::Action(a) => {
                if let Some(r) = self.action_forbidden(*a) {
                    return Some(r);
                }
                if let Some(r) = self.shared_token(text) {
                    return Some(r);
                }
                None
            }
            WebCommand::Train(_) => {
                for r in &self.rules {
                    let t = r.text.to_lowercase();
                    if ["entrenar", "entrena", "train", "aprendizaje", "adestrar", "educacion"]
                        .iter()
                        .any(|k| t.contains(k))
                    {
                        return Some(r.text.clone());
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn learn_material(&mut self, name: &str, text: &str) -> usize {
        let mut material = self.memory_db.get_config("learned_material").unwrap_or_default().unwrap_or_default();
        material.push_str(&format!("\n[{}]\n{}\n", name, text));
        if material.len() > 200_000 {
            material = material.chars().take(200_000).collect();
        }
        let _ = self.memory_db.set_config("learned_material", &material);

        let mut added = 0;
        for sentence in split_sentences(text) {
            if added >= 30 {
                break;
            }
            let words: Vec<&str> = sentence.split_whitespace().collect();
            if words.len() < 4 {
                continue;
            }
            let key: String = words[..4].join(" ");
            let phrase = if key.len() > 60 {
                key.chars().take(60).collect()
            } else {
                key
            };
            if self.add_knowledge(&phrase, &sentence, "material", name) {
                added += 1;
            }
        }
        added
    }

    fn status_json(&self) -> String {
        let mut sensor_map = String::new();
        let sensors = self.world.sensor_readings();
        let mut first = true;
        sensor_map.push('{');
        for (name, value) in &sensors {
            if !first {
                sensor_map.push(',');
            }
            first = false;
            sensor_map.push_str(&format!("\"{}\":{}", name, value));
        }
        sensor_map.push('}');

        let emoji = match self.emotion.dominant_emotion() {
            "confianza" => "^_^",
            "curiosidad" => "(?_?)",
            "estres" => ">_<",
            "satisfaccion" => ":-)",
            "cautela" => "(_o_)",
            _ => "-_-",
        };

        format!(
            "{{\"nombre\":\"{}\",\"activacion\":\"{}\",\"emocion\":\"{}\",\"emoji\":\"{}\",\"confianza\":{},\"estres\":{},\"energia\":{},\"curiosidad\":{},\"explotacion\":{},\"explotar\":{},\"episodio\":{},\"total_metas\":{},\"posicion\":\"({}, {})\",\"estados\":{},\"experiencias\":{},\"adaptaciones\":{},\"recompensa\":{},\"mundo\":\"{}\",\"sensores\":{},\"vista_brillo\":{},\"vista_movimiento\":{},\"vista_texto\":\"{}\",\"oido_nivel\":{},\"oido_voz\":{},\"oido_texto\":\"{}\",\"motor_izq\":{},\"motor_der\":{},\"servo_cabezal\":{},\"mensaje\":\"{}\"}}",
            self.name,
            self.wake_word,
            self.emotion.dominant_emotion(),
            emoji,
            self.emotion.confidence,
            self.emotion.stress,
            self.emotion.energy_level,
            self.emotion.curiosity,
            self.brain.q_table.exploration_rate,
            self.brain.q_table.exploration_rate,
            self.episode,
            self.total_goals,
            self.world.robot_pos.0,
            self.world.robot_pos.1,
            self.brain.q_table.num_states(),
            self.learning.experiences.len(),
            self.adaptation.total_adaptations,
            self.learning.total_reward,
            Self::json_escape_multi(&AsciiRenderer::render_world(&self.world)),
            sensor_map,
            self.vista.brillo,
            self.vista.movimiento,
            Self::json_escape_multi(&self.vista.texto),
            self.oido.nivel,
            self.oido.voz,
            Self::json_escape_multi(&self.oido.texto),
            self.last_motor_izq,
            self.last_motor_der,
            self.last_cabeza,
            Self::json_escape_multi(&self.last_message),
        )
    }

    fn json_escape_multi(s: &str) -> String {
        let mut out = String::new();
        for ch in s.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                _ => out.push(ch),
            }
        }
        out
    }

    fn run_training(&mut self, max_episodes: u64) -> Result<()> {
        println!("=== SynapseAI - Entrenamiento ===");
        println!("Episodios maximos: {}", max_episodes);
        println!();

        while self.episode < max_episodes {
            let completed = self.run_step()?;

            if completed && self.episode % 50 == 0 {
                println!("\n--- Checkpoint episodio {} ---", self.episode);
                println!("{}", self.emotion.mood_summary());
                println!("Exploracion: {:.1}%", self.brain.q_table.exploration_rate * 100.0);
                println!("Estados aprendidos: {}", self.brain.q_table.num_states());
                println!("Experiencias: {}", self.learning.experiences.len());
                println!("Adaptaciones: {}", self.adaptation.total_adaptations);
                println!();
            }
        }

        println!("\n=== Entrenamiento completado ===");
        println!("Episodios: {}", self.episode);
        println!("Metas alcanzadas: {}", self.total_goals);
        println!("Reward total: {:.1}", self.learning.total_reward);
        println!("Estados en Q-Table: {}", self.brain.q_table.num_states());

        let recall = RecallEngine::new(&self.memory_db);
        println!("\n{}", recall.recent_learning_summary()?);
        println!("{}", self.adaptation.report());
        println!("{}", self.monitor.diagnostic_report());

        Ok(())
    }

    fn run_interactive(&mut self) -> Result<()> {
        println!("=== SynapseAI - Modo Interactivo ===");
        println!("Comandos: step, train <n>, status, memory, brain, world, emotion, adapt, diagnostic, quit");
        println!();

        loop {
            print!("synapse> ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim();

            match input {
                "quit" | "exit" | "q" => {
                    println!("Guardando memoria...");
                    break;
                }
                "step" | "s" => {
                    let completed = self.run_step()?;
                    println!("{}", AsciiRenderer::render_world(&self.world));
                    println!("{}", AsciiRenderer::render_thought(
                        &format!("{:?}", self.robot.state.position),
                        self.emotion.dominant_emotion(),
                        "Paso ejecutado",
                    ));
                    if completed {
                        println!("*** Episodio {} completado ***", self.episode);
                    }
                }
                "status" => {
                    println!("{}", AsciiRenderer::render_stats(
                        self.episode,
                        self.total_goals,
                        self.learning.total_reward,
                        self.brain.q_table.exploration_rate,
                        self.emotion.confidence,
                    ));
                    println!("{}", self.robot.stats());
                }
                "world" | "w" => {
                    println!("{}", AsciiRenderer::render_world(&self.world));
                }
                "emotion" | "e" => {
                    println!("{}", self.emotion.mood_summary());
                    println!("Dominante: {}", self.emotion.dominant_emotion());
                    println!("Explorar: {} | Cauto: {}", self.emotion.should_explore(), self.emotion.should_be_cautious());
                }
                "brain" | "b" => {
                    let snap = self.brain.snapshot();
                    println!("=== Cerebro ===");
                    println!("Estados: {}", snap.num_states);
                    println!("Exploracion: {:.1}%", snap.exploration_rate * 100.0);
                    println!("Actualizaciones: {}", snap.total_updates);
                    println!("Tamanio Q-Table: {}", snap.q_table_size);
                }
                "memory" | "m" => {
                    let recall = RecallEngine::new(&self.memory_db);
                    println!("{}", recall.recent_learning_summary()?);
                }
                "adapt" | "a" => {
                    println!("{}", self.adaptation.report());
                }
                "diagnostic" | "d" => {
                    println!("{}", self.monitor.diagnostic_report());
                }
                cmd if cmd.starts_with("train") => {
                    let parts: Vec<&str> = cmd.split_whitespace().collect();
                    let n = if parts.len() > 1 {
                        parts[1].parse::<u64>().unwrap_or(100)
                    } else {
                        100
                    };
                    self.run_training(n)?;
                }
                "" => continue,
                _ => {
                    println!("Comando desconocido: '{}'. Usa: step, train <n>, status, memory, brain, world, emotion, adapt, diagnostic, quit", input);
                }
            }
        }

        Ok(())
    }

    fn run_web(&mut self, port: u16) -> Result<()> {
        println!("=== SynapseAI - Modo Web ===");
        println!("Nombre: {} | Palabra de activacion: {}", self.name, self.wake_word);
        println!("Presiona Ctrl+C para detener.");

        let hub = web::make_hub(&self.name, &self.wake_word);
        let hub_server = hub.clone();

        std::thread::spawn(move || {
            web::run_api_server(port, hub_server);
        });

        let voice_holder: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<voice::VoiceModel>>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let voice_load = voice_holder.clone();
        let hub_for_voice = hub.clone();
        std::thread::spawn(move || {
            match voice::build_voice() {
                Ok(model) => {
                    let voices = model.voices.clone();
                    {
                        let mut g = hub_for_voice.lock().unwrap();
                        g.voices = voices;
                    }
                    *voice_load.lock().unwrap() = Some(std::sync::Arc::new(model));
                }
                Err(e) => {
                    eprintln!("No se pudo cargar el modelo de voz: {}", e);
                }
            }
        });

        loop {
            self.refresh_status(&hub);

            let mut processed = 0;
            loop {
                if processed >= 10 {
                    break;
                }
                let next = hub.lock().unwrap().commands.pop_front();
                if next.is_none() {
                    break;
                }
                processed += 1;
                let msg = next.unwrap();
                let response = self.process_message(msg.as_str());
                self.last_message = response.clone();
                self.speak_response(&hub, &voice_holder, &response);
            }
            let _ = processed;

            loop {
                let next = hub.lock().unwrap().speak_requests.pop_front();
                if next.is_none() {
                    break;
                }
                self.speak_response(&hub, &voice_holder, &next.unwrap());
            }

            loop {
                let next = hub.lock().unwrap().learn_requests.pop_front();
                if next.is_none() {
                    break;
                }
                let (phrase, meaning) = next.unwrap();
                self.learn_phrase(&phrase, &meaning);
                let response = format!("He aprendido que {} es {}", phrase, meaning);
                self.last_message = response.clone();
                self.speak_response(&hub, &voice_holder, &response);
            }

            loop {
                let next = hub.lock().unwrap().pdf_requests.pop_front();
                if next.is_none() {
                    break;
                }
                let (name, text) = next.unwrap();
                let added = self.learn_material(&name, &text);
                let response = format!("He leido el material {} y aprendi {} frases.", name, added);
                self.last_message = response.clone();
                self.speak_response(&hub, &voice_holder, &response);
            }

            loop {
                let next = hub.lock().unwrap().rules_requests.pop_front();
                if next.is_none() {
                    break;
                }
                let (accion, payload) = next.unwrap();
                let response = if accion == "del" || accion == "quitar" || accion == "eliminar" {
                    if self.remove_rule(&payload) {
                        "Regla eliminada.".to_string()
                    } else {
                        "No encontre esa regla.".to_string()
                    }
                } else if self.add_rule(&payload) {
                    format!("He registrado la regla absoluta: {}", payload)
                } else {
                    "Esa regla ya existe o esta vacia.".to_string()
                };
                self.last_message = response.clone();
                self.speak_response(&hub, &voice_holder, &response);
            }

            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    fn speak_response(
        &self,
        hub: &web::WebHub,
        holder: &std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<voice::VoiceModel>>>>,
        text: &str,
    ) {
        if text.is_empty() {
            return;
        }
        let (enabled, vname) = {
            let g = hub.lock().unwrap();
            (g.voice_enabled, g.voice_name.clone())
        };
        if !enabled {
            return;
        }
        let model = holder.lock().unwrap().clone();
        if model.is_none() {
            return;
        }
        let model = model.unwrap();
        let spoken = text.to_string();
        std::thread::spawn(move || {
            let _ = voice::speak(&model, &vname, &spoken, 1.0);
        });
    }

    fn refresh_status(&mut self, hub: &web::WebHub) {
        self.monitor.update_uptime();

        let readings = self.world.sensor_readings();
        for (name, _value) in &readings {
            if let Some((v, _)) = self.sensors.get_reading(name) {
                self.monitor.record_sensor_reading(name, v, 0.1);
            }
        }

        let status = self.status_json();
        let mut guard = hub.lock().unwrap();
        guard.name = self.name.to_string();
        guard.wake_word = self.wake_word.to_string();
        guard.status = status;
        guard.learned_summary = self
            .knowledge
            .iter()
            .take(30)
            .map(|e| format!("{} = {}", e.key, e.value))
            .collect::<Vec<_>>()
            .join("; ");
        guard.rules = self
            .rules
            .iter()
            .map(|r| r.text.clone())
            .collect::<Vec<_>>();
        if !guard.learned_summary.is_empty() {
            guard
                .learned_summary
                .push_str(&format!(" | Reglas: {}", self.rules.len()));
        }
    }
}

fn normalize_key(key: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for c in key.chars() {
        let lc = c.to_lowercase().next().unwrap_or(c);
        if lc.is_alphanumeric() {
            out.push(lc);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

fn normalize_fold(key: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for c in key.chars() {
        let lc = c.to_lowercase().next().unwrap_or(c);
        let folded = match lc {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' => 'u',
            'ü' => 'u',
            'ñ' => 'n',
            _ => lc,
        };
        if folded.is_ascii_alphanumeric() {
            out.push(folded);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

fn tokenize(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "que", "los", "las", "del", "una", "un", "con", "por", "para", "pero", "mas", "mas",
        "muy", "sus", "cuando", "donde", "cual", "quien", "como", "desde", "hacia", "aunque",
        "entre", "eso", "esto", "era", "todo", "nada", "mucho", "uno", "ese", "son", "esta",
        "este", "esa", "el", "la", "de", "es", "me", "se", "y", "a", "o", "e", "su", "en", "al",
        "fue", "tiempo", "dia", "dia",
    ];
    normalize_fold(text)
        .split_whitespace()
        .map(|t| t.to_string())
        .filter(|t| t.len() >= 3 && !STOP.contains(&t.as_str()))
        .collect()
}

fn load_knowledge(db: &MemoryDatabase) -> Vec<MemEntry> {
    let rows = match db.all_knowledge() {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.into_iter()
        .map(|row| {
            let tokens = tokenize(&row.key);
            MemEntry {
                key: row.key,
                value: row.value,
                cat: row.category.unwrap_or_default(),
                source: row.source.unwrap_or_default(),
                tokens,
            }
        })
        .filter(|e| !e.tokens.is_empty())
        .collect()
}

fn load_rules(db: &MemoryDatabase) -> Vec<Rule> {
    let rows = match db.get_rules() {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.into_iter()
        .map(|(_key, text)| Rule {
            tokens: tokenize(&text),
            text,
        })
        .collect()
}

fn query_without_wake(msg: &str, wake: &str) -> String {
    let w = wake.trim();
    if w.is_empty() {
        return msg.trim().to_string();
    }
    msg.split_whitespace()
        .filter(|t| !t.eq_ignore_ascii_case(w))
        .collect::<Vec<_>>()
        .join(" ")
}

fn free_memory_mb() -> Option<u64> {
    if std::env::consts::OS != "linux" {
        return None;
    }
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in info.lines() {
        if let Some(rest) = line.strip_prefix("MemAvailable:") {
            let kb: u64 = rest.split_whitespace().next()?.trim().parse().ok()?;
            return Some(kb / 1024);
        }
    }
    None
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn topic_after(query: &str, marker: &str) -> Option<String> {
    let idx = query.find(marker)?;
    let rest = query[idx + marker.len()..].trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

fn topic_subject(query_folded: &str, key_orig: &str, key_folded: &str, marker: &str) -> Option<String> {
    if let Some(idx) = key_folded.find(marker) {
        let nchars = key_folded[..idx].chars().count() + marker.chars().count();
        let mut iter = key_orig.chars();
        let mut skipped = 0usize;
        let mut rest = String::new();
        for c in iter.by_ref() {
            if skipped >= nchars {
                rest.push(c);
            } else {
                skipped += 1;
            }
        }
        let t = rest.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    topic_after(query_folded, marker)
}

fn fmt_ubicacion(subject: &str, verb: &str, value: &str) -> String {
    let v = value.trim();
    if v.starts_with("en ") || v.starts_with("al ") || v.starts_with("a la ") {
        format!("{} {} {}.", capitalize_first(subject), verb, v)
    } else {
        format!("{} {} en {}.", capitalize_first(subject), verb, v)
    }
}

fn natural_answer(query: &str, key: &str, value: &str) -> String {
    let q = normalize_fold(query);
    let key_orig = normalize_key(key);
    let key_folded = normalize_fold(&key_orig);
    let v = value.trim();

    if q.contains("al sur de") {
        if let Some(t) = topic_subject(&q, &key_orig, &key_folded, "al sur de") {
            return format!("El país al sur de {} es {}.", t, v);
        }
    }

    let ubic_markers = [
        ("en que estado de mexico esta", "está"),
        ("en que costa de colombia esta", "está"),
        ("en que pais nacieron", "nacieron"),
        ("en que pais estan", "están"),
        ("en que continente esta", "está"),
        ("en que pais esta", "está"),
        ("en que ciudad esta", "está"),
        ("en que cordillera esta", "está"),
    ];
    for (m, verb) in ubic_markers {
        if q.contains(m) {
            if let Some(t) = topic_subject(&q, &key_orig, &key_folded, m) {
                return fmt_ubicacion(&t, verb, v);
            }
        }
    }
    if q.contains("donde estan") {
        if let Some(t) = topic_subject(&q, &key_orig, &key_folded, "donde estan") {
            return fmt_ubicacion(&t, "están", v);
        }
    }
    if q.contains("donde esta ubicada") {
        if let Some(t) = topic_subject(&q, &key_orig, &key_folded, "donde esta ubicada") {
            return fmt_ubicacion(&t, "está", v);
        }
    }
    if q.contains("donde esta") {
        if let Some(t) = topic_subject(&q, &key_orig, &key_folded, "donde esta") {
            return fmt_ubicacion(&t, "está", v);
        }
    }
    if q.contains("capital de") {
        if let Some(p) = topic_subject(&q, &key_orig, &key_folded, "capital de") {
            return format!("La capital de {} es {}.", p, v);
        }
    }
    if q.contains("ciudad mas poblada") {
        if let Some(p) = topic_subject(&q, &key_orig, &key_folded, "mas poblada") {
            return format!("La ciudad más poblada de {} es {}.", p, v);
        }
    }
    for (m, verbo) in [
        ("quien escribio", "escribió"),
        ("quien desarrollo", "desarrolló"),
        ("quien invento", "inventó"),
        ("quien pinto", "pintó"),
    ] {
        if q.contains(m) {
            if let Some(t) = topic_subject(&q, &key_orig, &key_folded, m) {
                return format!("{} {} {}.", capitalize_first(v), verbo, t);
            }
        }
    }
    if q.contains("quien fue") || q.contains("quien es") {
        let m = if q.contains("quien fue") {
            "quien fue"
        } else {
            "quien es"
        };
        if let Some(t) = topic_subject(&q, &key_orig, &key_folded, m) {
            return format!("{} fue {}.", capitalize_first(&t), v);
        }
    }
    if q.contains("cuando se celebra") {
        if let Some(t) = topic_subject(&q, &key_orig, &key_folded, "cuando se celebra") {
            return format!("{} se celebra {}.", capitalize_first(&t), v);
        }
    }
    if q.contains("cuando se usa") {
        if let Some(t) = topic_subject(&q, &key_orig, &key_folded, "cuando se usa") {
            return format!("{} se usa {}.", capitalize_first(&t), v);
        }
    }
    if q.contains("en que ano") {
        if let Some(t) = topic_subject(&q, &key_orig, &key_folded, "en que ano") {
            return format!("{} fue en {}.", capitalize_first(&t), v);
        }
    }
    if q.contains("cuantos") {
        return format!("En total son {}.", v);
    }
    if q.contains("que hacer") {
        return format!("Lo correcto es {}.", v);
    }
    if q.contains("que se dice") {
        return format!("En esos casos sueles decir {}.", v);
    }
    if q.contains("que significa") || q.contains("que es ser") {
        return format!("Significa {}.", v);
    }
    if q.contains("como se saluda") {
        return format!("Al llegar: {}.", capitalize_first(v));
    }
    if q.starts_with("como se") {
        let after = q["como se".len()..].trim();
        let verb = after.split_whitespace().next().unwrap_or("hace");
        return format!("Se {} {}.", verb, v);
    }
    if q.contains("cual es") {
        if let Some(t) = topic_subject(&q, &key_orig, &key_folded, "cual es") {
            return format!("{} es {}.", capitalize_first(&t), v);
        }
    }
    if q.contains("que es ") || q.ends_with("que es") {
        if let Some(t) = topic_subject(&q, &key_orig, &key_folded, "que es") {
            return format!("{} es {}.", capitalize_first(&t), v);
        }
    }
    format!("Te cuento que {}.", v)
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c == '\u{0}' {
            continue;
        }
        cur.push(c);
        if matches!(c, '.' | '!' | '?') || c == '\n' {
            let t = cur.trim().to_string();
            if t.chars().count() > 8 {
                out.push(t);
            }
            cur.clear();
        }
    }
    if cur.trim().chars().count() > 8 {
        out.push(cur.trim().to_string());
    }
    out
}

fn print_banner() {
    println!("╔══════════════════════════════════════════╗");
    println!("║           SynapseAI v0.1.0               ║");
    println!("║   Sistema de IA para Robotica            ║");
    println!("║   Aprendizaje Continuo + Conciencia      ║");
    println!("╚══════════════════════════════════════════╝");
    println!();
}

fn main() -> Result<()> {
    env_logger::init();
    print_banner();

    let args: Vec<String> = std::env::args().collect();

    let mut mind = SynapseMind::new()?;

    match args.get(1).map(|s| s.as_str()) {
        Some("train") => {
            let episodes = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(500);
            mind.run_training(episodes)?;
        }
        Some("demo") => {
            println!("Modo demo: ejecutando 100 episodios...\n");
            mind.run_training(100)?;
        }
        Some("web") => {
            let port = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8080);
            mind.run_web(port)?;
        }
        Some("speak") => {
            let text = args.get(2).cloned().unwrap_or_default();
            if text.is_empty() {
                println!("Uso: synapse speak \"texto a decir\" [--voice Bella] [--wav salida.wav] [--speed 1.0]");
                return Ok(());
            }

            let mut voice_name = voice::DEFAULT_VOICE.to_string();
            let mut speed = 1.0f32;
            let mut wav_path: Option<String> = None;

            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--voice" => {
                        if i + 1 < args.len() {
                            voice_name = args[i + 1].clone();
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    "--speed" => {
                        if i + 1 < args.len() {
                            speed = args[i + 1].parse().unwrap_or(1.0);
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    "--wav" => {
                        if i + 1 < args.len() {
                            wav_path = Some(args[i + 1].clone());
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    _ => i += 1,
                }
            }

            println!("Cargando modelo de voz (primera vez descarga, luego offline)...");
            let model = voice::build_voice().map_err(|e| anyhow::anyhow!(e.to_string()))?;
            println!("Voces disponibles: {:?}", model.voices);
            println!("Voz: {} | Velocidad: {}", voice_name, speed);

            if let Some(wav) = &wav_path {
                voice::write_wav(&model, &voice_name, &text, speed, std::path::Path::new(wav))?;
                println!("Audio guardado en {}", wav);
            } else {
                println!("Hablando: {:?}", text);
                voice::speak(&model, &voice_name, &text, speed)?;
                println!("Listo.");
            }
        }
        _ => {
            mind.run_interactive()?;
        }
    }

    println!("\nSynapseAI terminado. Hasta luego!");
    Ok(())
}
