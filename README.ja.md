# iPaste

> ローカルファーストでキーボード操作に強いデスクトップ向けクリップボードマネージャー。一時的なコピーを、検索でき、整理でき、再利用できるワークフローの部品に変えます。

**言語:** [English](README.md) | [简体中文](README.zh-CN.md) | 日本語 | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md)



iPaste はシステムトレイに常駐し、クリップボード履歴をローカルに記録します。グローバルショートカットでパネルを開き、過去の内容を検索し、Enter で貼り付けたり、よく使うスニペットをカテゴリに保存して長期的に再利用できます。

チャット、ブラウザ、ターミナル、デザインツール、ノート、コードエディタを一日中行き来する人のために作られています。リンク、コマンド、カラー値、プロンプト、返信テンプレート、スクリーンショット内のテキストを、一時ファイルや古いチャットスレッドに埋もれさせる必要はありません。

![iPaste desktop preview](docs/assets/ipaste-app-preview.jpg)

## Features

- ローカルファースト: クリップボード履歴は現在のデバイス上のローカル SQLite データベースに保存されます。
- すばやいアクセス: <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> / <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> でパネルを開けます。ショートカットは設定で変更できます。
- 複数のコンテンツタイプ: テキスト、リンク、カラー、HTML スニペット、画像、ファイルのクリップボード項目に対応します。
- 検索とキーボードフロー: すばやい検索、選択、Enter での貼り付けに最適化されています。
- 保存カテゴリ: コード、コマンド、住所、返信テンプレート、プロンプトなど、再利用するスニペットを保存できます。
- 画像ビューア: プレビュー、ズーム、回転、クリップボードへのコピー、OCR によるテキスト抽出に対応します。
- 追記コピー: 資料を集める間、複数回コピーしたテキストを一つのスニペットに一時的に結合できます。
- クロスデバイス同期: 2 台のデバイスが使い捨ての招待チケットを交換するだけでインターネット経由で信頼関係を築き、クリップボードの内容をエンドツーエンド暗号化で直接送受信できます（QUIC + NAT ホールパンチ、失敗時はリレーが暗号文のみ中継）。クラウドアカウント不要で、複数デバイスの管理・取り消し・自動再接続に対応します。
- クイックアクション: シェルコマンドをパネルからワンキーで実行するアクションとして保存できます。確認ダイアログ、出力ストリーミング、JSON のインポート/エクスポートに対応します。
- 設定可能な環境設定: 保持期間、パネルレイアウト、既定の起動動作、グローバルショートカット、言語、OCR モードを設定できます。
- 任意のセルフホスト同期: 保存カテゴリと保存済みのテキスト系コンテンツのみを同期し、生のクリップボード履歴はローカルに残します。
- 署名付きアップデート: GitHub Releases または Cloudflare R2 で配布されるリリース向けに、組み込みの Tauri updater をサポートします。

## Download

