use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

pub struct WebHubData {
    pub name: String,
    pub wake_word: String,
    pub status: String,
    pub commands: VecDeque<String>,
    pub voice_enabled: bool,
    pub voice_name: String,
    pub voices: Vec<String>,
    pub speak_requests: VecDeque<String>,
    pub learn_requests: VecDeque<(String, String)>,
    pub pdf_requests: VecDeque<(String, String)>,
    pub rules_requests: VecDeque<(String, String)>,
    pub learned_summary: String,
    pub rules: Vec<String>,
}

pub type WebHub = Arc<Mutex<WebHubData>>;

pub fn make_hub(name: &str, wake_word: &str) -> WebHub {
    Arc::new(Mutex::new(WebHubData {
        name: name.to_string(),
        wake_word: wake_word.to_string(),
        status: "{}".to_string(),
        commands: VecDeque::new(),
        voice_enabled: true,
        voice_name: crate::voice::DEFAULT_VOICE.to_string(),
        voices: Vec::new(),
        speak_requests: VecDeque::new(),
        learn_requests: VecDeque::new(),
        pdf_requests: VecDeque::new(),
        rules_requests: VecDeque::new(),
        learned_summary: String::new(),
        rules: Vec::new(),
    }))
}

pub fn run_api_server(port: u16, hub: WebHub) {
    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap_or_else(|e| {
        eprintln!("Direccion invalida: {}", e);
        std::process::exit(1)
    });

    let listener = mio::net::TcpListener::bind(addr).unwrap_or_else(|e| {
        eprintln!("No se pudo abrir el puerto {}: {}", port, e);
        std::process::exit(1)
    });

    println!("Dashboard web en http://0.0.0.0:{}", port);

    loop {
        let accept = listener.accept();
        if accept.is_err() {
            continue;
        }
        let (stream, _) = accept.unwrap();
        handle_connection(stream, hub.clone());
    }
}

fn handle_connection(stream: mio::net::TcpStream, hub: WebHub) {
    let mut s = stream;
    let request = read_request(&mut s);
    if request.is_empty() {
        return;
    }

    let (method, path, body) = split_request(&request);

    if method == "GET" && (path == "/" || path == "/index.html") {
        send_http(&mut s, 200, "text/html; charset=utf-8", DASHBOARD_HTML.as_bytes());
        return;
    }

    if method == "GET" && (path == "/api/speak.wav" || path.starts_with("/api/speak.wav?")) {
        handle_speak_wav(&mut s, path.as_str());
        return;
    }

    if method == "GET" && (path == "/api/learn" || path.starts_with("/api/learn?")) {
        handle_learn_get(&mut s, path.as_str(), hub);
        return;
    }

    if method == "GET" && path.starts_with("/api/") {
        handle_get_api(&mut s, path.as_str(), hub);
        return;
    }

    if (method == "POST" || method == "PUT") && path.starts_with("/api/") {
        handle_write_api(&mut s, path.as_str(), body.as_str(), hub);
        return;
    }

    send_json(&mut s, 404, "{\"error\":\"no encontrado\"}");
}

fn handle_speak_wav(s: &mut mio::net::TcpStream, path: &str) {
    let text = if let Some(q) = path.split('?').nth(1) {
        let raw: String = q
            .split('&')
            .map(|kv| {
                let mut it = kv.splitn(2, '=');
                let key = it.next().unwrap_or("");
                let val = it.next().unwrap_or("");
                if key == "text" {
                    val
                } else {
                    ""
                }
            })
            .collect();
        percent_decode(&raw)
    } else {
        String::new()
    };
    if text.is_empty() {
        send_json(s, 400, "{\"error\":\"texto vacio\"}");
        return;
    }
    match synth_text(&text) {
        Ok(wav) => {
            send_http(s, 200, "audio/wav", &wav);
            println!("OK speak.wav: {} bytes, txt={}", wav.len(), text);
        }
        Err(e) => {
            send_json(s, 500, &format!("{{\"error\":\"voz no disponible: {}\"}}", e));
        }
    }
}

fn handle_learn_get(s: &mut mio::net::TcpStream, path: &str, hub: WebHub) {
    let mut phrase = String::new();
    let mut meaning = String::new();
    if let Some(q) = path.split('?').nth(1) {
        for kv in q.split('&') {
            let mut it = kv.splitn(2, '=');
            let key = it.next().unwrap_or("");
            let val = it.next().unwrap_or("");
            if key == "phrase" {
                phrase = percent_decode(val).replace('+', " ");
            } else if key == "meaning" {
                meaning = percent_decode(val).replace('+', " ");
            }
        }
    }
    if phrase.is_empty() || meaning.is_empty() {
        send_json(s, 400, "{\"error\":\"faltan datos\"}");
        return;
    }
    {
        let mut guard = hub.lock().unwrap();
        guard.learn_requests.push_back((phrase.clone(), meaning.clone()));
    }
    let resp = format!("{{\"ok\":true,\"aprendido\":\"{}\"}}", json_escape(&phrase));
    send_json(s, 200, resp.as_str());
}

