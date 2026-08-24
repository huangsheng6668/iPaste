# iPaste

> Un gestor de portapapeles de escritorio local-first y cómodo para teclado que convierte copias temporales en piezas de flujo de trabajo buscables, organizadas y reutilizables.

**Idiomas:** [English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md) | Español | [Français](README.fr.md) | [Deutsch](README.de.md)



iPaste vive en la bandeja del sistema y registra el historial del portapapeles de forma local. Abre el panel con un atajo global, busca contenido anterior, pulsa Enter para pegar o guarda fragmentos usados con frecuencia en categorías para reutilizarlos a largo plazo.

Está pensado para personas que se mueven todo el día entre chats, navegadores, terminales, herramientas de diseño, notas y editores de código. Enlaces, comandos, valores de color, prompts, plantillas de respuesta y texto de capturas de pantalla no tienen por qué perderse en archivos temporales o hilos de chat antiguos.

![iPaste desktop preview](docs/assets/ipaste-app-preview.jpg)

## Features

- Local-first: el historial del portapapeles se guarda en una base de datos SQLite local en el dispositivo actual.
- Acceso rápido: abre el panel con <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> / <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd>, o personaliza el atajo en la configuración.
- Varios tipos de contenido: texto, enlaces, colores, fragmentos HTML, imágenes y entradas de archivos del portapapeles.
- Búsqueda y flujo de teclado: optimizado para consulta rápida, selección y pegado con Enter.
- Categorías guardadas: conserva fragmentos reutilizables para código, comandos, direcciones, plantillas de respuesta, prompts y más.
- Visor de imágenes: previsualiza, amplía, rota, copia de nuevo al portapapeles y extrae texto con OCR.
- Copia acumulativa: combina temporalmente varias copias de texto en un solo fragmento mientras recopilas material.
- Sincronización entre dispositivos: empareja dos dispositivos a través de internet intercambiando un ticket de invitación de un solo uso; el contenido del portapapeles viaja directo y cifrado de extremo a extremo (QUIC + perforación NAT, con un relé que reenvía solo texto cifrado si la perforación falla), sin cuenta en la nube; admite gestión multidispositivo, revocación y reconexión automática.
- Acciones rápidas: guarda comandos de shell como acciones de panel de una sola tecla, con confirmación opcional, salida en streaming e importación/exportación en JSON.
- Preferencias configurables: periodo de retención, diseño del panel, comportamiento de apertura predeterminado, atajo global, idioma y modo OCR.
- Sincronización autohospedada opcional: sincroniza solo categorías guardadas y contenido guardado de tipo texto; el historial bruto del portapapeles permanece local.
- Actualizaciones firmadas: soporte integrado del Tauri updater para versiones distribuidas mediante GitHub Releases o Cloudflare R2.

## Download

