use synapse_core::brain::Action;

pub enum WebCommand {
    Ignore,
    Greeting,
    Action(Action),
    Pause,
    Resume,
    Train(u64),
    SetName(String),
    SetWakeWord(String),
    DoStatus,
    DoDiagnostic,
    Help,
    Unknown,
}

pub fn parse_web_command(input: &str, wake_word: &str, grace_active: bool) -> WebCommand {
    let normalized = normalize(input);
    if normalized.is_empty() {
        return WebCommand::Ignore;
    }

    let wake = normalize(wake_word);
    let has_wake = !wake.is_empty() && normalized.contains(&wake);

    if !wake.is_empty() && !has_wake && !grace_active && !is_direct_command(&normalized) {
        return WebCommand::Ignore;
    }

    let mut text = normalized;
    if has_wake {
        text = text.replace(&wake, " ").trim().to_string();
    }

    if text.is_empty() {
        return WebCommand::Greeting;
    }

    if is_mention_expr(&text) {
        return WebCommand::Greeting;
    }

    // ORDEN DE MOVIMIENTO
    if let Some(action) = detect_action(&text) {
        return WebCommand::Action(action);
    }

    // PAUSA / REANUDAR
    if contains_any(&text, &["pausa", "pausar", "pause", "detenerte", "deten", "espera", "quieto"]) {
        return WebCommand::Pause;
    }
    if contains_any(&text, &["reanuda", "reanudar", "resume", "sigueme", "seguir", "continua", "sigue"]) {
        return WebCommand::Resume;
    }

    // ESTADO / DIAGNOSTICO / AYUDA
    if contains_any(&text, &["estado", "status", "como estas", "que tal", "telemetria"]) {
        return WebCommand::DoStatus;
    }
    if contains_any(&text, &["diagnostico", "diagnostic", "salud", "revision", "health", "revisa"]) {
        return WebCommand::DoDiagnostic;
    }
    if contains_any(&text, &["ayuda", "help", "comandos", "que puedes hacer", "opciones"]) {
        return WebCommand::Help;
    }

    // ENTRENAR con numero opcional
    if let Some(n) = detect_number(&text) {
        if contains_any(&text, &["entrenar", "entrena", "train", "aprendizaje", "aprende", "adestrar", "edutacion"]) {
            return WebCommand::Train(n);
        }
    } else if contains_any(&text, &["entrenar", "entrena", "train", "aprende", "aprendizaje", "adestrar"]) {
        return WebCommand::Train(50);
    }

    // CAMBIAR NOMBRE
    if let Some(name) = detect_name(&text) {
        return WebCommand::SetName(name);
    }

    // CAMBIAR PALABRA DE ACTIVACION
    if let Some(w) = detect_wake(&text) {
        return WebCommand::SetWakeWord(w);
    }

    WebCommand::Unknown
}

fn detect_action(text: &str) -> Option<Action> {
    let forward = ["adelante", "avanza", "avanzar", "asi adelante", "hacia adelante", "sigue recto", "sigue derecho", "tira pa adelante", "avance", "avanzamen", "pa adelante", "para adelante", "forward", "move forward", "go forward", "recto", "recta", "derechito", "ven aqui", "acercate"];
    let backward = ["atras", "reversa", "retrocede", "retroceder", "hacia atras", "pa atras", "para atras", "retrocede un poco", "backward", "move back", "go back", "reversa lenta", "echate para atras", "vuelve"];
    let left = ["izquierda", "izq", "a la izquierda", "gira a la izquierda", "torce a la izquierda", "vira a la izquierda", "da vuelta a la izquierda", "left", "turn left", "go left", "rota izquierda"];
    let right = ["derecha", "der", "a la derecha", "gira a la derecha", "torce a la derecha", "vira a la derecha", "da vuelta a la derecha", "right", "turn right", "go right", "rota derecha"];
    let stop = ["stop", "para", "parar", "alto", "frenar", "frena", "detente", "detener", "quiero que pares", "por favor para"];

    // Primero las acciones mas restringidas para evitar falsos positivos
    if contains_any(text, &stop) {
        return Some(Action::Stop);
    }
    if contains_any(text, &left) {
        return Some(Action::TurnLeft);
    }
    if contains_any(text, &right) {
        return Some(Action::TurnRight);
    }
    if contains_any(text, &backward) {
        return Some(Action::Backward);
    }
    if contains_any(text, &forward) {
        return Some(Action::Forward);
    }
    None
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    for n in needles {
        if text.contains(n) {
            return true;
        }
    }
    false
}