fn handle_get_api(s: &mut mio::net::TcpStream, path: &str, hub: WebHub) {
    let guard = hub.lock().unwrap();
    match path {
        "/api/status" => send_json(s, 200, guard.status.as_str()),
        "/api/config" => {
            let body = format!(
                "{{\"nombre\":\"{}\",\"activacion\":\"{}\",\"voz\":\"{}\",\"voz_activa\":{},\"voces\":[{}]}}",
                json_escape(&guard.name),
                json_escape(&guard.wake_word),
                json_escape(&guard.voice_name),
                if guard.voice_enabled { "true" } else { "false" },
                format_voices(&guard.voices),
            );
            send_json(s, 200, body.as_str());
        }
        "/api/voices" => {
            let body = format!("{{\"voces\":[{}]}}", format_voices(&guard.voices));
            send_json(s, 200, body.as_str());
        }
        "/api/learned" => {
            let body = format!("{{\"aprendido\":\"{}\"}}", json_escape(&guard.learned_summary));
            send_json(s, 200, body.as_str());
        }
        "/api/rules" => {
            let mut out = String::new();
            for (i, r) in guard.rules.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!("\"{}\"", json_escape(r)));
            }
            let body = format!("{{\"reglas\":[{}]}}", out);
            send_json(s, 200, body.as_str());
        }
        _ => send_json(s, 404, "{\"error\":\"no encontrado\"}"),
    }
}

fn format_voices(voices: &[String]) -> String {
    let mut out = String::new();
    for (i, v) in voices.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{}\"", json_escape(v)));
    }
    out
}

fn handle_write_api(s: &mut mio::net::TcpStream, path: &str, body: &str, hub: WebHub) {
    let mut guard = hub.lock().unwrap();

    if path == "/api/command" {
        let text = extract_json_string(body, "text").unwrap_or_default();
        if text.is_empty() {
            send_json(s, 400, "{\"error\":\"texto vacio\"}");
            return;
        }
        guard.commands.push_back(text);
        send_json(s, 200, "{\"ok\":true,\"recibido\":true}");
        return;
    }

    if path == "/api/rules" {
        let accion = extract_json_string(body, "accion").unwrap_or_default();
        let texto = extract_json_string(body, "texto").unwrap_or_default();
        if texto.is_empty() {
            send_json(s, 400, "{\"error\":\"texto vacio\"}");
            return;
        }
        guard.rules_requests.push_back((accion, texto.clone()));
        let resp = format!("{{\"ok\":true,\"accion\":\"{}\"}}", json_escape(&texto));
        send_json(s, 200, resp.as_str());
        return;
    }

    if path == "/api/config" {
        if let Some(name) = extract_json_string(body, "name") {
            if !name.is_empty() {
                guard.commands.push_back(format!("nombre {}", name));
            }
        }
        if let Some(wake) = extract_json_string(body, "wake_word") {
            if !wake.is_empty() {
                guard.commands.push_back(format!("activacion {}", wake));
            }
        }
        send_json(s, 200, "{\"ok\":true}");
        return;
    }

    if path == "/api/speak" {
        let text = extract_json_string(body, "text").unwrap_or_default();
        if text.is_empty() {
            send_json(s, 400, "{\"error\":\"texto vacio\"}");
            return;
        }
        guard.speak_requests.push_back(text);
        send_json(s, 200, "{\"ok\":true,\"hablando\":true}");
        return;
    }

    if path == "/api/voice" {
        if let Some(enabled) = extract_json_bool(body, "activa") {
            guard.voice_enabled = enabled;
        }
        if let Some(vname) = extract_json_string(body, "voz") {
            if !vname.is_empty() {
                guard.voice_name = vname;
            }
        }
        send_json(s, 200, "{\"ok\":true}");
        return;
    }

    if path == "/api/upload-pdf" {
        let name = extract_json_string(body, "name").unwrap_or_default();
        let b64 = extract_json_string(body, "pdf_base64").unwrap_or_default();
        if name.is_empty() || b64.is_empty() {
            send_json(s, 400, "{\"error\":\"faltan datos\"}");
            return;
        }
        let pdf_bytes = match base64_decode(&b64) {
            Some(bytes) if !bytes.is_empty() => bytes,
            _ => {
                send_json(s, 400, "{\"error\":\"base64 invalido\"}");
                return;
            }
        };
        let clean_name: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let folder = pdf_folder();
        let _ = std::fs::create_dir_all(&folder);
        let path = format!("{}{}.pdf", folder, clean_name);
        let _ = std::fs::write(&path, &pdf_bytes);
        let text = extract_pdf_text(&pdf_bytes);
        if text.trim().chars().count() < 10 {
            send_json(
                s,
                200,
                &format!(
                    "{{\"ok\":true,\"nombre\":\"{}\",\"texto_len\":0,\"msg\":\"no se pudo extraer texto\"}}",
                    json_escape(&clean_name)
                ),
            );
            return;
        }
        guard.pdf_requests.push_back((clean_name.clone(), text.trim().to_string()));
        send_json(
            s,
            200,
            &format!(
                "{{\"ok\":true,\"nombre\":\"{}\",\"texto_len\":{}}}",
                json_escape(&clean_name),
                text.trim().chars().count()
            ),
        );
        return;
    }

    if path == "/api/learn" {
        if let Some(phrase) = extract_json_string(body, "phrase") {
            if let Some(meaning) = extract_json_string(body, "meaning") {
                if !phrase.is_empty() && !meaning.is_empty() {
                    guard.learn_requests.push_back((phrase, meaning));
                    send_json(s, 200, "{\"ok\":true}");
                    return;
                }
            }
        }
        send_json(s, 400, "{\"error\":\"faltan datos\"}");
        return;
    }

    send_json(s, 404, "{\"error\":\"no encontrado\"}");
}

