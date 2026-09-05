//! Percepcion sensorial de susana: ver (camara) y oir (microfono).
//!
//! Disenada para NO agotar los recursos de la VM:
//! - En cualquier plataforma sin hardware usa un backend virtual, trivial en CPU/RAM.
//! - Detras de la feature "percepcion_real" (pensada para Raspberry Pi) captura
//!   un frame pequeno de V4L2 y una ventana de audio muy corta via ALSA.
//! - Si no hay dispositivo la fuente queda "sin lectura" y nunca crashea el proceso.
//! - El cerebro limita la frecuencia de captura y consulta la memoria libre.

#[derive(Debug, Clone)]
pub struct Vista {
    pub fuente: String,
    pub ancho: u32,
    pub alto: u32,
    pub brillo: f64,
    pub movimiento: f64,
    pub texto: String,
}

#[derive(Debug, Clone)]
pub struct Oido {
    pub fuente: String,
    pub nivel: f64,
    pub voz: bool,
    pub duracion_ms: u64,
    pub texto: String,
}

pub trait Percepcion {
    /// `hint_brillo`/`hint_movimiento` son pistas del mundo (usadas por el
    /// backend virtual; el real mide la imagen capturada).
    fn ver(&mut self, hint_brillo: f64, hint_movimiento: f64) -> Vista;
    fn oir(&mut self) -> Oido;
}

/// Backend fantasma: simula una camara y un microfono sin gastar casi nada.
pub struct PercepcionVirtual {
    frame: u64,
}

impl PercepcionVirtual {
    pub fn new() -> Self {
        Self { frame: 0 }
    }
}

impl Default for PercepcionVirtual {
    fn default() -> Self {
        Self::new()
    }
}

impl Percepcion for PercepcionVirtual {
    fn ver(&mut self, hint_brillo: f64, hint_movimiento: f64) -> Vista {
        self.frame += 1;
        let parpadeo = (self.frame % 3 == 0) as u8 as f64 * 0.02;
        let brillo = (hint_brillo + parpadeo).clamp(0.0, 1.0);
        let movimiento = if self.frame % 5 == 0 {
            (hint_movimiento + 0.08).clamp(0.0, 1.0)
        } else {
            hint_movimiento.clamp(0.0, 1.0)
        };
        Vista {
            fuente: "camara_virtual".to_string(),
            ancho: 0,
            alto: 0,
            brillo,
            movimiento,
            texto: format!(
                "Con mi camara virtual veo un recinto con luz al {:.0}% y un {:.0}% de movimiento.",
                brillo * 100.0,
                movimiento * 100.0
            ),
        }
    }

    fn oir(&mut self) -> Oido {
        Oido {
            fuente: "microfono_virtual".to_string(),
            nivel: 0.03,
            voz: false,
            duracion_ms: 1000,
            texto: "Con mi microfono virtual solo percibo silencio, con un leve murmullo de ambiente. No detecto voces."
                .to_string(),
        }
    }
}

#[cfg(feature = "percepcion_real")]
mod real {
    use super::{Oido, Percepcion, Vista};

    use v4l::io::traits::{CaptureStream as _, Stream as _};
    use v4l::video::Capture as _;

    const ANCHO_CAM: u32 = 320;
    const ALTO_CAM: u32 = 240;
    const UMBRAL_VOZ: f64 = 0.06;
    const DURACION_AUDIO_MS: u64 = 1000;

    /// Backend real: camara V4L2 (/dev/video0) + microfono ALSA ("default").
    pub struct PercepcionReal {
        camara: Option<v4l::Device>,
        periodo_alto: u32,
        ultimo_brillo: f64,
        microfono: Option<alsa::pcm::PCM>,
        buf_i16: Vec<i16>,
    }

    impl PercepcionReal {
        pub fn new() -> Self {
            use v4l::video::Capture as _;
            let camara = v4l::Device::with_path("/dev/video0")
                .ok()
                .and_then(|dev| {
                    let fmt = v4l::Format::new(ANCHO_CAM, ALTO_CAM, v4l::FourCC::new(b"YUYV"));
                    if dev.set_format(&fmt).is_ok() {
                        Some(dev)
                    } else {
                        None
                    }
                });
            if camara.is_none() {
                log::warn!("camara V4L2 no disponible (/dev/video0): susana ve con backend virtual");
            }

            let microfono = alsa::pcm::PCM::new("default", alsa::Direction::Capture, false).ok();
            let microfono = microfono.and_then(|pcm| {
                let hwp = match alsa::pcm::HwParams::any(&pcm) {
                    Ok(h) => h,
                    Err(e) => {
                        log::warn!("microfono ALSA sin params: {}", e);
                        return None;
                    }
                };
                if hwp.set_channels(1).is_err()
                    || hwp.set_rate(8000, alsa::ValueOr::Nearest).is_err()
                    || hwp
                        .set_format(alsa::pcm::Format::S16LE)
                        .is_err()
                    || hwp
                        .set_access(alsa::pcm::Access::RWInterleaved)
                        .is_err()
                {
                    log::warn!("microfono ALSA: config de hardware rechazada");
                    return None;
                }
                let res = pcm.hw_params(&hwp);
                drop(hwp);
                match res {
                    Ok(()) => Some(pcm),
                    Err(e) => {
                        log::warn!("microfono ALSA: hw_params fallo: {}", e);
                        None
                    }
                }
            });
            if microfono.is_none() {
                log::warn!("microfono ALSA no disponible: susana escucha con backend virtual");
            }

            Self {
                camara,
                periodo_alto: 0,
                ultimo_brillo: 0.5,
                microfono,
                buf_i16: vec![0i16; 8000],
            }
        }

