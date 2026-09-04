@echo off
REM ============================================
REM  SynapseAI - Lanzador para Windows
REM ============================================
echo SynapseAI v0.1.0 - IA para Robotica
echo.

REM Verificar si el binario existe
if not exist "target\release\synapse.exe" (
    echo Compilando primero...
    cargo build --release
)

REM Modo por defecto
if "%1"=="" (
    echo Modo interactivo. Escribe 'help' para ver comandos.
    echo.
    target\release\synapse.exe
) else (
    target\release\synapse.exe %*
)