fn split_request(request: &str) -> (String, String, String) {
    let head_end = request.find("\r\n\r\n").unwrap_or(request.len());
    let head = &request[..head_end];
    let body_start = head_end + 4;
    let body = if body_start <= request.len() {
        &request[body_start..]
    } else {
        ""
    };

    let line_end = head.find("\r\n").unwrap_or(head.len());
    let request_line = &head[..line_end];

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    let method = parts.get(0).map(|p| p.to_string()).unwrap_or_default();
    let path = parts.get(1).map(|p| p.to_string()).unwrap_or_default();
    (method, path, body.to_string())
}

fn read_request(s: &mut mio::net::TcpStream) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut attempts = 0;
    let mut need: usize = 0;

    loop {
        attempts += 1;
        if attempts > 4000 {
            break;
        }
        if need > 0 && buf.len() >= need {
            break;
        }

        let mut chunk = [0u8; 65536];
        let n = read_some(s, &mut chunk);

        match n {
            Some(0) => break,
            None => {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Some(len) => {
                buf.extend(&chunk[..len]);
                if need == 0 {
                    if let Some(hdr_end) = find_header_end(&buf) {
                        let body_len = content_length(&buf[..hdr_end]);
                        need = hdr_end + 4 + body_len;
                        if buf.len() >= need {
                            break;
                        }
                    }
                }
            }
        }
    }

    if buf.is_empty() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 3 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' && buf[i + 2] == b'\r' && buf[i + 3] == b'\n' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn content_length(head: &[u8]) -> usize {
    let text = String::from_utf8_lossy(head).to_lowercase();
    for line in text.split("\r\n") {
        if let Some(val) = line.strip_prefix("content-length:") {
            return val.trim().parse::<usize>().unwrap_or(0);
        }
    }
    0
}

fn read_some(s: &mut mio::net::TcpStream, buf: &mut [u8]) -> Option<usize> {
    loop {
        match s.read(buf) {
            Ok(n) => return Some(n),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(_) => return None,
        }
    }
}

fn send_http(s: &mut mio::net::TcpStream, code: u16, ctype: &str, body: &[u8]) {
    let reason = match code {
        200 => "OK",
        400 => "Solicitud invalida",
        404 => "No encontrado",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        code, reason, ctype, body.len()
    );
    write_all(s, head.as_bytes());
    write_all(s, body);
}

fn send_json(s: &mut mio::net::TcpStream, code: u16, body: &str) {
    send_http(s, code, "application/json; charset=utf-8", body.as_bytes());
}

fn synth_text(text: &str) -> Result<Vec<u8>, String> {
    let voice = get_voice_model()?;
    crate::voice::synthesize_wav(&voice, text).map_err(|e| format!("{}", e))
}

fn get_voice_model() -> Result<std::sync::Arc<crate::voice::VoiceModel>, String> {
    static VOICE: std::sync::OnceLock<
        std::sync::Mutex<Option<std::sync::Arc<crate::voice::VoiceModel>>>,
    > = std::sync::OnceLock::new();
    let cell = VOICE.get_or_init(|| std::sync::Mutex::new(None));
    if let Some(m) = cell.lock().unwrap().clone() {
        return Ok(m);
    }
    let m = std::sync::Arc::new(
        crate::voice::build_voice().map_err(|e| e.to_string())?,
    );
    let _ = cell.lock().unwrap().replace(m.clone());
    println!("Voz Piper cargada para el dashboard web");
    Ok(m)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn write_all(s: &mut mio::net::TcpStream, data: &[u8]) {
    let mut off = 0;
    while off < data.len() {
        match s.write(&data[off..]) {
            Ok(n) if n > 0 => off += n,
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(_) => break,
        }
    }
}

fn json_escape(s: &str) -> String {
    s.replace("\\", "\\\\").replace("\"", "\\\"").replace("\n", "\\n")
}

fn extract_json_string(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = body.find(needle.as_str())?;
    let value_start = start + needle.len();
    let bytes = body.as_bytes();
    let mut i = value_start;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            break;
        }
        i += 1;
    }
    let value_bytes = &bytes[value_start..i];
    Some(String::from_utf8_lossy(value_bytes).to_string())
}

fn extract_json_bool(body: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{}\":", key);
    let start = body.find(needle.as_str())?;
    let rest = &body[start + needle.len()..];
    if rest.starts_with("true") {
        return Some(true);
    }
    if rest.starts_with("false") {
        return Some(false);
    }
    None
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut vals = [255u8; 256];
    let alphabet: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for (i, &c) in alphabet.iter().enumerate() {
        vals[c as usize] = i as u8;
    }
    let bytes = input.trim().as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() / 4 * 3 + 3);
    let mut buf = [0u8; 4];
    let mut idx = 0;
    for &b in bytes {
        if b == b'\n' || b == b'\r' || b == b' ' {
            continue;
        }
        if b == b'=' {
            buf[idx] = 0xFF;
            idx += 1;
            if idx == 4 {
                decode_quad(&buf, &mut out);
                idx = 0;
            }
            continue;
        }
        let v = vals[b as usize];
        if v == 255 {
            return None;
        }
        buf[idx] = v;
        idx += 1;
        if idx == 4 {
            decode_quad(&buf, &mut out);
            idx = 0;
        }
    }
    if idx > 1 {
        decode_quad(&buf, &mut out);
    }
    Some(out)
}

