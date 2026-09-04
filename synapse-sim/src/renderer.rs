use crate::world::{Cell, World};

pub struct AsciiRenderer;

impl AsciiRenderer {
    pub fn render_world(world: &World) -> String {
        let mut output = String::new();

        output.push_str("+");
        for _ in 0..world.width {
            output.push_str("--");
        }
        output.push_str("+\n");

        for y in 0..world.height {
            output.push('|');
            for x in 0..world.width {
                let ch = match world.grid[y][x] {
                    Cell::Empty => "  ",
                    Cell::Wall => "##",
                    Cell::Obstacle => "::",
                    Cell::Goal => "GG",
                    Cell::Robot => "RR",
                };
                output.push_str(ch);
            }
            output.push_str("|\n");
        }

        output.push_str("+");
        for _ in 0..world.width {
            output.push_str("--");
        }
        output.push_str("+\n");

        output
    }

    pub fn render_with_sensors(world: &World, readings: &[(String, f64)]) -> String {
        let mut output = Self::render_world(world);

        output.push_str("\nLecturas de sensores:\n");
        for (name, value) in readings {
            let bar_len = (value * 20.0) as usize;
            let bar: String = "=".repeat(bar_len);
            output.push_str(&format!("  {:15} [{:20}] {:.2}\n", name, bar, value));
        }

        output
    }

    pub fn render_stats(
        episode: u64,
        goals: u64,
        total_reward: f64,
        exploration: f64,
        confidence: f64,
    ) -> String {
        format!(
            "Episodio: {} | Metas: {} | Reward: {:.1} | Exploracion: {:.0}% | Confianza: {:.0}%",
            episode, goals, total_reward, exploration * 100.0, confidence * 100.0
        )
    }

    pub fn render_thought(
        action: &str,
        emotion: &str,
        reason: &str,
    ) -> String {
        format!(
            "[IA] Accion: {} | Emocion: {} | Razon: {}",
            action, emotion, reason
        )
    }
}
