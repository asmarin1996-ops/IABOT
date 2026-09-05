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
    Learn(String, String), // phrase, meaning
    DoKnowledge,
    DoStatus,
    DoDiagnostic,
    Help,
    Think(String),
    SetBrain(bool),
    MoveServo(String, f64), // nombre del actuador, angulo en grados
    QueVes,
    Escucha,
    Unknown,
}

pub fn parse_web_command(input: &str, wake_word: &str, grace_active: bool) -> WebCommand {
    let normalized = normalize(input);
    if normalized.is_empty() {
        return WebCommand::Ignore;
    }

    let wake = normalize(wake_word);
    let has_wake = !wake.is_empty() && normalized.contains(&wake);

    if !wake.is_empty() && !has_wake && !grace_active && !is_direct_command(&fold_accents(&normalized)) {
        return WebCommand::Ignore;
    }

    let mut text = normalized;
    if has_wake {
        text = text.replace(&wake, " ").trim().to_string();
    }

    if text.is_empty() {
        return WebCommand::Greeting;
    }

    let detect = fold_accents(&text);

    if is_mention_expr(&detect) {
        return WebCommand::Greeting;
    }

    // MOVIMIENTO DE UN SERVO POR NOMBRE: "mueve cabeza a 90 grados"
    // (antes que las ordenes de giro: "mueve motor izquierdo a 1" no debe
    // interpretarse como girar a la izquierda)
    let manipula = contains_any(&detect, &["mueve", "mover", "angulo", "punto a", "posicion"]);
    let mide_grados = contains_any(&detect, &["grados"])
        && !contains_any(&detect, &["gira", "girar", "vira", "giro", "dobla", "ve a la izquierda", "ve a la derecha"]);
    let habla_del_robot = detect.contains("robot") || detect.contains("movete");
    if (manipula || mide_grados) && !habla_del_robot {
        if let Some(servo) = detect_servo(&detect) {
            if let Some(ang) = detect_number(&detect) {
                return WebCommand::MoveServo(servo.to_string(), ang as f64);
            }
        }
    }

    // PERCEPCION: ver y escuchar
    if contains_any(
        &detect,
        &[
            "que ves", "que veo", "ves algo", "que miras", "que estas viendo",
            "que esta viendo", "que se ve", "como se ve", "camara", "vista",
        ],
    ) {
        return WebCommand::QueVes;
    }
    if contains_any(
        &detect,
        &["escucha", "escuchar", "que oyes", "que oigo", "oyes algo", "que se escucha", "microfono", "audio"],
    ) {
        return WebCommand::Escucha;
    }

    // ORDEN DE MOVIMIENTO
    if let Some(action) = detect_action(&detect) {
        return WebCommand::Action(action);
    }

    // PAUSA / REANUDAR
    if contains_any(&detect, &["pausa", "pausar", "pause", "detenerte", "deten", "espera", "quieto"]) {
        return WebCommand::Pause;
    }
    if contains_any(&detect, &["reanuda", "reanudar", "resume", "sigueme", "seguir", "continua", "sigue"]) {
        return WebCommand::Resume;
    }

    // ESTADO / DIAGNOSTICO / AYUDA
    if has_word(&detect, "estado")
        || has_word(&detect, "status")
        || has_word(&detect, "telemetria")
        || contains_any(&detect, &["como estas", "que tal"])
    {
        return WebCommand::DoStatus;
    }
    if has_word(&detect, "diagnostico")
        || has_word(&detect, "diagnostic")
        || has_word(&detect, "health")
        || has_word(&detect, "salud")
        || contains_any(&detect, &["revision", "revisa", "revisar"])
    {
        return WebCommand::DoDiagnostic;
    }
    if contains_any(&detect, &["ayuda", "help", "comandos", "que puedes hacer", "opciones"]) {
        return WebCommand::Help;
    }

    // PATRÓN DE APRENDIZAJE (antes que ENTRENAR para no capturarlo como train)
    if let Some(pos) = text.find("aprende que") {
        let rest = text[pos + "aprende que".len()..].trim();
        // Formato: "aprende que X es Y"
        if let Some(pos_es) = rest.find(" es ") {
            let phrase = rest[..pos_es].trim();
            let after_es = rest[pos_es + "es ".len()..].trim();
            let meaning = if let Some(pos_para) = after_es.find(" para ") {
                after_es[..pos_para].trim().to_string()
            } else {
                after_es.to_string()
            };
            if !phrase.is_empty() && !meaning.is_empty() {
                return WebCommand::Learn(phrase.to_string(), meaning);
            }
        }
    }

    // Also check "remember that"
    if let Some(pos) = text.find("remember that") {
        let rest = text[pos + "remember that".len()..].trim();
        if let Some(pos_is) = rest.find(" is ") {
            let phrase = rest[..pos_is].trim();
            let meaning = if let Some(pos_para) = rest[pos_is + "is ".len()..].find(" para ") {
                rest[pos_is + "is ".len()..pos_is + "is ".len() + pos_para].trim().to_string()
            } else {
                rest[pos_is + "is ".len()..].trim().to_string()
            };
            if !phrase.is_empty() && !meaning.is_empty() {
                return WebCommand::Learn(phrase.to_string(), meaning);
            }
        }
    }

    // QUE SABES / CONOCIMIENTO APRENDIDO
    if contains_any(&detect, &["que sabes", "que aprendiste", "que has aprendido", "que sabemos", "que me ensenaste", "que dice el material", "lee el material", "que aprendiste del pdf", "que hay en el pdf"]) {
        return WebCommand::DoKnowledge;
    }

    // CEREBRO (LLM) ON/OFF
    if contains_any(&detect, &["cerebro on", "cerebro activo", "activa el cerebro", "activa llm"]) {
        return WebCommand::SetBrain(true);
    }
    if contains_any(&detect, &["cerebro off", "cerebro apagado", "desactiva el cerebro", "desactiva llm"]) {
        return WebCommand::SetBrain(false);
    }

    // CONVERSACION LIBRE / REFLEXION VIA LLM
    if contains_any(
        &detect,
        &[
            "piensa que",
            "en que piensas",
            "que piensas de",
            "que piensas",
            "conversemos",
            "conversa",
            "conversar",
            "hablemos",
            "hablame de",
            "charlar",
            "dime algo",
        ],
    ) {
        return WebCommand::Think(text.clone());
    }

    // ENTRENAR con numero opcional
    if let Some(n) = detect_number(&detect) {
        if contains_any(&detect, &["entrenar", "entrena", "train", "aprendizaje", "aprende", "adestrar", "edutacion"]) {
            return WebCommand::Train(n);
        }
    } else if contains_any(&detect, &["entrenar", "entrena", "train", "aprendizaje", "adestrar"]) {
        return WebCommand::Train(50);
    }

    // CAMBIAR NOMBRE
    if let Some(name) = detect_name(&text) {
        return WebCommand::SetName(name);
    }

    // CAMBIAR PALABRA DE ACTIVACION
    if let Some(w) = detect_wake(&detect) {
        return WebCommand::SetWakeWord(w);
    }

WebCommand::Unknown
}