        fn capturar_brillo(&mut self) -> (f64, f64) {
            let Some(dev) = self.camara.as_mut() else {
                return (self.ultimo_brillo, 0.0);
            };
            let res = (|| -> Result<(f64, f64), String> {
                let mut stream =
                    v4l::io::mmap::Stream::new(dev, v4l::buffer::Type::VideoCapture)
                        .map_err(|e| e.to_string())?;
                let (frame, _meta) = stream.next().map_err(|e| e.to_string())?;
                if frame.len() < 4 {
                    return Err("frame vacio".to_string());
                }
                // YUYV: los bytes en posiciones impares son el luma (Y)
                let mut total = 0.0f64;
                let mut n = 0u64;
                for b in frame.iter().step_by(2) {
                    total += *b as f64 / 255.0;
                    n += 1;
                    if n >= 4096 {
                        break;
                    }
                }
                let brillo = if n > 0 { total / n as f64 } else { 0.5 };
                let mov = (brillo - self.ultimo_brillo).abs();
                self.ultimo_brillo = brillo;
                Ok((brillo, mov))
            })();
            match res {
                Ok((b, m)) => (b, m),
                Err(e) => {
                    log::warn!("camara V4L2: captura fallo: {}", e);
                    (self.ultimo_brillo, 0.0)
                }
            }
        }
    }

    impl Percepcion for PercepcionReal {
        fn ver(&mut self, hint_brillo: f64, _hint_movimiento: f64) -> Vista {
            if self.camara.is_some() {
                self.periodo_alto += 1;
                let (brillo, mov) = self.capturar_brillo();
                Vista {
                    fuente: "camara_v4l".to_string(),
                    ancho: ANCHO_CAM,
                    alto: ALTO_CAM,
                    brillo,
                    movimiento: mov,
                    texto: format!(
                        "Capturo una imagen de camara {0}x{1} con un brillo del {2:.0}% y {3:.0}% de cambio respecto a la anterior.",
                        ANCHO_CAM, ALTO_CAM, brillo * 100.0, mov * 100.0
                    ),
                }
            } else {
                Vista {
                    fuente: "camara_offline".to_string(),
                    ancho: 0,
                    alto: 0,
                    brillo: hint_brillo.clamp(0.0, 1.0),
                    movimiento: 0.0,
                    texto: "No tengo camara conectada en este momento.".to_string(),
                }
            }
        }

        fn oir(&mut self) -> Oido {
            if let Some(pcm) = self.microfono.as_mut() {
                let mut total = 0.0f64;
                let mut n = 0usize;
                if let Ok(io) = alsa::pcm::PCM::io_i16(pcm) {
                    match io.readi(&mut self.buf_i16) {
                        Ok(read) => {
                            for s in &self.buf_i16[..read] {
                                total += s.unsigned_abs() as f64 / 32768.0;
                                n += 1;
                            }
                        }
                        Err(e) => log::warn!("microfono ALSA: lectura fallo: {}", e),
                    }
                }
                let nivel = if n > 0 { total / n as f64 } else { 0.0 };
                let voz = nivel >= UMBRAL_VOZ;
                Oido {
                    fuente: "microfono_alsa".to_string(),
                    nivel,
                    voz,
                    duracion_ms: DURACION_AUDIO_MS,
                    texto: if voz {
                        "Escuche voz o conversacion cerca de mi.".to_string()
                    } else if nivel >= 0.02 {
                        "Escuche un sonido leve, como ruido de ambiente.".to_string()
                    } else {
                        "Silencio: no detecte sonidos ni voces.".to_string()
                    },
                }
            } else {
                Oido {
                    fuente: "microfono_offline".to_string(),
                    nivel: 0.0,
                    voz: false,
                    duracion_ms: 0,
                    texto: "No tengo microfono conectado en este momento.".to_string(),
                }
            }
        }
    }
}

#[cfg(feature = "percepcion_real")]
pub use real::PercepcionReal;