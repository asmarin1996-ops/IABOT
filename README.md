# SynapseAI

Sistema de IA para robótica con aprendizaje continuo, memoria persistente y auto-conciencia.
Compatible con Linux / Raspberry Pi, testeable en PC.

## ¿Qué es?

SynapseAI es un framework modular de inteligencia artificial diseñado para robots.
Aprende de su entorno, guarda memoria en cada experiencia, adapta su comportamiento
y monitoriza su propio estado.

## Módulos

| Módulo              | Funcion |
|---------------------|---------|
| `synapse-core`      | Motor de aprendizaje (Q-Learning + exploración) |
| `synapse-memory`    | Memoria persistente (SQLite: experiencias, patrones, conocimiento) |
| `synapse-consciousness` | Auto-monitoreo, emociones simuladas, adaptación |
| `synapse-hal`       | Hardware Abstraction Layer (sensores y actuadores) |
| `synapse-sim`       | Simulador virtual del mundo y robot (para testing en PC) |
| `synapse-cli`       | Terminal interactiva |

## Requisitos

- Rust (1.75+)
- Linux (o Windows para desarrollo/testing)

## Uso rápido (PC)

```bash
# Compilar
cargo build --release

# Modo demo (entrena 100 episodios automáticamente)
./target/release/synapse demo

# Entrenar con N episodios
./target/release/synapse train 500

# Modo interactivo (más comandos)
./target/release/synapse
```

## Comandos en modo interactivo

```
step        - Ejecutar un paso de simulación
train <n>   - Entrenar N episodios
status      - Estado del robot (posición, metas, reward)
world       - Render del mundo en ASCII
emotion     - Estado emocional del sistema
brain       - Estado del cerebro (Q-Table)
memory      - Resume de memoria aprendida
adapt       - Reglas de adaptación
diagnostic  - Diagnóstico del sistema
quit        - Salir
```

## Raspberry Pi

Para Raspberry Pi (Linux ARM64):

```bash
# Desde una PC con Rust:
cargo build --release --target aarch64-unknown-linux-gnu

# Copiar el binario a la Raspberry Pi:
scp target/aarch64-unknown-linux-gnu/release/synapse pi@<ip>:/home/pi/

# En la Raspberry Pi:
ssh pi@<ip>
chmod +x synapse
./synapse demo
```

Para usar sensores reales (HC-SR04, servos, motores, etc.),
implementa los traits de `synapse-hal` con los drivers del GPIO
(`rppal` o `linux-embedded-hal`).

## Arquitectura

```
┌──────────────────────────────────────────┐
│               CLI / Dashboard               │
├──────────┬───────────┬───────────┬────────┤
│synapse- │synapse-   │synapse-   │synapse-│
│core     │memory     │conscious- │hal     │
│(Brain)  │(SQLite)   │ness       │(HW)    │
│         │           │(Emotion)  │        │
├──────────┴───────────┴───────────┴────────┤
│              simulator (PC)                │
│            o  Raspberry Pi HW              │
└──────────────────────────────────────────┘
```

## Concepto de conciencia

- **Emociones simuladas**: confianza, curiosidad, estrés, satisfacción, cautela
- **Adaptación**: reglas que cambian según el estado emocional y de los sensores
- **Auto-monitoreo**: detecta anomalías, sensores caídos, y genera diagnósticos
- **Memoria**: recupera experiencias similares para decidir mejor

## Estado del proyecto

- [x] Motor de aprendizaje (Q-Learning)
- [x] Memoria persistente (SQLite)
- [x] Conciencia (emociones + adaptación + monitoreo)
- [x] HAL abstracto (sensores/actuadores)
- [x] Simulador virtual (mundo ASCII + robot)
- [x] CLI interactiva
- [ ] Drivers Raspberry Pi reales (GPIO)
- [ ] Red neuronal en lugar de Q-Learning
- [ ] Cámara / visión
- [ ] Comunicación entre múltiples robots

## Licencia

MIT