fn detect_number(text: &str) -> Option<u64> {
    let words: Vec<&str> = text.split_whitespace().collect();
    for w in words {
        if let Ok(n) = w.parse::<u64>() {
            if n > 0 {
                return Some(n);
            }
        }
    }
    None
}

fn detect_name(text: &str) -> Option<String> {
    let triggers = ["llamame", "llamate", "te llamas", "te puedes llamar", "cambiate el nombre a", "cambia tu nombre a", "nombre", "name"];
    for t in triggers {
        if let Some(idx) = text.find(t) {
            let rest = text[idx + t.len()..].trim();
            if rest.is_empty() {
                continue;
            }
            let cleaned = clean_tail(rest);
            if !cleaned.is_empty() && is_plain_word(&cleaned) {
                return Some(first_word(&cleaned));
            }
        }
    }
    None
}

fn detect_wake(text: &str) -> Option<String> {
    let triggers = ["palabra de activacion", "palabra clave", "wake word", "activacion", "wake", "palabra magica", "dime que te active", "decime que te active"];
    for t in triggers {
        if let Some(idx) = text.find(t) {
            let rest = text[idx + t.len()..].trim();
            if rest.is_empty() {
                continue;
            }
            let cleaned = clean_tail(rest);
            if !cleaned.is_empty() {
                return Some(first_word(&cleaned));
            }
        }
    }
    None
}

fn normalize(input: &str) -> String {
    let lower = lower(input);
    let mut out = String::new();
    let mut prev_space = false;
    for ch in lower.chars() {
        if ch.is_ascii_punctuation() || ch.is_ascii_control() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

fn clean_tail(s: &str) -> String {
    let mut cleaned = String::new();
    let mut prev_space = false;
    for ch in s.trim().chars() {
        if ch.is_ascii_punctuation() {
            continue;
        }
        if ch.is_whitespace() {
            if !prev_space {
                cleaned.push(' ');
                prev_space = true;
            }
        } else {
            cleaned.push(ch);
            prev_space = false;
        }
    }
    cleaned
}

fn first_word(s: &str) -> String {
    s.split_whitespace().next().unwrap_or_default().to_string()
}

fn is_plain_word(s: &str) -> bool {
    let forbidden = ["que", "por", "pa", "para", "el", "la", "los", "las", "de", "del", "como", "un", "una", "se", "a", "al", "lo", "es"];
    let first = s.split_whitespace().next().unwrap_or_default();
    if forbidden.contains(&first) || first.is_empty() {
        return false;
    }
    true
}

fn is_mention_expr(text: &str) -> bool {
    let low = text;
    low.contains("oye")
        || low.contains("hey")
        || low.contains("ai")
        || low.contains("presentate")
        || low.contains("quien eres")
        || low.contains("hola")
        || low.contains("buenas")
        || low.contains("saluda")
}

fn is_direct_command(text: &str) -> bool {
    [
        "adelante", "avanza", "avanzar", "atras", "reversa", "retrocede", "izquierda",
        "izq", "derecha", "der", "stop", "para", "parar", "alto", "pausa", "pausar",
        "reanudar", "resume", "estado", "status", "diagnostico", "diagnostic", "ayuda",
        "help", "entrenar", "entrena", "train", "aprende", "nombre", "llamame", "activacion",
        "wake", "jugosa",
    ]
    .iter()
    .any(|w| text.contains(w))
}

fn lower(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        let lower_b = if b >= b'A' && b <= b'Z' {
            b + 32
        } else {
            b
        };
        out.push(lower_b as char);
    }
    out
}
