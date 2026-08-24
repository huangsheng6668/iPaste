# iPaste

> Ein local-first, tastaturfreundlicher Desktop-Clipboard-Manager, der temporäre Kopien in durchsuchbare, organisierte und wiederverwendbare Workflow-Bausteine verwandelt.

**Sprachen:** [English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | Deutsch


iPaste lebt in der Systemablage und speichert den Clipboard-Verlauf lokal. Öffne das Panel mit einem globalen Shortcut, suche frühere Inhalte, drücke Enter zum Einfügen oder speichere häufig genutzte Snippets in Kategorien, um sie langfristig wiederzuverwenden.

Es ist für Menschen gebaut, die den ganzen Tag zwischen Chat, Browsern, Terminals, Design-Tools, Notizen und Code-Editoren wechseln. Links, Befehle, Farbwerte, Prompts, Antwortvorlagen und Screenshot-Text müssen nicht in temporären Dateien oder alten Chat-Verläufen verschwinden.

![iPaste desktop preview](docs/assets/ipaste-app-preview.jpg)

## Features

- Local-first: Der Clipboard-Verlauf wird in einer lokalen SQLite-Datenbank auf dem aktuellen Gerät gespeichert.
- Schneller Zugriff: Öffne das Panel mit <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> / <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> oder passe den Shortcut in den Einstellungen an.
- Mehrere Inhaltstypen: Text, Links, Farben, HTML-Snippets, Bilder und Datei-Clipboard-Einträge.
- Suche und Tastaturfluss: Optimiert für schnelles Nachschlagen, Auswählen und Einfügen mit Enter.
- Gespeicherte Kategorien: Bewahre wiederverwendbare Snippets für Code, Befehle, Adressen, Antwortvorlagen, Prompts und mehr auf.
- Bildbetrachter: Vorschau, Zoom, Drehen, Zurückkopieren in das Clipboard und Textextraktion mit OCR.
- Anhängendes Kopieren: Führe mehrere Textkopien vorübergehend zu einem Snippet zusammen, während du Material sammelst.
- Geräteübergreifende Synchronisierung: Koppeln Sie zwei Geräte über das Internet, indem Sie ein einmaliges Einladungs-Token austauschen – Inhalte der Zwischenablage werden direkt und Ende-zu-Ende-verschlüsselt übertragen (QUIC + NAT-Durchbruch; schlägt er fehl, leitet ein Relay nur Chiffrat weiter), ohne Cloud-Konto; mit Verwaltung mehrerer Geräte, Widerruf und automatischer Wiederverbindung.
- Schnellaktionen: Speichern Sie Shell-Befehle als Panel-Aktionen mit einem einzigen Tastendruck – mit optionaler Bestätigung, gestreamter Ausgabe und JSON-Import/-Export.
- Konfigurierbare Einstellungen: Aufbewahrungsdauer, Panel-Layout, Standard-Öffnungsverhalten, globaler Shortcut, Sprache und OCR-Modus.
- Optionale selbst gehostete Synchronisierung: Synchronisiert nur gespeicherte Kategorien und gespeicherte textähnliche Inhalte; der rohe Clipboard-Verlauf bleibt lokal.
- Signierte Updates: Eingebaute Unterstützung für den Tauri updater für Releases, die über GitHub Releases oder Cloudflare R2 verteilt werden.

## Download

Lade den neuesten Build über [Releases](https://github.com/iPaste-app/iPaste/releases/latest) herunter.

Aktuelle Release-Ziele:

| Platform | Architecture | Notes |
| --- | --- | --- |
| Windows | x64 | Verwendet die WebView2 Runtime des Systems; installiere sie zuerst, falls sie fehlt. |
| macOS | Apple Silicon | Automatisches Einfügen erfordert die Bedienungshilfen-Berechtigung. |
| macOS | Intel | Automatisches Einfügen erfordert die Bedienungshilfen-Berechtigung. |

Linux ist noch kein offizielles Ziel. Tauri ist plattformübergreifend, aber dieses Repository konzentriert sich derzeit auf die Validierung von macOS und Windows.

### macOS-Berechtigungen

iPaste benötigt unter macOS zwei getrennte Berechtigungen (Systemeinstellungen → Datenschutz & Sicherheit):

- **Bedienungshilfen**: für automatisches Einfügen (Simulation von Tastatureingaben).
- **Bildschirmaufnahme**: für Screenshot-OCR. Sie ist von der Bedienungshilfen-Berechtigung getrennt — Screenshot-OCR funktioniert nicht nur mit Bedienungshilfen.

Nach dem Aktivieren muss iPaste vollständig neu gestartet werden. Da die Installationen unsigniert sind, können Berechtigungen nach jedem Update verloren gehen (der Schalter erscheint als aktiv, die App meldet aber „Nicht erteilt“): Entferne iPaste in diesem Fall aus der Liste, füge es erneut hinzu, aktiviere es und starte die App neu.

## Quick Start

1. Starte iPaste. Es bleibt in der Ablage und beginnt, das Clipboard zu überwachen.
2. Kopiere Text, Links, Farben oder Bilder wie gewohnt.
3. Drücke <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> oder <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd>, um das Panel zu öffnen.
4. Suche, wähle einen Eintrag aus und drücke Enter, um ihn zurück in die aktive App einzufügen.
5. Speichere langfristig wiederverwendbare Inhalte in Kategorien und organisiere sie passend zu deinem Workflow.

Automatisches Einfügen unter macOS erfordert die Bedienungshilfen-Berechtigung. Bild-OCR unter Windows erfordert das Herunterladen der PaddleOCR-Modelle aus Settings.

## Privacy And Data

iPaste ist standardmäßig local-first.

- Automatisch erfasster Clipboard-Verlauf wird nicht hochgeladen oder synchronisiert.
- Lokale Daten werden in einer SQLite-Datenbank im App-Datenverzeichnis des Systems gespeichert.
- Die geräteübergreifende Synchronisierung überträgt Inhalte direkt zwischen Ihren eigenen Geräten über das Internet. Vertrauen wird durch ein einmaliges Einladungs-Token hergestellt; die Übertragung ist per QUIC TLS Ende-zu-Ende-verschlüsselt (NAT-Durchbruch; schlägt er fehl, leitet ein Relay nur Chiffrat weiter); ein Cloud-Konto ist nicht erforderlich.
- Wenn Cloud-Sync aktiviert ist, werden nur Kategorien sowie gespeicherte Text-, Link-, Farb- und HTML-Einträge synchronisiert.
- Bild- und Datei-Snippets sind derzeit von der Cloud-Sync-Nutzlast ausgeschlossen.
- Cloud-Sync erfordert deine eigene API-Adresse und deinen eigenen API-Schlüssel.
- Der updater prüft signierte Release-Artefakte vor der Installation.

Wenn dein Clipboard häufig Passwörter, Schlüssel, Kundendaten oder interne Unternehmensinhalte enthält, bestätige die Sicherheitsregeln deines Teams, bevor du einen Clipboard-Manager verwendest.

## Platform Support

| Platform | Status | Notes |
| --- | --- | --- |
| macOS | Supported | OCR nutzt das systemeigene Vision framework; automatisches Einfügen erfordert die Bedienungshilfen-Berechtigung. |
| Windows | Supported | OCR nutzt herunterladbare PaddleOCR-Modelle. |
| Linux | Not supported yet | Derzeit gibt es kein offizielles Release und keine vollständige Validierung. |

## Tech Stack

- Tauri 2: Desktop-Shell, Ablage, Fenster, updater und Systemintegration.
- Rust: Clipboard-Erfassung, SQLite-Speicher, globale Shortcuts, Einfügeautomatisierung, OCR-Pipeline und Synchronisierungs-Orchestrierung.
- Vue 3, TypeScript, Pinia, Vite, Tailwind CSS 4: App-UI.
- `rusqlite`: lokale SQLite-Persistenz.
- Cloudflare Pages/Workers-kompatible API: optionaler Synchronisierungsdienst.

## Development

### Requirements

- Node.js 22 oder neuer.
- npm 10 oder neuer.
- Rust stable toolchain.
- Tauri 2 Plattformabhängigkeiten für dein Betriebssystem.

macOS-Entwicklung erfordert Xcode Command Line Tools. Windows-Entwicklung erfordert Microsoft C++ Build Tools; installiere auch WebView2 Runtime, falls sie fehlt.

### Install Dependencies

```bash
npm install
```

### Web Preview

```bash
npm run dev
```

Die Browser-Vorschau verwendet mock data, wenn native Tauri-APIs nicht verfügbar sind. Sie ist nützlich für UI-Arbeit, erfasst aber nicht das echte System-Clipboard.

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

Schneller nativer Compile-Check:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

### Shared Types

Die TypeScript-Bindings in `src/types/generated/` werden per ts-rs aus Rust erzeugt. Nach Änderungen an gemeinsamen Modellen in `models.rs` oder Ereignissen in `events.rs` regenerieren und committen Sie sie; die CI prüft die Aktualität.

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

Das Rust-Backend überwacht das System-Clipboard, normalisiert unterstützte Inhalte, schreibt sie in SQLite und sendet Updates an das Vue-Panel. Textähnliche Snippets werden per Content-Hash dedupliziert. Bild-Snippets werden als lokale App-Datenressourcen gespeichert und über das Tauri resource protocol gerendert.

### Applying Snippets

Beim Einfügen aus iPaste schreibt die App das ausgewählte Snippet zurück in das System-Clipboard und löst anschließend den Einfüge-Shortcut der Plattform aus. Direktes Einfügen unter macOS erfordert die Bedienungshilfen-Berechtigung.

### Saved Categories

Verlaufseinträge und gespeicherte Kategorieeinträge sind unterschiedliche Konzepte. Verlaufseinträge laufen gemäß der Aufbewahrungsrichtlinie ab. Gespeicherte Kategorieeinträge sind explizite Snapshots, die erhalten bleiben, bis du sie löschst.

### Cloud Sync

Die Desktop-App kann sich über eine API-Adresse und einen API-Schlüssel in Preferences mit einer selbst gehosteten iPaste sync API verbinden. Der Synchronisierungsumfang umfasst Kategorien und gespeicherte textähnliche Kategorieeinträge. Der Quellcode des Synchronisierungsdienstes wird open source veröffentlicht, sobald er bereit ist.

### Geräteübergreifende Synchronisierung

Zwei iPaste-Instanzen koppeln sich durch den Austausch eines einmaligen Einladungs-Tokens: Ein Gerät erstellt die Einladung, das andere tritt mit dem Token bei, und beide Seiten bestätigen vor jeder Übertragung. Die Geräte verbinden sich direkt über das Internet per QUIC (NAT-Durchbruch; schlägt er fehl, leitet ein Relay nur Chiffrat weiter); Clips und ganze Kategorien fließen zwischen gekoppelten Geräten, und eine auf der Empfängerseite fehlende Kategorie wird automatisch erstellt. Gekoppelte Geräte lassen sich jederzeit verwalten oder widerrufen, und Verbindungen bauen sich nach Trennungen automatisch wieder auf.

### Quick Actions

Schnellaktionen sind gespeicherte Shell-Befehle, die in einer eigenen Panel-Kategorie erscheinen. Führen Sie sie mit einem Tastendruck aus, bestätigen Sie bei Bedarf vorher, verfolgen Sie die gestreamte Ausgabe im Detailbereich und teilen Sie Sets zwischen Rechnern über JSON-Import/-Export.

### Image OCR

macOS nutzt das systemeigene Vision framework. Windows nutzt PaddleOCR-Modelle, die über die App-Einstellungen installiert werden können.

## Contributing

Issues, Ideen und Pull Requests sind willkommen.

Führe vor dem Einreichen eines Pull Requests mindestens Folgendes aus:

```bash
npm run lint
npm test
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

Für den Build des Rust-Backends unter Windows wird libclang für bindgen benötigt (verwendet von der PaddleOCR-Engine). Installieren Sie es mit `choco install llvm`, oder per `pip install libclang`, wobei `LIBCLANG_PATH` auf den Ordner `clang/native` in Ihren Python site-packages zeigen muss.

Wenn Ihre Änderung gemeinsame Rust-Modelle oder Ereignisse betrifft, führen Sie zusätzlich `npm run gen:types` aus und committen Sie die neu generierten Bindings.

Bitte halte das Projekt local-first, datenschutzbewusst und vorsichtig bei jeder Änderung, die Nutzerdaten synchronisiert. Öffne für größere Funktionen zuerst ein Issue, um Grenzen und Interaktionsdesign zu besprechen.

## License

Dieses Projekt ist unter der Apache License 2.0 lizenziert. Siehe [LICENSE](LICENSE) und [NOTICE](NOTICE).

Bei der Weiterverteilung müssen Lizenz, Copyright und NOTICE-Informationen erhalten bleiben; geänderte Dateien müssen ihre Änderungen dokumentieren.