Descarga la compilación más reciente desde [Releases](https://github.com/iPaste-app/iPaste/releases/latest).

Destinos de la versión actual:

| Platform | Architecture | Notes |
| --- | --- | --- |
| Windows | x64 | Usa WebView2 Runtime del sistema; instálalo primero si falta. |
| macOS | Apple Silicon | El pegado automático requiere permiso de Accesibilidad. |
| macOS | Intel | El pegado automático requiere permiso de Accesibilidad. |

Linux aún no es un destino oficial. Tauri es multiplataforma, pero este repositorio se centra actualmente en validar macOS y Windows.

### Permisos de macOS

iPaste necesita dos permisos independientes en macOS (Ajustes del Sistema → Privacidad y seguridad):

- **Accesibilidad**: para el pegado automático (simulación de teclas).
- **Grabación de pantalla**: para el OCR de capturas. Es un permiso distinto de Accesibilidad: el OCR de capturas no funciona solo con Accesibilidad activada.

Tras activar un permiso, cierra por completo y reinicia iPaste. Como las instalaciones no están firmadas, los permisos pueden dejar de funcionar tras cada actualización (el interruptor sigue activado pero la app indica «No concedido»): en ese caso, elimina iPaste de la lista, añádelo de nuevo, actívalo y reinicia la app.

## Quick Start

1. Inicia iPaste. Permanece en la bandeja y empieza a escuchar el portapapeles.
2. Copia texto, enlaces, colores o imágenes como de costumbre.
3. Pulsa <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> o <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> para abrir el panel.
4. Busca, selecciona un elemento y pulsa Enter para pegarlo de nuevo en la aplicación activa.
5. Guarda el contenido reutilizable a largo plazo en categorías y organízalo alrededor de tu flujo de trabajo.

El pegado automático en macOS requiere permiso de Accesibilidad. El OCR de imágenes en Windows requiere descargar los modelos de PaddleOCR desde Settings.

## Privacy And Data

iPaste es local-first de forma predeterminada.

- El historial del portapapeles capturado automáticamente no se sube ni se sincroniza.
- Los datos locales se guardan en una base de datos SQLite dentro del directorio de datos de la aplicación del sistema.
- La sincronización entre dispositivos transfiere contenido directamente entre tus propios dispositivos a través de internet. La confianza se establece con un ticket de invitación de un solo uso; el transporte está cifrado de extremo a extremo con QUIC TLS (perforación NAT; un relé reenvía solo texto cifrado si falla); no hace falta ninguna cuenta en la nube.
- Cuando la sincronización en la nube está activada, solo se sincronizan categorías y entradas guardadas de texto, enlaces, colores y HTML.
- Los fragmentos de imagen y archivo están actualmente excluidos de la carga de sincronización en la nube.
- La sincronización en la nube requiere tu propia dirección de API y clave de API.
- El updater verifica los artefactos de versión firmados antes de la instalación.

Si tu portapapeles suele contener contraseñas, claves, datos de clientes o contenido interno de la empresa, confirma las reglas de seguridad de tu equipo antes de usar cualquier gestor de portapapeles.

## Platform Support

| Platform | Status | Notes |
| --- | --- | --- |
| macOS | Supported | OCR usa el framework Vision del sistema; el pegado automático requiere permiso de Accesibilidad. |
| Windows | Supported | OCR usa modelos descargables de PaddleOCR. |
| Linux | Not supported yet | Por el momento no hay versión oficial ni validación completa. |

## Tech Stack

- Tauri 2: shell de escritorio, bandeja, ventanas, updater e integración del sistema.
- Rust: captura del portapapeles, almacenamiento SQLite, atajos globales, automatización de pegado, pipeline de OCR y orquestación de sincronización.
- Vue 3, TypeScript, Pinia, Vite, Tailwind CSS 4: interfaz de la app.
- `rusqlite`: persistencia SQLite local.
- API compatible con Cloudflare Pages/Workers: servicio de sincronización opcional.

## Development

### Requirements

- Node.js 22 o posterior.
- npm 10 o posterior.
- Rust stable toolchain.
- Dependencias de plataforma de Tauri 2 para tu sistema operativo.

El desarrollo en macOS requiere Xcode Command Line Tools. El desarrollo en Windows requiere Microsoft C++ Build Tools; instala también WebView2 Runtime si falta.

### Install Dependencies

```bash
npm install
```

### Web Preview

```bash
npm run dev
```

La vista previa en el navegador usa mock data cuando las API nativas de Tauri no están disponibles. Es útil para trabajo de UI, pero no captura el portapapeles real del sistema.

### Desktop Development

```bash
npm run tauri dev
```

### Build

```bash
npm run lint        # ESLint
npm test            # Vitest unit tests (frontend)
npm run build       # Type-check (vue-tsc) + Vite production build
npm run tauri build # Desktop installers
```

Comprobación rápida de compilación nativa:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

### Shared Types

Los bindings de TypeScript en `src/types/generated/` se generan desde Rust mediante ts-rs. Tras cambiar los modelos compartidos en `models.rs` o los eventos en `events.rs`, regenera y haz commit de los archivos; la CI verifica su frescura.

```bash
npm run gen:types
```

## Project Structure

```text
.
├── src/                  # Vue app: components, composables, Pinia stores, frontend API wrappers
├── src-tauri/            # Tauri config and Rust desktop backend
│   └── src/              # Rust backend modules (see below)
├── scripts/              # Release, versioning, and updater distribution tools
├── docs/                 # Operational docs and project notes
├── key/                  # Public updater key; private keys must not be committed
└── .github/workflows/    # CI and signed desktop build release workflows
```

The Rust backend in `src-tauri/src/` is split into small domain modules:

| Module | Responsibility |
| --- | --- |
| `lib.rs` | Tauri builder entry (`run()` composition root) and shared constants |
| `models.rs` | Structured serde data models shared by commands and modules (exported to TypeScript via ts-rs) |
| `error.rs` | `AppError`: unified command error contract (`{code, message, params}`) |
| `events.rs` | Single source of frontend/backend event names and payloads; generates `src/types/generated/events.ts` |
| `util.rs` | Shared pure helpers: hashing, clip-type detection, `clean_*` validation, localized labels |
| `store.rs` + `store/` | SQLite persistence split by domain (clips/categories/settings/automations/sync/migrations/secrets) |
| `clipboard.rs` | Clipboard capture, normalization, and write-back |
| `cloud.rs` | Self-hosted sync API client |
| `lan_sync/` | Cross-device sync (v5): iroh QUIC transport, one-time invite tickets, device identity and trust store, multi-device link registry, pairing guard |
| `ocr/` | Image OCR: asset installer and status (Windows), PaddleOCR runner (Windows), Vision pipeline (macOS) |
| `window.rs` | Panel/settings/viewer windows, native panel behavior, window positioning |
| `tray.rs` | System tray, menu labels, menu event handling |
| `shortcut.rs` | Global shortcut registration and updates |
| `paste.rs` | Target app activation and paste triggering |
| `automation.rs` | Quick-action process execution and event streaming |
| `commands.rs` | Thin Tauri command layer exposing domain modules to the UI |

## How It Works

### Clipboard Capture

El backend de Rust escucha el portapapeles del sistema, normaliza el contenido admitido, lo escribe en SQLite y emite actualizaciones al panel de Vue. Los fragmentos de tipo texto se deduplican mediante hash de contenido. Los fragmentos de imagen se guardan como recursos de datos locales de la app y se renderizan mediante el Tauri resource protocol.

### Applying Snippets

Al pegar desde iPaste, la app escribe el fragmento seleccionado de nuevo en el portapapeles del sistema y luego dispara el atajo de pegado de la plataforma. El pegado directo en macOS requiere permiso de Accesibilidad.

### Saved Categories

Los elementos del historial y los elementos de categorías guardadas son conceptos distintos. Los elementos del historial caducan según la política de retención. Los elementos de categorías guardadas son capturas explícitas que se mantienen hasta que las elimines.

### Cloud Sync

La app de escritorio puede conectarse a una iPaste sync API autohospedada usando una dirección de API y una clave de API en Preferences. El alcance de la sincronización incluye categorías y elementos de categoría guardados de tipo texto. El código fuente del servicio de sincronización se publicará como open source cuando esté listo.

### Sincronización entre dispositivos

Dos instancias de iPaste se emparejan intercambiando un ticket de invitación de un solo uso: un dispositivo crea la invitación, el otro se une con el ticket y ambos confirman antes de cualquier transferencia. Los dispositivos se conectan directamente por internet mediante QUIC (perforación NAT; un relé reenvía solo texto cifrado si falla), y los clips y las categorías completas fluyen entre dispositivos emparejados; una categoría que no exista en el receptor se crea automáticamente. Los dispositivos emparejados pueden gestionarse o revocarse en cualquier momento, y las conexiones se reconectan automáticamente tras un corte.

### Quick Actions

Las acciones rápidas son comandos de shell guardados que se muestran en su propia categoría del panel. Ejecútalas con una tecla, confírmalas antes si lo deseas, consulta la salida en streaming en el panel de detalles y comparte conjuntos entre máquinas mediante importación/exportación JSON.

### Image OCR

macOS usa el framework Vision del sistema. Windows usa modelos de PaddleOCR que pueden instalarse desde las preferencias de la app.

## Contributing

Se agradecen Issues, ideas y Pull Requests.

Antes de enviar un Pull Request, ejecuta al menos:

```bash
npm run lint
npm test
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

Para compilar el backend de Rust en Windows se necesita libclang para bindgen (usado por el motor de PaddleOCR). Instálalo con `choco install llvm`, o con `pip install libclang` señalando `LIBCLANG_PATH` a la carpeta `clang/native` de tus site-packages de Python.

Si tu cambio afecta a los modelos compartidos de Rust o a los eventos, ejecuta también `npm run gen:types` e incluye los bindings regenerados en el commit.

Mantén el proyecto local-first, consciente de la privacidad y cuidadoso con cualquier cambio que sincronice datos de usuario. Para funciones grandes, abre primero un Issue para discutir límites y diseño de interacción.

## License

Este proyecto está licenciado bajo Apache License 2.0. Consulta [LICENSE](LICENSE) y [NOTICE](NOTICE).

Al redistribuir, conserva la licencia, el copyright y la información de NOTICE; los archivos modificados deben documentar sus cambios.
