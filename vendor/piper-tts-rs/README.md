# Piper TTS Rust direct binding

# Requirements
Need to install clang (and other built-in tools, that already exists on many linux distributions)

`sudo apt-get install libclang-dev`

## Tips
- Convert RAW to WAV for piper, use `ffmpeg -f f32le -ar 22050 -ac 1 -i audio.raw audio.wav` (soon this convertation will be implemented)
- `cargo run --example simple` - generates .raw file (raw pcm chunks 22050 Hz, 1 mono)

[Let's listen](https://github.com/user-attachments/files/24545995/audio.wav)

## Usage (simple)

```rust
use piper_tts_rs::PiperSession;
use std::{fs::File, io::Write};

fn main() {
    let mut file = File::create("./samples/test.wav").expect("Error");
    let session = PiperSession::new(
        "./model.onnx".to_string(),
        "./model.onnx.json".to_string(),
        None,
    )
    .expect("error during creating session");

    let inference_text = r#"
        Jahon savdo tashkilotiga a’zolikning yakuniy pallasi: 
        O‘zbekiston 2,5 oy ichida ulgura oladimi? Kasalni yashirsang,
        isitmasi oshkor qiladi: o‘qituvchilar majburiy mehnatga tizimli ravishda jalb qilindimi?
    "#
    .to_string();

    // ! Clean WAV file with headers, do not use in streaming
    let mut empty_buff = Vec::<u8>::new();
    session
        .generate_wav(&mut empty_buff, inference_text)
        .expect("failed to generate chunks");
    file.write_all(&empty_buff)
        .expect("failed saving chunks to file");

    println!("WAV File saved successfully!")
}
```

### Use with CUDA
```bash
cargo run --example simple_native --features cuda
```

or

```toml
piper-tts-rs = { version = "0.1.3", features = "cuda" }
```