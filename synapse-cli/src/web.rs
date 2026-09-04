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
        if attempts > 1500 {
            break;
        }
        if need > 0 && buf.len() >= need {
            break;
        }

        let mut chunk = [0u8; 4096];
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
  body { background:var(--bg); color:var(--f); font-family:monospace; margin:0; padding:16px; }
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

<div class="panel" style="margin-top:14px">
  <h1 style="font-size:14px">CHAT / MENSAJES</h1>
  <div class="chat" id="chat"></div>
  <form onsubmit="enviar(); return false;">
    <input id="msg" placeholder="Escribe tu orden (ej: synapse adelante)">
    <button class="act" type="submit">Enviar</button>
  </form>
</div>

<script>
let wake = '';
let lastMsg = '';
function log(msg){ const c=document.getElementById('chat'); c.textContent += msg+'\n'; c.scrollTop=c.scrollHeight; }
function jstr(s){ return JSON.stringify(s); }
function cmd(c){ enviarTexto(wake + ' ' + c); }
function cmdText(t){ enviarTexto(wake + ' ' + t); }
function enviar(){ const m=document.getElementById('msg').value; enviarTexto(m); document.getElementById('msg').value=''; }
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
async function refresh(){
  try{
    const r = await fetch('/api/status'); if(!r.ok) return;
    const d = await r.json();
    document.getElementById('nombre').textContent = d.nombre;
    document.getElementById('emocion').textContent = d.emocion;
    document.getElementById('confianza').textContent = d.confianza+'%';
    document.getElementById('estres').textContent = d.estres+'%';
    document.getElementById('energia').textContent = d.energia+'%';
    document.getElementById('exploracion').textContent = d.exploracion+'%';
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
refresh(); cargarConfig();
</script>
</body>
</html>
"###;
