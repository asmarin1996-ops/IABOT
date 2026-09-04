// IMPORTANT NOTE: WORK IN PROGRESS

use std::{ffi::CString, io::Cursor, mem::MaybeUninit};

use hound::{WavSpec, WavWriter};

#[cfg(not(feature = "cuda"))]
use piper_tts_rs_sys::{
    PIPER_DONE, piper_audio_chunk, piper_create, piper_default_synthesize_options, piper_free,
    piper_synthesize_next, piper_synthesize_options, piper_synthesize_start, piper_synthesizer,
};

#[cfg(feature = "cuda")]
use piper_tts_rs_sys::cuda::{
    PIPER_DONE, piper_audio_chunk, piper_create, piper_default_synthesize_options, piper_free,
    piper_synthesize_next, piper_synthesize_options, piper_synthesize_start, piper_synthesizer,
};

#[derive(Debug)]
pub struct PiperAudioChunk {
    pub chunk: piper_audio_chunk,
}

impl PiperAudioChunk {
    pub fn new() -> Self {
        let chunk = unsafe { std::mem::MaybeUninit::<piper_audio_chunk>::zeroed().assume_init() };
        Self { chunk }
    }
}

#[derive(Debug)]
pub struct PiperSession {
    synthesizer: *mut piper_synthesizer,
    options: piper_synthesize_options,
}

impl PiperSession {
    pub fn new(
        model_path: String,
        model_config_path: String,
        espeak_ng_data_path: Option<String>,
    ) -> anyhow::Result<Self> {
        let model_path = CString::new(model_path)?;
        let model_config_path = CString::new(model_config_path)?;
        let espeak_ng_data_path = CString::new(espeak_ng_data_path.unwrap_or_default())?;

        #[cfg(feature = "cuda")]
        let synth = unsafe {
            piper_create(
                model_path.as_ptr(),
                model_config_path.as_ptr(),
                espeak_ng_data_path.as_ptr(),
                true,
            )
        };
        #[cfg(not(feature = "cuda"))]
        let synth = unsafe {
            piper_create(
                model_path.as_ptr(),
                model_config_path.as_ptr(),
                espeak_ng_data_path.as_ptr(),
            )
        };

        let options = unsafe { piper_default_synthesize_options(synth) };

        Ok(Self {
            synthesizer: synth,
            options: options,
        })
    }

    pub fn generate_wav(&self, audio_buffer: &mut Vec<u8>, text: String) -> anyhow::Result<()> {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 22050,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let text = CString::new(text)?;
        unsafe { piper_synthesize_start(self.synthesizer, text.as_ptr(), &self.options) };

        let mut chunk = unsafe { MaybeUninit::<piper_audio_chunk>::zeroed().assume_init() };
        while unsafe { piper_synthesize_next(self.synthesizer, &mut chunk) } != PIPER_DONE as i32 {
            // SAFETY: chunk.samples should be strictly PCM bytes, *const float32

            let audio_chunk =
                unsafe { std::mem::transmute::<*const f32, *const u8>(chunk.samples) };
            let buffer = unsafe {
                std::slice::from_raw_parts(audio_chunk, chunk.num_samples * size_of::<f32>())
            };
            audio_buffer.append(&mut buffer.to_vec());
        }

        WavWriter::new(&mut Cursor::new(audio_buffer), spec)?;
        Ok(())
    }
}

impl Drop for PiperSession {
    fn drop(&mut self) {
        unsafe { piper_free(self.synthesizer) };
    }
}
