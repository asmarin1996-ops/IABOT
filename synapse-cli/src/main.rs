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
    memory_db: MemoryDatabase,
    agent_state: AgentState,
    episode: u64,
    total_goals: u64,
    name: String,
    wake_word: String,
    knowledge: Vec<MemEntry>,
    token_index: HashMap<String, Vec<usize>>,
    paused: bool,
    last_message: String,
    last_wake_at: std::time::Instant,
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
        actuators.add(Box::new(VirtualActuator::new("motor_izq")));
        actuators.add(Box::new(VirtualActuator::new("motor_der")));
        actuators.add(Box::new(VirtualActuator::new("servo_cabezal")));

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
            memory_db,
            agent_state: AgentState::new(),
            episode: 0,
            total_goals: 0,
            name,
            wake_word,
            knowledge,
            token_index,
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

        let action = match forced {
            Some(a) => a,
            None => self.brain.decide(&synapse_core::brain::State::new(state_features.clone())),
        };

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

    fn actuator_cmd_for(action: Action) -> synapse_hal::actuator::ActuatorCommand {
        match action {
            Action::Forward => synapse_hal::actuator::ActuatorCommand::MoveForward(0.5),
            Action::Backward => synapse_hal::actuator::ActuatorCommand::MoveBackward(0.5),
            Action::TurnLeft => synapse_hal::actuator::ActuatorCommand::TurnLeft(30.0),
            Action::TurnRight => synapse_hal::actuator::ActuatorCommand::TurnRight(30.0),
            Action::Stop | Action::Custom(_) => synapse_hal::actuator::ActuatorCommand::Stop,
        }
    }

    fn apply_action(&mut self, action: Action) {
        self.actuators.execute_all(Self::actuator_cmd_for(action));
        self.robot.execute_action(action, &mut self.world);
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
                return format!(
                    "Episodio {}, metas {}, posicion {:?}, emocion {}",
                    self.episode, self.total_goals, self.robot.state.position,
                    self.emotion.dominant_emotion(),
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
                let facts: Vec<&MemEntry> = self.knowledge.iter().filter(|e| e.cat != "material").collect();
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
            WebCommand::Unknown => {
                match self.retrieve(msg) {
                    Some(e) if e.cat == "material" => {
                        let src = if e.source.is_empty() {
                            "el PDF".to_string()
                        } else {
                            e.source.clone()
                        };
                        format!("Del documento {}, recuerdo: {}", src, e.value)
                    }
                    Some(e) => e.value.clone(),
                    None => {
                        "Aun no tengo informacion sobre eso. Ensename con 'aprende que X es Y' o sube un PDF."
                            .to_string()
                    }
                }
            }
        }
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
            if match_count < 2 {
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
            "{{\"nombre\":\"{}\",\"activacion\":\"{}\",\"emocion\":\"{}\",\"emoji\":\"{}\",\"confianza\":{},\"estres\":{},\"energia\":{},\"curiosidad\":{},\"explotacion\":{},\"explotar\":{},\"episodio\":{},\"total_metas\":{},\"posicion\":\"({}, {})\",\"estados\":{},\"experiencias\":{},\"adaptaciones\":{},\"recompensa\":{},\"mundo\":\"{}\",\"sensores\":{},\"mensaje\":\"{}\"}}",
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
    }
}

fn normalize_key(key: &str) -> String {
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
    normalize_key(text)
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
