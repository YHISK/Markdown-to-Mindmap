# 🧠 MindCraft (Aún no, hasta no terminarlo completamente)

> **Editor y visualizador interactivo de mapas mentales montado en RUST y basados en Markdown en tiempo real, con exportación nativa a `.xmind`.**

![Rust](https://img.shields.io/badge/Rust-2021_Edition-orange?style=for-the-badge&logo=rust)
![GUI](https://img.shields.io/badge/GUI-eframe_%2F_egui-blue?style=for-the-badge)
![Status](https://img.shields.io/badge/Status-In_Development-yellow?style=for-the-badge)
![Platform](https://img.shields.io/badge/Platform-Linux_%7C_Windows_%7C_macOS-lightgrey?style=for-the-badge)

**MindCraft** es una aplicación de escritorio nativa y ultraligera escrita en **Rust** que transforma esquemas en formato **Markdown** en diagramas visuales interactivos de manera instantánea, permitiendo además editar la estructura directamente desde el lienzo y exportar el resultado a archivos compatibles con **XMind (`.xmind`)** con un solo clic.

---

## ✨ Características Principales

* ⚡ **Sincronización bidireccional en tiempo real:** Escribe en el panel Markdown o manipula los nodos en el lienzo; ambos se mantienen sincronizados al instante.
* 🎨 **Múltiples vistas y estructuras de diagrama:**
  * **Mapa de ideas (*Idea Map*):** Distribución radial balanceada a izquierda y derecha del tema central.
  * **Gráfico lógico (*Logic Chart*):** Expansión jerárquica configurable por dirección global (`Derecha`, `Izquierda`, `Abajo`, `Arriba`) y personalizable por nodo individual.
  * **Gráfico de árbol (*Tree Chart*):** Estructura organizacional descendente por niveles.
  * **Línea temporal (*Timeline*):** Eje horizontal cronológico con hitos alternados arriba y abajo.
* 🗂️ **Plegado y despliegue de ramas:** Contrae o expande cualquier subárbol con hijos (`[+]` / `[-]`) para enfocarte en secciones específicas sin perder contenido al exportar.
* 🖱️ **Navegación fluida en lienzo:** Arrastre libre (*pan*) con clic sostenido y zoom independiente del mapa (`Ctrl + Rueda del ratón`), además de centrado rápido de vista.
* 📦 **Exportación nativa a `.xmind`:** Genera paquetes `.xmind` válidos (`content.json`, `metadata.json` y `manifest.json` comprimidos en ZIP con UUIDs únicos por tema).
* 🔒 **100% Local y Offline:** Sin telemetría, sin cuentas en la nube y con rendimiento nativo gracias a la compilación directa a código máquina sin recolector de basura.

---

## 🏗️ Arquitectura Interna del Código

El proyecto está organizado en cuatro módulos lógicos dentro de `src/main.rs`:

```text
┌─────────────────────┐      ┌──────────────────────┐      ┌────────────────────────┐
│   Editor Markdown   │ ───▶ │   Parser de Árbol    │ ───▶ │   Motor de Layout &    │
│   (egui TextEdit)   │ ◀─── │   (TreeNode / AST)   │      │   Renderizado (egui)   │
└─────────────────────┘      └──────────┬───────────┘      └────────────────────────┘
                                        │
                                        ▼
                             ┌──────────────────────┐
                             │  Exportador .xmind   │
                             │ (JSON + ZIP + UUID)  │
                             └──────────────────────┘
