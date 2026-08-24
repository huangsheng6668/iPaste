# iPaste

> Un gestionnaire de presse-papiers de bureau local-first et pensé pour le clavier, qui transforme les copies temporaires en éléments de workflow consultables, organisés et réutilisables.

**Langues:** [English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md) | [Español](README.es.md) | Français | [Deutsch](README.de.md)


iPaste vit dans la zone de notification et enregistre l'historique du presse-papiers localement. Ouvrez le panneau avec un raccourci global, recherchez du contenu précédent, appuyez sur Enter pour coller, ou enregistrez les extraits utilisés fréquemment dans des catégories pour les réutiliser durablement.

Il est conçu pour les personnes qui passent toute la journée entre messageries, navigateurs, terminaux, outils de design, notes et éditeurs de code. Liens, commandes, valeurs de couleur, prompts, modèles de réponse et texte de captures d'écran n'ont pas besoin de disparaître dans des fichiers temporaires ou d'anciens fils de discussion.

![iPaste desktop preview](docs/assets/ipaste-app-preview.jpg)

## Features

- Local-first: l'historique du presse-papiers est stocké dans une base SQLite locale sur l'appareil actuel.
- Accès rapide: ouvrez le panneau avec <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> / <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd>, ou personnalisez le raccourci dans les paramètres.
- Plusieurs types de contenu: texte, liens, couleurs, extraits HTML, images et entrées de fichiers du presse-papiers.
- Recherche et navigation au clavier: optimisé pour retrouver rapidement, sélectionner et coller avec Enter.
- Catégories enregistrées: conservez des extraits réutilisables pour du code, des commandes, des adresses, des modèles de réponse, des prompts et plus encore.
- Visionneuse d'images: prévisualisez, zoomez, faites pivoter, recopiez dans le presse-papiers et extrayez du texte avec l'OCR.
- Copie par ajout: fusionnez temporairement plusieurs copies de texte en un seul extrait pendant que vous rassemblez du contenu.
- Synchronisation entre appareils : appariez deux appareils via Internet en échangeant un ticket d'invitation à usage unique — le contenu du presse-papiers est transmis directement et chiffré de bout en bout (QUIC + perçage NAT, retransmis sous forme chiffrée par un relais si le perçage échoue), sans compte cloud ; gestion multi-appareils, révocation et reconnexion automatique incluses.
- Actions rapides : enregistrez des commandes shell comme actions de panneau à une touche, avec confirmation optionnelle, sortie en flux et import/export JSON.
- Préférences configurables: durée de conservation, disposition du panneau, comportement d'ouverture par défaut, raccourci global, langue et mode OCR.
- Synchronisation autohébergée facultative: synchronise uniquement les catégories enregistrées et le contenu textuel enregistré; l'historique brut du presse-papiers reste local.
- Mises à jour signées: prise en charge du Tauri updater intégré pour les versions distribuées via GitHub Releases ou Cloudflare R2.

## Download

Téléchargez la dernière version depuis [Releases](https://github.com/iPaste-app/iPaste/releases/latest).

Cibles de la version actuelle:

| Platform | Architecture | Notes |
| --- | --- | --- |
| Windows | x64 | Utilise WebView2 Runtime du système; installez-le d'abord s'il manque. |
| macOS | Apple Silicon | Le collage automatique nécessite l'autorisation Accessibilité. |
| macOS | Intel | Le collage automatique nécessite l'autorisation Accessibilité. |

Linux n'est pas encore une cible officielle. Tauri est multiplateforme, mais ce dépôt se concentre actuellement sur la validation de macOS et Windows.

### Autorisations macOS

iPaste nécessite deux autorisations distinctes sur macOS (Réglages Système → Confidentialité et sécurité) :

- **Accessibilité** : pour le collage automatique (simulation de touches).
- **Enregistrement d'écran** : pour l'OCR de capture. C'est une autorisation distincte de l'Accessibilité — l'OCR de capture ne fonctionne pas avec l'Accessibilité seule.

Après l'avoir activée, quittez complètement et redémarrez iPaste. Les installations n'étant pas signées, les autorisations peuvent cesser de fonctionner après chaque mise à jour (l'interrupteur reste activé mais l'app indique « non accordée ») : retirez alors iPaste de la liste, ajoutez-le à nouveau, activez-le et redémarrez l'app.

## Quick Start

1. Lancez iPaste. Il reste dans la zone de notification et commence à écouter le presse-papiers.
2. Copiez du texte, des liens, des couleurs ou des images comme d'habitude.
3. Appuyez sur <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> ou <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> pour ouvrir le panneau.
4. Recherchez, sélectionnez un élément, puis appuyez sur Enter pour le coller dans l'application active.
5. Enregistrez le contenu réutilisable à long terme dans des catégories et organisez-le autour de votre workflow.

Le collage automatique sur macOS nécessite l'autorisation Accessibilité. L'OCR d'image sur Windows nécessite de télécharger les modèles PaddleOCR depuis Settings.

## Privacy And Data

iPaste est local-first par défaut.

- L'historique du presse-papiers capturé automatiquement n'est ni téléversé ni synchronisé.
- Les données locales sont stockées dans une base SQLite sous le répertoire de données d'application du système.
- La synchronisation entre appareils transfère le contenu directement entre vos propres appareils via Internet. La confiance est établie par un ticket d'invitation à usage unique ; le transport est chiffré de bout en bout via QUIC TLS (perçage NAT ; un relais ne retransmet que du texte chiffré en cas d'échec) ; aucun compte cloud n'est nécessaire.
- Lorsque la synchronisation cloud est activée, seules les catégories et les entrées enregistrées de texte, lien, couleur et HTML sont synchronisées.
- Les extraits d'image et de fichier sont actuellement exclus de la charge utile de synchronisation cloud.
- La synchronisation cloud nécessite votre propre adresse d'API et votre propre clé d'API.
- L'updater vérifie les artefacts de version signés avant l'installation.

