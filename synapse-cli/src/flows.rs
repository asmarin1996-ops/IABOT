//! Motor de flujos: comportamiento autonomo de susana inspirado en los perros.
//!
//! Cuando nadie le ordena nada, la robot recorre una "jornada" de 9 horas
//! dividida en bloques de una hora, cada uno con un comportamiento propio
//! (explorar, vigilar, rondar, descansar, jugar, esperar al dueno).
//!
//! La jornada arranca con la primera orden del dia ("al despertarla") y se
//! mide con reloj propio: se congela mientras susana este pausada (por orden
//! o por que el dueno hizo una pausa manual). Cualquier orden la pone en
//! pausa; retoma sola tras unos minutos sin ordenes, o si el dueno le dice
//! "sigue con tus tareas".

use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate};
use synapse_core::brain::Action;

use crate::SynapseMind;

const SEGMENTOS: usize = 9;

/// Los 9 bloques de una hora del trayecto.
const HORARIO: [&str; SEGMENTOS] = [
    "explorar", "vigilar", "rondar", "descansar", "explorar", "vigilar", "jugar", "rondar",
    "esperar",
];

const NOMBRES: [&str; SEGMENTOS] = [
    "Explorar el entorno",
    "Vigilar el hogar",
    "Rondar el perimetro",
    "Descansar (siesta)",
    "Explorar el entorno",
    "Vigilar el hogar",
    "Jugar un poco",
    "Rondar el perimetro",
    "Esperar al dueno",
];

