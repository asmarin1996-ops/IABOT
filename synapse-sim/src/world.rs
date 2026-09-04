use rand::Rng;

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Empty,
    Wall,
    Obstacle,
    Goal,
    Robot,
}

pub struct World {
    pub width: usize,
    pub height: usize,
    pub grid: Vec<Vec<Cell>>,
    pub robot_pos: (usize, usize),
    pub goal_pos: (usize, usize),
}

impl World {
    pub fn new(width: usize, height: usize) -> Self {
        let mut grid = vec![vec![Cell::Empty; width]; height];
        let goal_pos = (width - 2, height - 2);
        grid[goal_pos.1][goal_pos.0] = Cell::Goal;

        let mut world = Self {
            width,
            height,
            grid,
            robot_pos: (1, 1),
            goal_pos,
        };

        world.generate_walls();
        world.grid[1][1] = Cell::Robot;
        world
    }

    pub fn new_empty(width: usize, height: usize) -> Self {
        let grid = vec![vec![Cell::Empty; width]; height];
        let goal_pos = (width - 2, height - 2);
        let mut world = Self {
            width,
            height,
            grid,
            robot_pos: (1, 1),
            goal_pos,
        };
        world.grid[goal_pos.1][goal_pos.0] = Cell::Goal;
        world.grid[1][1] = Cell::Robot;
        world
    }

    fn generate_walls(&mut self) {
        let mut rng = rand::thread_rng();
        let wall_count = (self.width * self.height) / 10;

        for _ in 0..wall_count {
            let x = rng.gen_range(0..self.width);
            let y = rng.gen_range(0..self.height);

            if (x, y) == self.robot_pos || (x, y) == self.goal_pos {
                continue;
            }
            if x <= 2 && y <= 2 {
                continue;
            }

            self.grid[y][x] = Cell::Wall;
        }
    }

    pub fn is_valid_position(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height && self.grid[y][x] != Cell::Wall
    }

    pub fn move_robot(&mut self, dx: i32, dy: i32) -> bool {
        let new_x = self.robot_pos.0 as i32 + dx;
        let new_y = self.robot_pos.1 as i32 + dy;

        if new_x < 0 || new_y < 0 {
            return false;
        }

        let nx = new_x as usize;
        let ny = new_y as usize;

        if !self.is_valid_position(nx, ny) {
            return false;
        }

        self.grid[self.robot_pos.1][self.robot_pos.0] = Cell::Empty;
        self.robot_pos = (nx, ny);
        self.grid[ny][nx] = Cell::Robot;
        true
    }

    pub fn sensor_readings(&self) -> Vec<(String, f64)> {
        let rx = self.robot_pos.0 as i32;
        let ry = self.robot_pos.1 as i32;

        let dist_to_wall_up = self.distance_to_wall(rx, ry, 0, -1) as f64;
        let dist_to_wall_down = self.distance_to_wall(rx, ry, 0, 1) as f64;
        let dist_to_wall_left = self.distance_to_wall(rx, ry, -1, 0) as f64;
        let dist_to_wall_right = self.distance_to_wall(rx, ry, 1, 0) as f64;

        let gx = self.goal_pos.0 as f64;
        let gy = self.goal_pos.1 as f64;
        let dx = gx - rx as f64;
        let dy = gy - ry as f64;
        let dist_to_goal = (dx * dx + dy * dy).sqrt();

        let max_dist = ((self.width as f64).powi(2) + (self.height as f64).powi(2)).sqrt();

        vec![
            (
                "wall_up".to_string(),
                dist_to_wall_up / self.height as f64,
            ),
            (
                "wall_down".to_string(),
                dist_to_wall_down / self.height as f64,
            ),
            (
                "wall_left".to_string(),
                dist_to_wall_left / self.width as f64,
            ),
            (
                "wall_right".to_string(),
                dist_to_wall_right / self.width as f64,
            ),
            ("goal_distance".to_string(), dist_to_goal / max_dist),
        ]
    }

    fn distance_to_wall(&self, x: i32, y: i32, dx: i32, dy: i32) -> i32 {
        let mut cx = x;
        let mut cy = y;
        let mut dist = 0;

        loop {
            cx += dx;
            cy += dy;
            dist += 1;

            if cx < 0 || cy < 0 || cx >= self.width as i32 || cy >= self.height as i32 {
                return dist;
            }
            if self.grid[cy as usize][cx as usize] == Cell::Wall {
                return dist;
            }
        }
    }

    pub fn reached_goal(&self) -> bool {
        self.robot_pos == self.goal_pos
    }

    pub fn reset_robot(&mut self) {
        self.grid[self.robot_pos.1][self.robot_pos.0] = Cell::Empty;
        self.robot_pos = (1, 1);
        self.grid[1][1] = Cell::Robot;
    }

    pub fn randomize(&mut self) {
        for row in &mut self.grid {
            for cell in row.iter_mut() {
                *cell = Cell::Empty;
            }
        }
        self.grid[self.goal_pos.1][self.goal_pos.0] = Cell::Goal;
        self.generate_walls();
        self.reset_robot();
    }
}
