use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};

pub const SAMPLE_RATE: u32 = 22050;
pub const DEFAULT_VOICE: &str = "es_MX-claude-high";

const MODEL_SUBDIR: &str = "synapse/piper/es_MX-claude-high";
const MODEL_ONNX: &str = "es_MX-claude-high.onnx";
const MODEL_JSON: &str = "es_MX-claude-high.onnx.json";

pub struct VoiceModel {
    inner: Mutex<PipeWrap>,
    pub voices: Vec<String>,
}

struct PipeWrap(piper_tts_rs::PiperSession);
unsafe impl Send for PipeWrap {}

pub fn build_voice() -> Result<VoiceModel> {
    let dir = model_dir();
    let onnx = dir.join(MODEL_ONNX);
    let json = dir.join(MODEL_JSON);

    if !onnx.exists() || !json.exists() {
        anyhow::bail!(
            "No se encontro el modelo de voz Piper en: {} (se espera {} y {}). \
             Copia el modelo es_MX-claude-high ahi.",
            dir.display(),
            MODEL_ONNX,
            MODEL_JSON
        );
    }

    let session = piper_tts_rs::PiperSession::new(
        onnx.to_string_lossy().to_string(),
        json.to_string_lossy().to_string(),
        None,
    )
    .context("No se pudo inicializar Piper")?;

    Ok(VoiceModel {
        inner: Mutex::new(PipeWrap(session)),
        voices: vec![DEFAULT_VOICE.to_string()],
    })
}

fn model_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SYNAPSE_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".cache".to_string());
    PathBuf::from(home).join(".cache").join(MODEL_SUBDIR)
}

fn synthesize(voice: &VoiceModel, text: &str) -> Result<Vec<u8>> {
    let raw = {
        let guard = voice.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut raw_buf: Vec<u8> = Vec::new();
        guard
            .0
            .generate_wav(&mut raw_buf, text.trim().to_string())
            .with_context(|| "No se pudo sintetizar el audio")?;
        raw_buf
    };
    if raw.len() <= 44 {
        return Ok(Vec::new());
    }
    Ok(clean_wav(&raw))
}

/// Rebuilds a valid WAV (float32, mono, 22050 Hz) from the raw buffer
/// produced by piper-tts-rs (its own header overwrite is unreliable).
fn clean_wav(raw: &[u8]) -> Vec<u8> {
    let samples = &raw[44..];
    let samples_len = samples.len();
    let data_len = samples_len as u32;
    let mut out = Vec::with_capacity(44 + samples_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes()); // PCM float
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes()); // byte rate
    out.extend_from_slice(&4u16.to_le_bytes()); // block align
    out.extend_from_slice(&32u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(samples);
    out
}

pub fn synthesize_wav(voice: &VoiceModel, text: &str) -> Result<Vec<u8>> {
    synthesize(voice, text)
}

pub fn speak(voice: &VoiceModel, _voice_name: &str, text: &str, _speed: f32) -> Result<()> {
    let wav = synthesize(voice, text)?;
    if wav.is_empty() {
        return Ok(());
    }
    play_wav(&wav);
    Ok(())
}

pub fn write_wav(
    voice: &VoiceModel,
    _voice_name: &str,
    text: &str,
    _speed: f32,
    output: &Path,
) -> Result<()> {
    let wav = synthesize(voice, text)?;
    std::fs::write(output, &wav)
        .with_context(|| format!("No se pudo escribir el WAV {}", output.display()))
}

fn play_wav(wav: &[u8]) {
    let mut sink = match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("No se pudo abrir la salida de audio del sistema: {}", e);
            return;
        }
    };
    sink.log_on_drop(false);

    let mixer = sink.mixer().clone();
    match rodio::stream::play(&mixer, Cursor::new(wav.to_vec())) {
        Ok(player) => player.sleep_until_end(),
        Err(e) => eprintln!("No se pudo reproducir el audio: {}", e),
    }
}