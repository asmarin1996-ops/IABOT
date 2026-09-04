use anyhow::Result;
use std::path::PathBuf;
use std::io::{self, Write};

use synapse_core::brain::Brain;
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
        })
    }

    fn run_step(&mut self) -> Result<bool> {
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

        let action = self.brain.decide(&synapse_core::brain::State::new(state_features.clone()));

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
        _ => {
            mind.run_interactive()?;
        }
    }

    println!("\nSynapseAI terminado. Hasta luego!");
    Ok(())
}