fn decode_quad(buf: &[u8; 4], out: &mut Vec<u8>) {
    if buf[0] == 0xFF || buf[1] == 0xFF {
        return;
    }
    out.push((buf[0] << 2) | (buf[1] >> 4));
    if buf[2] != 0xFF {
        out.push((buf[1] << 4) | (buf[2] >> 2));
        if buf[3] != 0xFF {
            out.push((buf[2] << 6) | buf[3]);
        }
    }
}

fn pdf_folder() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/asilva".to_string());
    format!("{}/learn/", home)
}

fn extract_pdf_text(pdf: &[u8]) -> String {
    if let Ok(mut child) = std::process::Command::new("pdftotext")
        .arg("-eol")
        .arg("dos")
        .arg("-")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        let stdin = child.stdin.take();
        if let Some(mut si) = stdin {
            let _ = si.write_all(pdf);
            drop(si);
        }
        if let Ok(output) = child.wait_with_output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).to_string();
                if text.trim().chars().count() >= 10 {
                    return text;
                }
            }
        }
    }
    naive_pdf_text(pdf)
}

fn naive_pdf_text(pdf: &[u8]) -> String {
    let s = String::from_utf8_lossy(pdf);
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    loop {
        let Some(rel) = find_sub(&s[i..], "stream") else { break };
        let mut start = i + rel + "stream".len();
        if start < bytes.len() && (bytes[start] == b'\r' || bytes[start] == b'\n') {
            start += 1;
            if start < bytes.len() && bytes[start - 1] == b'\r' && bytes[start] == b'\n' {
                start += 1;
            }
        }
        let Some(endrel) = find_sub(&s[start..], "endstream") else { break };
        let end = start + endrel;
        let seg = &s[start..end];
        let mut k = 0usize;
        while k < seg.len() {
            if seg.as_bytes()[k] == b'(' {
                if let Some((text, close)) = read_paren_string(seg, k) {
                    if !text.trim().is_empty() {
                        out.push_str(text.trim());
                        out.push(' ');
                    }
                    k = close + 1;
                    continue;
                }
            }
            k += 1;
        }
        i = end + "endstream".len();
    }
    out.trim().to_string()
}

fn read_paren_string(s: &str, open_pos: usize) -> Option<(String, usize)> {
    let b = s.as_bytes();
    let mut j = open_pos + 1;
    let mut out = String::new();
    let mut depth = 0;
    while j < b.len() {
        match b[j] {
            b'\\' => {
                if j + 1 < b.len() {
                    match b[j + 1] {
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'(' => out.push('('),
                        b')' => out.push(')'),
                        b'\\' => out.push('\\'),
                        _ => {}
                    }
                    j += 2;
                } else {
                    j += 1;
                }
            }
            b'(' => {
                depth += 1;
                out.push('(');
                j += 1;
            }
            b')' => {
                if depth == 0 {
                    return Some((out, j));
                }
                depth -= 1;
                out.push(')');
                j += 1;
            }
            _ => {
                out.push(b[j] as char);
                j += 1;
            }
        }
    }
    None
}

fn find_sub(haystack: &str, needle: &str) -> Option<usize> {
    haystack.find(needle)
}

const DASHBOARD_HTML: &str = r###"