fn detect_action(text: &str) -> Option<Action> {
    let forward = ["adelante", "avanza", "avanzar", "asi adelante", "hacia adelante", "sigue recto", "sigue derecho", "tira pa adelante", "avance", "avanzamen", "pa adelante", "para adelante", "forward", "move forward", "go forward", "recto", "recta", "derechito", "ven aqui", "acercate"];
    let backward = ["atras", "reversa", "retrocede", "retroceder", "hacia atras", "pa atras", "para atras", "retrocede un poco", "backward", "move back", "go back", "reversa lenta", "echate para atras", "vuelve"];
    let left = ["izquierda", "izq", "a la izquierda", "gira a la izquierda", "torce a la izquierda", "vira a la izquierda", "da vuelta a la izquierda", "left", "turn left", "go left", "rota izquierda"];
    let right = ["derecha", "der", "a la derecha", "gira a la derecha", "torce a la derecha", "vira a la derecha", "da vuelta a la derecha", "right", "turn right", "go right", "rota derecha"];
    let stop = ["stop", "para", "parar", "alto", "frenar", "frena", "detente", "detener", "detenerte", "quiero que pares", "para ya", "por favor para", "frena ya", "para de moverte"];

    // Primero las acciones mas restringidas para evitar falsos positivos
    let short_cmd = text.split_whitespace().count() <= 4;
    if stop
        .iter()
        .any(|w| w.split_whitespace().count() > 1 && text.contains(w))
        || (short_cmd && stop.iter().any(|w| w.split_whitespace().count() == 1 && has_word(text, w)))
    {
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

fn has_word(text: &str, word: &str) -> bool {
    text.split_whitespace().any(|t| t == word)
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

fn detect_servo(text: &str) -> Option<&'static str> {
    let names = [
        ("brazo izquierdo", "servo_brazo_izq"),
        ("brazo derecho", "servo_brazo_der"),
        ("pierna izquierda", "servo_pierna_izq"),
        ("pierna derecha", "servo_pierna_der"),
        ("motor izquierdo", "motor_izq"),
        ("motor derecho", "motor_der"),
        ("brazo_izq", "servo_brazo_izq"),
        ("brazo_der", "servo_brazo_der"),
        ("pierna_izq", "servo_pierna_izq"),
        ("pierna_der", "servo_pierna_der"),
        ("cabezal", "servo_cabezal"),
        ("cabeza", "servo_cabezal"),
        ("cuello", "servo_cabezal"),
        ("izquierda", "motor_izq"),
        ("izq", "motor_izq"),
        ("derecha", "motor_der"),
        ("der", "motor_der"),
        ("brazo", "servo_brazo_izq"),
        ("pierna", "servo_pierna_izq"),
        ("cola", "servo_cola"),
    ];
    for (word, canonical) in names {
        if text.contains(word) {
            return Some(canonical);
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

fn fold_accents(input: &str) -> String {
    let mut out = String::new();
    for c in input.chars() {
        let lc = c.to_lowercase().next().unwrap_or(c);
        let folded = match lc {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' => 'u',
            'ü' => 'u',
            'ñ' => 'n',
            _ => lc,
        };
        out.push(folded);
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
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let single = ["hola", "buenas", "buenos", "hey", "eai", "ai", "oye", "harmonia", "cortesia"];
    if tokens.len() <= 3 {
        for t in &tokens {
            for g in &single {
                if t.eq_ignore_ascii_case(g) {
                    return true;
                }
            }
        }
        let phrase = tokens.join(" ");
        return phrase.contains("que hay")
            || phrase.contains("que hubo")
            || phrase.contains("buenos dias")
            || phrase.contains("buenas tardes")
            || phrase.contains("buenas noches")
            || phrase.contains("quien eres")
            || phrase.contains("presentate");
    }
    false
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
    s.chars().flat_map(|c| c.to_lowercase()).collect()
}