fn cadencia_seg(flujo: &str) -> u64 {
    match flujo {
        "descansar" => 20,
        "vigilar" => 4,
        "rondar" => 3,
        "explorar" => 5,
        "jugar" => 2,
        "esperar" => 6,
        _ => 10,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EstadoFlujo {
    Durmiendo,
    Activo,
    Pausado,
    Completado,
}

pub struct Flujos {
    jornada_fecha: Option<NaiveDate>,
    reloj_segundos: f64,
    segmento: usize,
    pausado_por_orden: bool,
    pendiente_anuncio: Option<String>,
    config_cargada: bool,
    segmento_s: u64,
    auto_s: u64,
    ultimo_tick: Option<Instant>,
    ultima_accion: Instant,
    head_fase: u8,
    giro_alterno: u8,
}

impl Default for Flujos {
    fn default() -> Self {
        Self::new()
    }
}

impl Flujos {
    pub fn new() -> Self {
        Self {
            jornada_fecha: None,
            reloj_segundos: 0.0,
            segmento: 0,
            pausado_por_orden: false,
            pendiente_anuncio: None,
            config_cargada: false,
            segmento_s: 3600,
            auto_s: 180,
            ultimo_tick: None,
            ultima_accion: Instant::now(),
            head_fase: 0,
            giro_alterno: 0,
        }
    }

    fn cargar_config(&mut self, mind: &SynapseMind) {
        if let Some(v) = mind.config_i64("flujos_segmento_s") {
            if v > 0 {
                self.segmento_s = v as u64;
            }
        }
        if let Some(v) = mind.config_i64("flujos_auto_s") {
            if v > 0 {
                self.auto_s = v as u64;
            }
        }
    }

    /// Cada orden valida pausa las tareas y, si es la primera del dia,
    /// arranca la jornada de 9 horas.
    pub fn orden_recibida(&mut self, hoy: NaiveDate) {
        self.pausado_por_orden = true;
        if self.jornada_fecha != Some(hoy) {
            self.jornada_fecha = Some(hoy);
            self.reloj_segundos = 0.0;
            self.segmento = 0;
            self.ultimo_tick = None;
            self.ultima_accion = Instant::now();
            self.pendiente_anuncio = Some(format!(
                "Buenos dias! Empiezo mi jornada de 9 horas: {}.",
                NOMBRES[0]
            ));
        }
    }

    /// El dueno le ordena seguir con sus tareas.
    pub fn reanudar(&mut self) {
        self.pausado_por_orden = false;
    }

    pub fn tomar_anuncio(&mut self) -> Option<String> {
        self.pendiente_anuncio.take()
    }

    pub fn estado(&self, mind_paused: bool) -> EstadoFlujo {
        if self.jornada_fecha != Some(Local::now().date_naive()) {
            return EstadoFlujo::Durmiendo;
        }
        if self.reloj_segundos >= SEGMENTOS as f64 * self.segmento_s as f64 {
            return EstadoFlujo::Completado;
        }
        if mind_paused || self.pausado_por_orden {
            return EstadoFlujo::Pausado;
        }
        EstadoFlujo::Activo
    }

    pub fn estado_str(&self, mind_paused: bool) -> &'static str {
        match self.estado(mind_paused) {
            EstadoFlujo::Durmiendo => "durmiendo",
            EstadoFlujo::Activo => "activo",
            EstadoFlujo::Pausado => "pausado",
            EstadoFlujo::Completado => "completado",
        }
    }

    pub fn flujo_nombre(&self) -> &'static str {
        NOMBRES[self.segmento]
    }

    pub fn segmento(&self) -> usize {
        self.segmento
    }

    pub fn total_segmentos(&self) -> usize {
        SEGMENTOS
    }

    pub fn avance_horas(&self) -> f64 {
        self.reloj_segundos / 3600.0
    }

    /// Evaluacion periodica del comportamiento autonomo (un tick por iteracion).
    pub fn tick(&mut self, mind: &mut SynapseMind) {
        let ahora = Instant::now();

        if !self.config_cargada {
            self.cargar_config(mind);
            self.config_cargada = true;
        }

        // Reanudar si la pausa era por orden y ya no hay ordenes recientes.
        if self.pausado_por_orden
            && !mind.paused
            && mind.ultima_orden_at.elapsed() >= Duration::from_secs(self.auto_s)
        {
            self.pausado_por_orden = false;
            self.ultimo_tick = None;
            self.ultima_accion = ahora;
            mind.last_message =
                "Llevo un rato sin ordenes: sigo con mis tareas.".to_string();
        }

        // Sin jornada activa: aun durmiendo, esperando la primera orden del dia.
        if self.jornada_fecha != Some(Local::now().date_naive()) {
            return;
        }

        let jornada_completa = self.reloj_segundos >= SEGMENTOS as f64 * self.segmento_s as f64;

        if jornada_completa {
            if self.ultimo_tick.is_some() {
                self.ultimo_tick = None;
                mind.last_message =
                    "He completado mi jornada de 9 horas. Me retiro a descansar.".to_string();
            }
            mind.aplicar_stop();
            return;
        }

        // Pausado (por orden o por el dueno): congelar el reloj y parar motores.
        if mind.paused || self.pausado_por_orden {
            self.ultimo_tick = None;
            mind.aplicar_stop();
            return;
        }

        // Activo: avanzar el reloj solo con el tiempo realmente vivido.
        if let Some(t) = self.ultimo_tick {
            self.reloj_segundos += t.elapsed().as_secs_f64();
        }
        self.ultimo_tick = Some(ahora);

        // Transicion de bloque de la jornada.
        let nuevo = ((self.reloj_segundos / self.segmento_s as f64) as usize).min(SEGMENTOS - 1);
        if nuevo != self.segmento {
            self.segmento = nuevo;
            self.head_fase = 0;
            self.giro_alterno = 0;
            mind.last_message = format!("Nuevo flujo: {}.", NOMBRES[nuevo]);
        }

        let flujo = HORARIO[self.segmento];
        if self.ultima_accion.elapsed() >= Duration::from_secs(cadencia_seg(flujo)) {
            self.ultima_accion = ahora;
            self.ejecutar_flujo(mind, flujo);
        }
    }

    fn ejecutar_flujo(&mut self, mind: &mut SynapseMind, flujo: &str) {
        match flujo {
            "descansar" => {
                mind.aplicar_stop();
                mind.emotion.energy_level = (mind.emotion.energy_level + 0.004).min(1.0);
                mind.emotion.stress = (mind.emotion.stress - 0.01).max(0.0);
            }
            "vigilar" => {
                mind.aplicar_stop();
                self.barrer_cabeza(mind);
                self.espiar(mind);
            }
            "esperar" => {
                mind.aplicar_stop();
                mover_cabeza_servo(mind, 135.0);
                self.espiar(mind);
            }
            "rondar" => self.patrullar(mind),
            "explorar" => {
                if self.giro_alterno % 3 == 0 {
                    self.patrullar(mind);
                } else {
                    self.vagabundear(mind);
                }
                self.espiar(mind);
            }
            "jugar" => self.jugar(mind),
            _ => {}
        }
    }

    /// Escaneo del cuello de un lado a otro, como un perro atento.
    fn barrer_cabeza(&mut self, mind: &mut SynapseMind) {
        const ANGULOS: [f64; 3] = [135.0, 90.0, 45.0];
        mover_cabeza_servo(mind, ANGULOS[(self.head_fase % 3) as usize]);
        self.head_fase = (self.head_fase + 1) % 3;
    }

    /// Ver y escuchar cada poco, sin pisar los limites de recursos de la VM.
    fn espiar(&mut self, mind: &mut SynapseMind) {
        if mind.last_vision_at.elapsed() >= Duration::from_secs(5) && !mind.memoria_baja(300) {
            let (b, m) = mind.percepcion_hints();
            mind.vista = mind.percepcion.ver(b, m);
            mind.last_vision_at = Instant::now();
        }
        if mind.last_audio_at.elapsed() >= Duration::from_secs(3) && !mind.memoria_baja(300) {
            mind.oido = mind.percepcion.oir();
            mind.last_audio_at = Instant::now();
            if mind.oido.voz {
                mind.last_message =
                    "He oido una voz. Puede que me esten llamando.".to_string();
            }
        }
    }

    /// Ronda: avanza hacia adelante con pasos cortos y rodea las paredes,
    /// respetando las reglas absolutas del dueno.
    fn patrullar(&mut self, mind: &mut SynapseMind) {
        let lecturas = mind.world.sensor_readings();
        let pared_arriba = leer(&lecturas, "wall_up");
        let pared_izq = leer(&lecturas, "wall_left");
        let pared_der = leer(&lecturas, "wall_right");

        let accion = if pared_arriba < 0.35 {
            if pared_izq > pared_der {
                Action::TurnRight
            } else {
                Action::TurnLeft
            }
        } else {
            Action::Forward
        };
        if mind.action_forbidden(accion).is_none() {
            mind.apply_action(accion);
        } else {
            mind.aplicar_stop();
        }
        self.giro_alterno = self.giro_alterno.wrapping_add(1);
    }

    /// Deambular curioso: avanza y de vez en cuando tuerce.
    fn vagabundear(&mut self, mind: &mut SynapseMind) {
        let lecturas = mind.world.sensor_readings();
        let pared_arriba = leer(&lecturas, "wall_up");
        let accion = if pared_arriba < 0.3 {
            if self.giro_alterno % 2 == 0 {
                Action::TurnLeft
            } else {
                Action::TurnRight
            }
        } else if self.giro_alterno % 3 == 0 {
            Action::TurnLeft
        } else {
            Action::Forward
        };
        self.giro_alterno = self.giro_alterno.wrapping_add(1);
        if mind.action_forbidden(accion).is_none() {
            mind.apply_action(accion);
        } else {
            mind.aplicar_stop();
        }
    }

    /// Un rato de juego: giros cortos y la cabeza de un lado a otro.
    fn jugar(&mut self, mind: &mut SynapseMind) {
        let accion = match self.giro_alterno % 3 {
            0 => Action::TurnLeft,
            1 => Action::TurnRight,
            _ => Action::Forward,
        };
        self.giro_alterno = self.giro_alterno.wrapping_add(1);
        if mind.action_forbidden(accion).is_none() {
            mind.apply_action(accion);
        } else {
            mind.aplicar_stop();
        }
        const ANGULOS: [f64; 3] = [45.0, 135.0, 90.0];
        mover_cabeza_servo(mind, ANGULOS[(self.head_fase % 3) as usize]);
        self.head_fase = (self.head_fase + 1) % 3;
        mind.emotion.energy_level = (mind.emotion.energy_level - 0.003).max(0.0);
    }
}

fn leer(lecturas: &[(String, f64)], nombre: &str) -> f64 {
    lecturas
        .iter()
        .find(|(n, _)| n == nombre)
        .map(|(_, v)| *v)
        .unwrap_or(1.0)
}

fn mover_cabeza_servo(mind: &mut SynapseMind, grados: f64) {
    use synapse_hal::actuator::ActuatorCommand;
    mind.actuators
        .execute_all(ActuatorCommand::Custom("servo_cabezal".to_string(), vec![grados]));
    mind.last_cabeza = grados;
}