Si votre presse-papiers contient souvent des mots de passe, clés, données client ou contenus internes à l'entreprise, confirmez les règles de sécurité de votre équipe avant d'utiliser un gestionnaire de presse-papiers.

## Platform Support

| Platform | Status | Notes |
| --- | --- | --- |
| macOS | Supported | L'OCR utilise le framework Vision du système; le collage automatique nécessite l'autorisation Accessibilité. |
| Windows | Supported | L'OCR utilise des modèles PaddleOCR téléchargeables. |
| Linux | Not supported yet | Aucune version officielle ni validation complète pour le moment. |

## Tech Stack

- Tauri 2: shell de bureau, zone de notification, fenêtres, updater et intégration système.
- Rust: capture du presse-papiers, stockage SQLite, raccourcis globaux, automatisation du collage, pipeline OCR et orchestration de la synchronisation.
- Vue 3, TypeScript, Pinia, Vite, Tailwind CSS 4: interface de l'application.
- `rusqlite`: persistance SQLite locale.
- API compatible Cloudflare Pages/Workers: service de synchronisation facultatif.

## Development

### Requirements

- Node.js 22 ou plus récent.
- npm 10 ou plus récent.
- Rust stable toolchain.
- Dépendances de plateforme Tauri 2 pour votre système d'exploitation.

Le développement sur macOS nécessite Xcode Command Line Tools. Le développement sur Windows nécessite Microsoft C++ Build Tools; installez aussi WebView2 Runtime s'il manque.

### Install Dependencies

```bash
npm install
```

### Web Preview

```bash
npm run dev
```

L'aperçu navigateur utilise mock data lorsque les API natives Tauri ne sont pas disponibles. Il est utile pour le travail d'interface, mais il ne capture pas le vrai presse-papiers du système.

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

Vérification rapide de compilation native:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

### Shared Types

Les liaisons TypeScript dans `src/types/generated/` sont générées depuis Rust via ts-rs. Après avoir modifié les modèles partagés dans `models.rs` ou les événements dans `events.rs`, régénérez-les et versionnez-les ; la CI vérifie leur fraîcheur.

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

Le backend Rust écoute le presse-papiers du système, normalise le contenu pris en charge, l'écrit dans SQLite et émet des mises à jour vers le panneau Vue. Les extraits de type texte sont dédupliqués par hash de contenu. Les extraits d'image sont stockés comme ressources de données locales de l'application et rendus via le Tauri resource protocol.

### Applying Snippets

Lors du collage depuis iPaste, l'application réécrit l'extrait sélectionné dans le presse-papiers du système, puis déclenche le raccourci de collage de la plateforme. Le collage direct sur macOS nécessite l'autorisation Accessibilité.

### Saved Categories

Les éléments d'historique et les éléments de catégories enregistrées sont deux concepts différents. Les éléments d'historique expirent selon la politique de conservation. Les éléments de catégories enregistrées sont des instantanés explicites conservés jusqu'à leur suppression.

### Cloud Sync

L'application de bureau peut se connecter à une iPaste sync API autohébergée en utilisant une adresse d'API et une clé d'API dans Preferences. Le périmètre de synchronisation inclut les catégories et les éléments de catégorie enregistrés de type texte. Le code source du service de synchronisation sera publié en open source lorsqu'il sera prêt.

### Synchronisation entre appareils

Deux instances d'iPaste s'apparient en échangeant un ticket d'invitation à usage unique : un appareil crée l'invitation, l'autre rejoint avec le ticket, et les deux parties confirment avant tout transfert. Les appareils se connectent directement via Internet sur QUIC (perçage NAT ; un relais ne retransmet que du texte chiffré en cas d'échec) ; les clips et des catégories entières circulent entre appareils appairés, et une catégorie absente chez le destinataire est créée automatiquement. Les appareils appairés peuvent être gérés ou révoqués à tout moment, et les connexions se rétablissent automatiquement après une coupure.

### Quick Actions

Les actions rapides sont des commandes shell enregistrées, affichées dans leur propre catégorie du panneau. Exécutez-les d'une touche, confirmez au préalable si vous le souhaitez, suivez la sortie en flux dans le volet de détails et partagez des ensembles entre machines via l'import/export JSON.

### Image OCR

macOS utilise le framework Vision du système. Windows utilise des modèles PaddleOCR qui peuvent être installés depuis les préférences de l'application.

## Contributing

Les Issues, idées et Pull Requests sont les bienvenues.

Avant d'envoyer une Pull Request, exécutez au minimum:

```bash
npm run lint
npm test
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

Pour compiler le backend Rust sous Windows, libclang est nécessaire pour bindgen (utilisé par le moteur PaddleOCR). Installez-le avec `choco install llvm`, ou via `pip install libclang` en pointant `LIBCLANG_PATH` vers le dossier `clang/native` de vos site-packages Python.

Si votre modification touche les modèles Rust partagés ou les événements, exécutez aussi `npm run gen:types` et incluez les liaisons régénérées dans le commit.

Veuillez garder le projet local-first, respectueux de la vie privée et prudent avec tout changement qui synchronise des données utilisateur. Pour les fonctionnalités plus importantes, ouvrez d'abord une Issue afin de discuter des limites et du design d'interaction.

## License

Ce projet est sous licence Apache License 2.0. Consultez [LICENSE](LICENSE) et [NOTICE](NOTICE).

Lors de la redistribution, conservez la licence, le copyright et les informations NOTICE; les fichiers modifiés doivent documenter leurs changements.