<!DOCTYPE html>
<html lang="es">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>SynapseAI - Robot</title>
<style>
  :root { --bg:#0d1117; --panel:#161b22; --f:#e6edf3; --acc:#58a6ff; --ok:#3fb950; --warn:#d29922; --err:#f85149; }
  * { box-sizing:border-box; }
  body { background:var(--bg); color:var(--f); font-family:monospace; margin:0; padding:16px 16px 16px 206px; }
  h1 { font-size:18px; margin:0 0 12px; }
  .bar { display:flex; gap:8px; flex-wrap:wrap; align-items:center; margin-bottom:14px; }
  .panel { background:var(--panel); border:1px solid #30363d; border-radius:8px; padding:12px; }
  .grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(280px,1fr)); gap:14px; }
  .stat b { color:var(--acc); }
  .emoji { font-size:30px; }
  pre#world { background:#0b0f14; border:1px solid #30363d; border-radius:8px; padding:10px; font-size:13px; line-height:1.1; }
  .barw { background:#21262d; border-radius:4px; height:10px; }
  .barf { background:var(--acc); height:10px; border-radius:4px; width:0%; }
  button { background:#21262d; color:var(--f); border:1px solid #30363d; border-radius:6px; padding:6px 10px; cursor:pointer; }
  button:hover { border-color:var(--acc); }
  button.act { background:var(--ok); color:#0d1117; }
  input { background:#0d1117; color:var(--f); border:1px solid #30363d; border-radius:6px; padding:6px; width:100%; }
  .chat { height:180px; overflow-y:auto; font-size:12px; white-space:pre-wrap; }
  .sensorrow { display:flex; align-items:center; gap:8px; }
  table { border-collapse:collapse; width:100%; }
  td,th { border:1px solid #30363d; padding:4px 8px; font-size:12px; }
  .tabs { display:flex; flex-direction:column; gap:4px; margin:0; padding:12px 10px; position:fixed; left:0; top:0; bottom:0; width:190px; background:var(--panel); border-right:1px solid #30363d; overflow-y:auto; }
  .side-brand { font-size:15px; font-weight:bold; color:var(--acc); padding:2px 12px 10px; border-bottom:1px solid #30363d; margin-bottom:8px; }
  .tab { background:transparent; color:var(--f); border:none; border-left:3px solid transparent; border-radius:6px; text-align:left; padding:10px 12px; font-size:13px; }
  .tab:hover { background:#21262d; }
  .tab.act { background:#21262d; border-left-color:var(--acc); color:#ffffff; }
  .regla { display:flex; align-items:center; justify-content:space-between; gap:8px; padding:6px 8px; border:1px solid #30363d; border-radius:6px; margin-bottom:6px; }
  .regla span { color:var(--f); }
  .regla button { color:var(--err); }
  .sec { margin-top:4px; }
  @media (max-width:700px){ .tabs{ position:static; width:100%; flex-direction:row; flex-wrap:wrap; border-right:none; border-bottom:1px solid #30363d; margin-bottom:14px; } .side-brand{ display:none; } body{ padding:16px; } }
</style>
</head>
<body>
<h1>SYNAPSE AI <span id="nombre">-</span></h1>
<div class="bar">
  <span class="emoji" id="emoji">?</span>
  <span>Emocion: <b id="emocion">-</b></span>
  <span>Confianza: <b id="confianza">-</b></span>
  <span>Estres: <b id="estres">-</b></span>
  <span>Energia: <b id="energia">-</b></span>
  <span>Exploracion: <b id="exploracion">-</b></span>
</div>

<nav class="tabs">
  <div class="side-brand">SYNAPSE AI</div>
  <button class="tab act" onclick="showTab('sec_monitoreo', this)">MONITOREO</button>
  <button class="tab" onclick="showTab('sec_ordenes', this)">ORDENES</button>
  <button class="tab" onclick="showTab('sec_cuerpo', this)">CUERPO</button>
  <button class="tab" onclick="showTab('sec_percepcion', this)">PERCEPCION</button>
  <button class="tab" onclick="showTab('sec_conocimiento', this)">CONOCIMIENTO</button>
  <button class="tab" onclick="showTab('sec_reglas', this)">REGLAS</button>
  <button class="tab" onclick="showTab('sec_config', this)">CONFIGURACION</button>
  <button class="tab" onclick="showTab('sec_chat', this)">CHAT</button>
</nav>

<section id="sec_monitoreo" class="sec">
  <div class="grid">
    <div class="panel">
      <h1 style="font-size:14px">MUNDO DEL ROBOT</h1>
      <pre id="world">cargando...</pre>
    </div>

    <div class="panel">
      <h1 style="font-size:14px">TELEMETRIA</h1>
      <table>
        <tr><td>Episodio</td><td><b id="episodio">-</b></td></tr>
        <tr><td>Metas alcanzadas</td><td><b id="total_metas">-</b></td></tr>
        <tr><td>Posicion</td><td><b id="posicion">-</b></td></tr>
        <tr><td>Estado del cerebro</td><td><b id="estados">-</b></td></tr>
        <tr><td>Experiencias</td><td><b id="experiencias">-</b></td></tr>
        <tr><td>Adaptaciones</td><td><b id="adaptaciones">-</b></td></tr>
        <tr><td>Recompensa total</td><td><b id="recompensa">-</b></td></tr>
      </table>
    </div>

    <div class="panel">
      <h1 style="font-size:14px">SENSORES</h1>
      <div id="sensores">cargando...</div>
    </div>
  </div>
</section>

<section id="sec_ordenes" class="sec" hidden>
  <div class="grid">
    <div class="panel">
      <h1 style="font-size:14px">ORDENES (palabra de activacion)</h1>
      <div>
        <button onclick="cmd('adelante')">Adelante</button>
        <button onclick="cmd('atras')">Atras</button>
        <button onclick="cmd('izquierda')">Izquierda</button>
        <button onclick="cmd('derecha')">Derecha</button>
        <button onclick="cmd('stop')">Stop</button>
        <button class="act" onclick="cmd('pausa')">Pausa</button>
        <button class="act" onclick="cmd('reanudar')">Reanudar</button>
      </div>
      <div style="margin-top:8px">
        <button onclick="cmd('ayuda')">Ayuda</button>
        <button onclick="cmd('estado')">Estado</button>
        <button onclick="cmd('diagnostico')">Diagnostico</button>
      </div>
      <div style="margin-top:8px">
        <input id="train_n" type="number" value="50" min="1" style="width:70px" placeholder="n">
        <button onclick="cmdText('entrenar '+document.getElementById('train_n').value)">Entrenar</button>
      </div>
    </div>
  </div>
</section>

<section id="sec_cuerpo" class="sec" hidden>
  <div class="grid">
    <div class="panel">
      <h1 style="font-size:14px">CUERPO (actuadores)</h1>
      <table>
        <tr><td>Motor izquierdo</td><td><b id="body_motor_izq">-</b></td></tr>
        <tr><td>Motor derecho</td><td><b id="body_motor_der">-</b></td></tr>
        <tr><td>Servo cuello</td><td><b id="body_servo">-</b></td></tr>
      </table>
      <div style="margin-top:12px">
        <label>Angulo del cuello: <b id="cabeza_val">90</b> grados</label>
        <input type="range" id="cabeza_slider" min="0" max="180" value="90"
               oninput="document.getElementById('cabeza_val').textContent=this.value" style="width:100%">
        <div style="margin-top:6px">
          <button class="act" onclick="cmdText('mueve cabeza a '+document.getElementById('cabeza_slider').value)">Mover cuello</button>
        </div>
      </div>
      <div style="margin-top:12px">
        <button onclick="cmd('adelante')">Adelante</button>
        <button onclick="cmd('atras')">Atras</button>
        <button onclick="cmd('izquierda')">Izquierda</button>
        <button onclick="cmd('derecha')">Derecha</button>
        <button onclick="cmd('stop')">Stop</button>
        <button class="act" onclick="cmd('pausa')">Pausa</button>
        <button class="act" onclick="cmd('reanudar')">Reanudar</button>
      </div>
    </div>
  </div>
</section>

<section id="sec_percepcion" class="sec" hidden>
  <div class="grid">
    <div class="panel">
      <h1 style="font-size:14px">VISION (ver)</h1>
      <button class="act" onclick="cmd('que ves')">Ver ahora</button>
      <div style="margin-top:12px">
        <div class="sensorrow"><span>Brillo</span><div class="barw"><div class="barf" id="vista_brillo_bar"></div></div><span id="vista_brillo">-</span></div>
        <div class="sensorrow"><span>Movimiento</span><div class="barw"><div class="barf" id="vista_mov_bar"></div></div><span id="vista_mov">-</span></div>
      </div>
      <div id="vista_texto" style="margin-top:8px;font-size:12px;color:#8b949e">Todavia no he mirado.</div>
    </div>

    <div class="panel">
      <h1 style="font-size:14px">AUDICION (escuchar)</h1>
      <button class="act" onclick="cmd('escucha')">Escuchar ahora</button>
      <div style="margin-top:12px">
        <div class="sensorrow"><span>Nivel sonido</span><div class="barw"><div class="barf" id="oido_nivel_bar"></div></div><span id="oido_nivel">-</span></div>
        <div class="sensorrow"><span>Voz detectada</span><span id="oido_voz">-</span></div>
      </div>
      <div id="oido_texto" style="margin-top:8px;font-size:12px;color:#8b949e">Todavia no he escuchado.</div>
    </div>
  </div>
</section>

<section id="sec_conocimiento" class="sec" hidden>
  <div class="grid">
    <div class="panel">
      <h1 style="font-size:14px">ENSENANZA</h1>
      <div style="margin-bottom:6px">
        <label>Frase a ensenar</label>
        <input id="learn_phrase" type="text" placeholder="ej: hola">
      </div>
      <div style="margin-bottom:6px">
        <label>Significado</label>
        <input id="learn_meaning" type="text" placeholder="ej: saludo">
      </div>
      <button onclick="learnRobot()">Ensenar al robot</button>
    </div>

    <div class="panel">
      <h1 style="font-size:14px">SUBIR PDF (material de estudio)</h1>
      <div style="margin-bottom:6px">
        <input id="pdf_file" type="file" accept="application/pdf">
      </div>
      <button onclick="uploadPDF()">Enviar PDF al robot</button>
      <div id="pdf_result" style="font-size:12px;color:var(--ok);margin-top:6px"></div>
    </div>

    <div class="panel">
      <h1 style="font-size:14px">MEMORIA</h1>
      <div style="margin-bottom:8px">
        <button onclick="cmdText('que sabes')">Que sabes?</button>
        <button onclick="refreshAprendido()">Refrescar memoria</button>
      </div>
      <div class="chat" id="learned_resumen" style="height:120px">cargando...</div>
    </div>
  </div>
</section>

<section id="sec_reglas" class="sec" hidden>
  <div class="grid">
    <div class="panel">
      <h1 style="font-size:14px">REGLAS ABSOLUTAS (no pueden violarse jamas)</h1>
      <p style="font-size:12px;color:#8b949e;margin:0 0 10px">
        Escribe una regla que el robot NO pueda cumplir bajo ningun precepto, por ejemplo
        "nunca te muevas hacia la izquierda" o "no puedes girar a la derecha".
        Se aplica a los movimientos manuales, a las ordenes del chat y a los movimientos automaticos del entrenamiento.
      </p>
      <div style="display:flex;gap:8px;margin-bottom:10px">
        <input id="regla_texto" placeholder="ej: nunca te muevas hacia la izquierda">
        <button class="act" onclick="addRegla()">Agregar regla</button>
      </div>
      <div id="reglas_lista" style="font-size:12px">Sin reglas registradas.</div>
    </div>
  </div>
</section>

<section id="sec_config" class="sec" hidden>
  <div class="grid">
    <div class="panel">
      <h1 style="font-size:14px">CONFIGURACION</h1>
      <div style="margin-bottom:6px"><label>Nombre del robot</label>
        <input id="cfg_nombre" placeholder="nombre"></div>
      <div style="margin-bottom:6px"><label>Palabra de activacion</label>
        <input id="cfg_activacion" placeholder="palabra"></div>
      <button onclick="guardarConfig()">Guardar configuracion</button>
    </div>

    <div class="panel">
      <h1 style="font-size:14px">VOZ (femenina, offline)</h1>
      <div style="margin-bottom:8px">
        <label><input type="checkbox" id="voz_activa" checked onchange="guardarVoz()"> Voz activada</label>
      </div>
      <div style="margin-bottom:8px"><label>Voz</label>
        <select id="voz_sel" onchange="guardarVoz()"></select></div>
      <div style="margin-bottom:8px"><label>Texto de prueba</label>
        <input id="voz_texto" placeholder="Hola, soy tu robot"></div>
      <button class="act" onclick="probarVoz()">Hablar</button>
      <span id="voz_estado" style="margin-left:8px;color:var(--ok)"></span>
    </div>
  </div>
</section>

<section id="sec_chat" class="sec" hidden>
  <div class="grid">
    <div class="panel">
      <h1 style="font-size:14px">CHAT / MENSAJES</h1>
      <div class="chat" id="chat"></div>
      <form onsubmit="enviar(); return false;">
        <input id="msg" placeholder="Escribe tu orden (ej: synapse adelante)">
        <button class="act" type="submit">Enviar</button>
      </form>
    </div>
  </div>
</section>

<script>
let wake = '';
let lastMsg = '';
function log(msg){ const c=document.getElementById('chat'); c.textContent += msg+'\n'; c.scrollTop=c.scrollHeight; }
function jstr(s){ return JSON.stringify(s); }
function cmd(c){ enviarTexto(wake + ' ' + c); }
function cmdText(t){ enviarTexto(wake + ' ' + t); }
function enviar(){ const m=document.getElementById('msg').value; enviarTexto(m); document.getElementById('msg').value=''; }
function showTab(id, btn){
  document.querySelectorAll('.sec').forEach(function(s){ s.hidden = (s.id !== id); });
  document.querySelectorAll('.tab').forEach(function(t){ t.classList.remove('act'); });
  if(btn) btn.classList.add('act');
  if(id === 'sec_reglas') renderRules();
  if(id === 'sec_conocimiento') refreshAprendido();
}
async function renderRules(){
  const r = await fetch('/api/rules').then(x=>x.json()).catch(()=>({reglas:[]}));
  const el = document.getElementById('reglas_lista');
  el.textContent = '';
  if(!r.reglas || !r.reglas.length){ el.textContent = 'Sin reglas registradas.'; return; }
  r.reglas.forEach(function(t, i){
    const div = document.createElement('div');
    div.className = 'regla';
    const span = document.createElement('span');
    span.textContent = (i+1) + '. ' + t;
    const btn = document.createElement('button');
    btn.textContent = 'Quitar';
    btn.onclick = function(){ quitarRegla(t); };
    div.appendChild(span);
    div.appendChild(btn);
    el.appendChild(div);
  });
}
async function addRegla(){
  const inp = document.getElementById('regla_texto');
  const t = inp.value.trim();
  if(!t) return;
  const r = await fetch('/api/rules', {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({accion:'agregar', texto:t})});
  const d = await r.json().catch(()=>({}));
  log('< regla: ' + (d.ok ? t : (d.error||'error')));
  inp.value = '';
  renderRules();
}
async function quitarRegla(t){
  await fetch('/api/rules', {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({accion:'quitar', texto:t})});
  renderRules();
}
async function refreshAprendido(){
  try{
    const r = await fetch('/api/learned'); if(!r.ok) return;
    const d = await r.json();
    const el = document.getElementById('learned_resumen');
    if(el) el.textContent = d.aprendido || 'Sin contenido aun.';
  }catch(e){}
}
async function enviarTexto(t){
  if(!t) return;
  log('> '+t);
  const r = await fetch('/api/command',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({text:t})});
  const d = await r.json();
  log('< ok='+d.ok);
}
async function guardarConfig(){
  const body = {name:document.getElementById('cfg_nombre').value, wake_word:document.getElementById('cfg_activacion').value};
  await fetch('/api/config',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  log('< configuracion guardada');
}
async function learnRobot(){
  const phrase = document.getElementById('learn_phrase').value;
  const meaning = document.getElementById('learn_meaning').value;
  if(!phrase || !meaning) return;
  await fetch('/api/learn', {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({phrase, meaning})});
  log('< frase aprendida');
  // Limpiar inputs
  document.getElementById('learn_phrase').value = '';
  document.getElementById('learn_meaning').value = '';
}
async function uploadPDF(){
  const fi = document.getElementById('pdf_file');
  const out = document.getElementById('pdf_result');
  if(!fi.files || !fi.files.length){ out.textContent='Selecciona un PDF.'; return; }
  const f = fi.files[0];
  if(f.size > 4*1024*1024){ out.textContent='Maximo 4 MB.'; return; }
  out.textContent='Leyendo PDF...';
  try{
    const buf = await f.arrayBuffer();
    const bytes = new Uint8Array(buf);
    let bin='';
    const chunk=0x8000;
    for(let i=0;i<bytes.length;i+=chunk){
      bin += String.fromCharCode.apply(null, bytes.subarray(i, i+chunk));
    }
    const b64 = btoa(bin);
    out.textContent='Enviando ('+(f.size/1024).toFixed(0)+' KB)...';
    const r = await fetch('/api/upload-pdf', {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({name:f.name, pdf_base64:b64})});
    const j = await r.json();
    if(j.ok){
      out.textContent = j.texto_len>0 ? ('OK: '+j.nombre+' - '+j.texto_len+' caracteres extraidos') : ('Recibido: '+j.nombre+' - '+ (j.msg||'sin texto'));
      log('< pdf enviado: '+j.nombre);
    } else {
      out.textContent = j.error || 'Error al enviar PDF';
    }
  }catch(e){
    out.textContent = 'Error: '+e.message;
  }
}
async function refresh(){
  try{
    const r = await fetch('/api/status'); if(!r.ok) return;
    const d = await r.json();
    document.getElementById('nombre').textContent = d.nombre;
    document.getElementById('emocion').textContent = d.emocion;
    document.getElementById('confianza').textContent = d.confianza+'%';
    document.getElementById('estres').textContent = d.estres+'%';
    document.getElementById('energia').textContent = d.energia+'%';
    document.getElementById('exploracion').textContent = (d.explotacion*100).toFixed(0)+'%';
    document.getElementById('emoji').textContent = d.emoji;
    document.getElementById('episodio').textContent = d.episodio;
    document.getElementById('total_metas').textContent = d.total_metas;
    document.getElementById('posicion').textContent = d.posicion;
    document.getElementById('estados').textContent = d.estados;
    document.getElementById('experiencias').textContent = d.experiencias;
    document.getElementById('adaptaciones').textContent = d.adaptaciones;
    document.getElementById('recompensa').textContent = d.recompensa;
    document.getElementById('world').textContent = d.mundo;
    // sensores
    let sh='';
    for(const k in d.sensores){ const v=d.sensores[k]; sh+='<div class="sensorrow"><span>'+k+'</span><div class="barw"><div class="barf" style="width:'+(v*100)+'%"></div></div><span>'+Math.round(v*100)+'%</span></div>'; }
    document.getElementById('sensores').innerHTML = sh;
    // cuerpo (actuadores)
    if(d.motor_izq!=null){ document.getElementById('body_motor_izq').textContent = (d.motor_izq>0?'+':'')+d.motor_izq.toFixed(2); }
    if(d.motor_der!=null){ document.getElementById('body_motor_der').textContent = (d.motor_der>0?'+':'')+d.motor_der.toFixed(2); }
    if(d.servo_cabezal!=null){
      const cab = Math.round(d.servo_cabezal);
      document.getElementById('body_servo').textContent = cab+'°';
      const sl = document.getElementById('cabeza_slider');
      if(document.activeElement !== sl){ sl.value = cab; document.getElementById('cabeza_val').textContent = cab; }
    }
    // percepcion: vision
    if(d.vista_brillo!=null){
      const vb = Math.round(d.vista_brillo*100);
      document.getElementById('vista_brillo').textContent = vb+'%';
      document.getElementById('vista_brillo_bar').style.width = (d.vista_brillo*100)+'%';
    }
    if(d.vista_movimiento!=null){
      const vm = Math.round(d.vista_movimiento*100);
      document.getElementById('vista_mov').textContent = vm+'%';
      document.getElementById('vista_mov_bar').style.width = (d.vista_movimiento*100)+'%';
    }
    if(d.vista_texto){ document.getElementById('vista_texto').textContent = d.vista_texto; }
    // percepcion: audicion
    if(d.oido_nivel!=null){
      const on = Math.round(d.oido_nivel*100);
      document.getElementById('oido_nivel').textContent = on+'%';
      document.getElementById('oido_nivel_bar').style.width = (d.oido_nivel*100)+'%';
    }
    if(d.oido_voz!=null){ document.getElementById('oido_voz').textContent = d.oido_voz ? 'SI' : 'no'; }
    if(d.oido_texto){ document.getElementById('oido_texto').textContent = d.oido_texto; }
    // mensaje del robot
    if(d.mensaje && d.mensaje.length){
      log(d.mensaje);
      if(document.getElementById('voz_activa').checked && d.mensaje!==lastMsg){
        lastMsg=d.mensaje;
        playWavText(d.mensaje);
      }
    }
  }catch(e){}
}
async function cargarConfig(){
  const r=await fetch('/api/config'); const d=await r.json();
  document.getElementById('cfg_nombre').value=d.nombre;
  document.getElementById('cfg_activacion').value=d.activacion;
  wake=d.activacion;
  // voz
  const sel=document.getElementById('voz_sel');
  sel.innerHTML='';
  (d.voces||[]).forEach(function(v){
    const o=document.createElement('option');
    o.value=v; o.textContent=v;
    if(v===d.voz) o.selected=true;
    sel.appendChild(o);
  });
  document.getElementById('voz_activa').checked = !!d.voz_activa;
}
async function guardarVoz(){
  const body = {activa:document.getElementById('voz_activa').checked, voz:document.getElementById('voz_sel').value};
  await fetch('/api/voice',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  document.getElementById('voz_estado').textContent='voz actualizada';
  setTimeout(function(){ document.getElementById('voz_estado').textContent=''; },2000);
}
async function probarVoz(){
  const t=document.getElementById('voz_texto').value || 'Hola, soy tu robot, probando mi voz.';
  document.getElementById('voz_estado').textContent='sintetizando...';
  const ok=await playWavText(t);
  document.getElementById('voz_estado').textContent = ok ? 'hablando' : 'error';
  if(ok) setTimeout(function(){ document.getElementById('voz_estado').textContent=''; },4000);
}
async function playWavText(t){
  try{
    const r=await fetch('/api/speak.wav?text='+encodeURIComponent(t));
    if(!r.ok) return false;
    const blob=await r.blob();
    const u=URL.createObjectURL(blob);
    const a=new Audio(u);
    await a.play();
    return true;
  }catch(e){ return false; }
}
setInterval(refresh, 1200);
refresh(); cargarConfig(); renderRules(); refreshAprendido();
</script>
</body>
</html>
"###;