最新ビルドは [Releases](https://github.com/iPaste-app/iPaste/releases/latest) からダウンロードできます。

現在のリリース対象:

| Platform | Architecture | Notes |
| --- | --- | --- |
| Windows | x64 | システムの WebView2 Runtime を使用します。ない場合は先にインストールしてください。 |
| macOS | Apple Silicon | 自動貼り付けにはアクセシビリティ権限が必要です。 |
| macOS | Intel | 自動貼り付けにはアクセシビリティ権限が必要です。 |

Linux はまだ公式ターゲットではありません。Tauri はクロスプラットフォームですが、このリポジトリでは現在 macOS と Windows の検証に重点を置いています。

### macOS の権限について

iPaste は macOS で 2 つの別々の権限が必要です（システム設定 → プライバシーとセキュリティ）:

- **アクセシビリティ**: 自動貼り付け（キー入力のシミュレーション）用。
- **画面収録**: スクリーンショット OCR 用。アクセシビリティとは別の権限で、アクセシビリティだけではスクリーンショット OCR は動作しません。

権限を有効にした後は、iPaste を完全に終了して再起動してください。インストーラーは未署名のため、iPaste の更新後に権限が無効になることがあります（設定のスイッチはオンのまま、アプリは「未許可」と表示される）。その場合は、リストから iPaste を削除して再度追加し、有効化してからアプリを再起動してください。

## Quick Start

1. iPaste を起動します。トレイに常駐し、クリップボードの監視を開始します。
2. いつもどおりテキスト、リンク、カラー、画像をコピーします。
3. <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> または <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> を押してパネルを開きます。
4. 検索して項目を選択し、Enter を押してアクティブなアプリに貼り付けます。
5. 長期的に再利用する内容はカテゴリに保存し、自分のワークフローに合わせて整理します。

macOS での自動貼り付けにはアクセシビリティ権限が必要です。Windows で画像 OCR を使うには、Settings から PaddleOCR モデルをダウンロードする必要があります。

## Privacy And Data

iPaste は既定でローカルファーストです。

- 自動取得されたクリップボード履歴はアップロードも同期もされません。
- ローカルデータはシステムのアプリデータディレクトリ内の SQLite データベースに保存されます。
- クロスデバイス同期は、自分のデバイス間でインターネット経由の直接転送を行います。信頼は使い捨ての招待チケットで確立し、通信は QUIC TLS によるエンドツーエンド暗号化で保護されます（NAT ホールパンチ失敗時はリレーが暗号文のみ中継）。クラウドアカウントは不要です。
- クラウド同期を有効にした場合、カテゴリと保存済みのテキスト、リンク、カラー、HTML 項目のみが同期されます。
- 画像とファイルのスニペットは、現在クラウド同期のペイロードから除外されています。
- クラウド同期には、自分で用意した API アドレスと API キーが必要です。
- updater はインストール前に署名済みリリース成果物を検証します。

クリップボードにパスワード、キー、顧客データ、社内コンテンツが頻繁に含まれる場合は、クリップボードマネージャーを使用する前にチームのセキュリティルールを確認してください。

## Platform Support

| Platform | Status | Notes |
| --- | --- | --- |
| macOS | Supported | OCR はシステムの Vision framework を使用します。自動貼り付けにはアクセシビリティ権限が必要です。 |
| Windows | Supported | OCR はダウンロード可能な PaddleOCR モデルを使用します。 |
| Linux | Not supported yet | 現時点では公式リリースも完全な検証もありません。 |

## Tech Stack

- Tauri 2: デスクトップシェル、トレイ、ウィンドウ、updater、システム連携。
- Rust: クリップボード取得、SQLite ストレージ、グローバルショートカット、貼り付け自動化、OCR パイプライン、同期オーケストレーション。
- Vue 3、TypeScript、Pinia、Vite、Tailwind CSS 4: アプリ UI。
- `rusqlite`: ローカル SQLite 永続化。
- Cloudflare Pages/Workers 互換 API: 任意の同期サービス。

## Development

### Requirements

- Node.js 22 以降。
- npm 10 以降。
- Rust stable toolchain。
- 使用している OS 向けの Tauri 2 プラットフォーム依存関係。

macOS での開発には Xcode Command Line Tools が必要です。Windows での開発には Microsoft C++ Build Tools が必要です。WebView2 Runtime がない場合は、あわせてインストールしてください。

### Install Dependencies

```bash
npm install
```

### Web Preview

```bash
npm run dev
```

ネイティブ Tauri API が利用できない場合、ブラウザプレビューはモックデータを使用します。UI 作業には便利ですが、実際のシステムクリップボードは取得しません。

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

ネイティブの簡易コンパイルチェック:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

### Shared Types

`src/types/generated/` の TypeScript バインディングは、ts-rs によって Rust から生成されます。`models.rs` の共有モデルや `events.rs` のイベント定義を変更した後は、再生成してコミットしてください。CI が新しさを検証します。

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

Rust バックエンドはシステムクリップボードを監視し、対応しているコンテンツを正規化して SQLite に書き込み、Vue パネルへ更新を送信します。テキスト系スニペットはコンテンツハッシュで重複排除されます。画像スニペットはローカルのアプリデータリソースとして保存され、Tauri resource protocol 経由でレンダリングされます。

### Applying Snippets

iPaste から貼り付けると、アプリは選択したスニペットをシステムクリップボードへ書き戻し、その後プラットフォームの貼り付けショートカットを実行します。macOS での直接貼り付けにはアクセシビリティ権限が必要です。

### Saved Categories

履歴項目と保存カテゴリ項目は別の概念です。履歴項目は保持ポリシーに従って期限切れになります。保存カテゴリ項目は明示的に保存されたスナップショットで、削除するまで保持されます。

### Cloud Sync

デスクトップアプリは、Preferences で API アドレスと API キーを設定して、セルフホストの iPaste sync API に接続できます。同期範囲にはカテゴリと保存済みのテキスト系カテゴリ項目が含まれます。同期サービスのソースは準備ができ次第オープンソース化されます。

### クロスデバイス同期

2 台の iPaste は使い捨ての招待チケットでペアリングします。片方のデバイスが招待を作成し、もう片方がチケットで参加し、転送の前に両側が確認します。デバイス間は QUIC でインターネット経由の直接接続となり（NAT ホールパンチ失敗時はリレーが暗号文のみ中継）、クリップとカテゴリ全体がエンドツーエンド暗号化で直接やり取りされ、受け取り側に存在しないカテゴリは自動的に作成されます。ペア済みデバイスはいつでも管理・取り消しでき、切断後は自動で再接続します。

### Quick Actions

クイックアクションは、保存したシェルコマンドであり、専用のパネルカテゴリに表示されます。ワンキーで実行し、必要なら事前に確認でき、詳細ペインでストリーミング出力を確認でき、JSON のインポート/エクスポートでセットを共有できます。

### Image OCR

macOS はシステムの Vision framework を使用します。Windows はアプリの環境設定からインストールできる PaddleOCR モデルを使用します。

## Contributing

Issue、アイデア、Pull Request を歓迎します。

Pull Request を送る前に、少なくとも次を実行してください。

```bash
npm run lint
npm test
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

Windows で Rust バックエンドをビルドするには、bindgen 用の libclang が必要です（PaddleOCR エンジンが使用します）。`choco install llvm` で導入するか、`pip install libclang` を実行して `LIBCLANG_PATH` を Python の site-packages 内の `clang/native` ディレクトリに設定してください。

共有 Rust モデルやイベントに触れる変更では、`npm run gen:types` も実行し、再生成されたバインディングをコミットに含めてください。

プロジェクトをローカルファースト、プライバシー重視に保ち、ユーザーデータを同期する変更には慎重に対応してください。大きな機能については、まず Issue を開いて境界とインタラクション設計を相談してください。

## License

このプロジェクトは Apache License 2.0 の下でライセンスされています。詳しくは [LICENSE](LICENSE) と [NOTICE](NOTICE) を参照してください。

再配布する場合は、ライセンス、著作権、NOTICE 情報を保持してください。変更したファイルには変更内容を記録する必要があります。
