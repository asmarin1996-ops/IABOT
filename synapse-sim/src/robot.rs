use crate::world::World;
use synapse_core::brain::Action;
use synapse_core::state::AgentState;

pub struct VirtualRobot {
    pub state: AgentState,
    pub total_distance: u64,
    pub goals_reached: u64,
    pub steps_in_episode: u64,
    pub max_steps: u64,
}

impl VirtualRobot {
    pub fn new() -> Self {
        Self {
            state: AgentState::new(),
            total_distance: 0,
            goals_reached: 0,
            steps_in_episode: 0,
            max_steps: 200,
        }
    }

    pub fn execute_action(&mut self, action: Action, world: &mut World) -> bool {
        let (dx, dy) = match action {
            Action::Forward => (0, -1),
            Action::Backward => (0, 1),
            Action::TurnLeft => (-1, 0),
            Action::TurnRight => (1, 0),
            Action::Stop => (0, 0),
            Action::Custom(_) => (0, 0),
        };

        self.steps_in_episode += 1;

        if dx == 0 && dy == 0 {
            self.state.record_action(true);
            return true;
        }

        let success = world.move_robot(dx, dy);
        self.state.record_action(success);

        if success {
            self.total_distance += 1;
        }

        success
    }

    pub fn compute_reward(&self, world: &World, _action: Action) -> f64 {
        if world.reached_goal() {
            return 100.0;
        }

        let rx = world.robot_pos.0 as f64;
        let ry = world.robot_pos.1 as f64;
        let gx = world.goal_pos.0 as f64;
        let gy = world.goal_pos.1 as f64;

        let current_dist = ((rx - gx).powi(2) + (ry - gy).powi(2)).sqrt();
        let max_dist = ((world.width as f64).powi(2) + (world.height as f64).powi(2)).sqrt();

        let proximity_reward = (1.0 - current_dist / max_dist) * 10.0;

        let step_penalty = -0.5;

        let stuck_penalty = if self.steps_in_episode > self.max_steps / 2 {
            -2.0
        } else {
            0.0
        };

        proximity_reward + step_penalty + stuck_penalty
    }

    pub fn at_goal(&self, world: &World) -> bool {
        world.reached_goal()
    }

    pub fn reset_episode(&mut self, world: &mut World) -> f64 {
        let episode_reward = if world.reached_goal() {
            self.goals_reached += 1;
            100.0
        } else {
            0.0
        };

        world.reset_robot();
        self.steps_in_episode = 0;
        episode_reward
    }

    pub fn stats(&self) -> String {
        format!(
            "Pos: {:?} | Pasos: {} | Metas: {} | Dist: {} | Exito: {:.0}%",
            self.state.position,
            self.steps_in_episode,
            self.goals_reached,
            self.total_distance,
            self.state.success_rate() * 100.0
        )
    }
}
