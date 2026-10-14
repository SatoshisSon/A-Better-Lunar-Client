# A Better Lunar Client

Ein schneller, schlanker Minecraft-Java-Launcher als Alternative zu Lunar Client. **Keine Werbung, keine Telemetrie, kein Shop, kein Ballast.**

| | Lunar Client | A Better Lunar Client |
|---|---|---|
| Größe des Launchers | mehrere hundert MB (Electron) | ca. 7 MB (Tauri/Rust) |
| RAM des Launchers | 300–600 MB | ca. 50 MB |
| Werbung / Shop / Kosmetik-Käufe | ja | nein |
| Telemetrie | ja | nein |
| Performance-Mods | eigene, Closed Source | Sodium, Lithium, FerriteCore & Co. (Open Source, meist schneller) |
| Shader | eingeschränkt | Iris (alle gängigen Shaderpacks) |
| Eigene Mods hinzufügen | nein | ja, direkt aus Modrinth |
| Quellcode | geschlossen | offen |

## Funktionen

- **Minecraft starten**: alle Release-Versionen (optional auch Snapshots) mit
  - **Fabric** (ab 1.14), empfohlen
  - **Legacy Fabric** für alte Versionen wie **1.8.9**
  - **Vanilla**
- **Vorinstallierte Mods (Presets)**, die beim Anlegen eines Profils automatisch passend zur Version installiert werden:
  - *Performance*: Sodium, Lithium, FerriteCore, EntityCulling, ImmediatelyFast, ModernFix, Dynamic FPS
  - *Shader & Optik*: Iris, Sodium Extra, Reese's Sodium Options, Zoomify, LambDynamicLights, Continuity
  - *Komfort*: Mod Menu, Mouse Tweaks
  - Benötigte Abhängigkeiten (z. B. Fabric API) kommen automatisch dazu.
- **Mod-Manager**: Mods, Shader und Ressourcenpakete von Modrinth suchen, installieren, an/aus schalten, entfernen und mit einem Klick aktualisieren. Wechselst du die Version eines Profils, werden die Mods automatisch angepasst.
- **Profile**: getrennte Instanzen mit eigenen Mods, Welten und Einstellungen.
- **Accounts**: Microsoft-Login (auch mehrere Accounts) plus Offline-Accounts.
- **Java automatisch**: Für jede Minecraft-Version wird die richtige Java-Version (8, 17, 21 …) direkt von Mojang geladen.
- **RAM- und JVM-Einstellungen**: RAM-Regler, optimierte G1GC-Flags gegen Ruckler, eigene JVM-Argumente, Fenstergröße.
- **Discord-Status** (optional): zeigt „Minecraft 1.21.1 · Fabric“ an.
- **Spiel-Log** live im Launcher.
- **Schnell**: parallele Downloads (Standard 32), Dateien werden nur einmal geladen und von allen Profilen geteilt.

## Installation

Fertige Installer (Windows `.exe`, macOS `.dmg`) erzeugt GitHub Actions automatisch:

- Unter **Actions → Build** beim letzten Lauf unter *Artifacts*, oder
- als Release, sobald ein Tag wie `v0.1.0` gepusht wird (`git tag v0.1.0 && git push --tags`).

> macOS: Die App ist nicht von Apple signiert. Beim ersten Start daher Rechtsklick → *Öffnen*.
> Windows: SmartScreen ggf. mit *Weitere Informationen → Trotzdem ausführen* bestätigen.

## Microsoft-Login einrichten (einmalig)

Microsoft verlangt, dass jeder Launcher eine eigene App-ID hat. Das ist kostenlos:

1. Auf [portal.azure.com](https://portal.azure.com) anmelden → **App-Registrierungen** → **Neue Registrierung**.
2. Name: z. B. „A Better Lunar Client“. Kontotypen: **Nur persönliche Microsoft-Konten**. Umleitungs-URI leer lassen.
3. In der App unter **Authentifizierung** die Option **Öffentliche Clientflows zulassen** auf **Ja** stellen und speichern.
4. Die **Anwendungs-ID (Client-ID)** kopieren.
5. Die ID bei Mojang für die Minecraft-API freischalten lassen: [aka.ms/mce-reviewappid](https://aka.ms/mce-reviewappid). Die Prüfung kann einige Tage dauern.
6. Die ID im Launcher unter **Einstellungen → Microsoft-Login** eintragen.
   Alternativ als GitHub-Secret `ABLC_MS_CLIENT_ID` hinterlegen, dann ist sie in jedem Build schon eingebaut.

Offline-Accounts lassen sich (wie bei Prism Launcher) erst anlegen, nachdem ein Microsoft-Account mit gekauftem Minecraft hinzugefügt wurde.

## Discord-Status einrichten (optional)

1. Im [Discord Developer Portal](https://discord.com/developers/applications) eine Anwendung „A Better Lunar Client“ anlegen.
2. Unter *Rich Presence → Art Assets* das Logo (`assets/logo.svg` als PNG) mit dem Namen `logo` hochladen.
3. Die **Application ID** in den Einstellungen eintragen (oder als Secret `ABLC_DISCORD_APP_ID`) und den Discord-Status einschalten.

## Selbst bauen

Voraussetzungen: [Rust](https://rustup.rs), [Node.js 20+](https://nodejs.org) und die [Tauri-Voraussetzungen](https://v2.tauri.app/start/prerequisites/) für dein System.

```bash
npm install
npm run tauri dev      # Entwicklungsmodus
npm run tauri build    # Installer bauen
cargo test --manifest-path src-tauri/Cargo.toml
```

## Aufbau

```
src/                    Oberfläche (TypeScript, ohne Framework, ~27 KB)
  pages/                Spielen, Profile, Mods, Accounts, Einstellungen
src-tauri/src/
  minecraft/            Versionen, Downloads, Natives, Assets, Java, Startbefehl
    loader.rs           Fabric / Legacy Fabric
  auth.rs               Microsoft-Login (Device Code → Xbox → Minecraft)
  mods.rs               Modrinth: Presets, Suche, Installation, Updates
  profiles.rs           Profile / Instanzen
  discord.rs            Discord-Status
```

Daten liegen im App-Datenordner (Windows: `%APPDATA%\io.github.satoshisson.ablc`, macOS: `~/Library/Application Support/io.github.satoshisson.ablc`). Libraries, Assets und Java werden unter `meta/` von allen Profilen geteilt, jedes Profil hat unter `instances/` seinen eigenen Spielordner.

## Hinweise

- Kosmetik, Emotes und die eigenen HUD-Mods von Lunar sind Closed Source und nicht enthalten.
- Auf Servern wie Hypixel sind reine Performance- und Optik-Mods (Sodium, Iris, Zoom) üblich und erlaubt. Prüfe trotzdem die Regeln des jeweiligen Servers.
- Für 1.8.9 (Legacy Fabric) gibt es auf Modrinth nur wenige Mods; nicht verfügbare Preset-Mods werden beim Anlegen angezeigt und übersprungen.

## Lizenz

MIT. Nicht offiziell mit Mojang, Microsoft oder Lunar Client verbunden.
