# Changelog

Versions follow [SemVer](https://semver.org/): `0.MINOR.PATCH`.
A new feature bumps MINOR (0.1.0 → 0.2.0), a fix bumps PATCH (0.1.0 → 0.1.1).
Every version has a matching git tag `vX.Y.Z` and a release on GitHub.

## [0.10.1] — build fix
- Fixed the failing `cargo test` in CI: the test `light_palettes_are_detected` used the wrong palette numbers (Sakura is number 7, Sand is 8, Neon is dark). Only the test was wrong, the launcher itself drew the palettes correctly
- The stroke widths are now written as `f32` explicitly. The newest Rust compiler warned that the old form will stop compiling in a future release
- The release tag has to match the version in `Cargo.toml`: for this version the tag is `v0.10.1`

## [0.10.0] — Microsoft sign-in
- A separate **Microsoft account** block at the top of **Accounts**: sign in with a licensed Minecraft account and play on online-mode servers
- Sign-in through Microsoft's own page with a short code (device code flow): the launcher never sees the password. The code can be copied, the page opened with one button, and the sign-in cancelled at any time
- The sign-in is renewed silently before each launch, so it does not have to be repeated. A broken or expired sign-in is reported before the long downloads start
- The game is started with the real Minecraft token, name and UUID of the profile. The token is hidden in the launch log, and the temporary @argfile that could contain it is deleted when the game exits
- The tokens are stored in a separate file per account (`auth/<uuid>.json`, owner-only on Linux and macOS), not in `config.json`. Deleting the account deletes the file
- Clear messages for the usual problems: no Xbox profile, child account, unavailable country, no Minecraft purchase, an application that Mojang has not approved
- The Azure application ID is entered in **Accounts → Microsoft account** or built in with the `FERRUM_MS_CLIENT_ID` environment variable; the release workflow passes the repository variable of the same name. It has to be approved by Mojang, see README → Microsoft sign-in
- Accounts show their type (Microsoft, offline, temporary). The Skin button is hidden for Microsoft accounts, their skin is changed on minecraft.net
- The terms of use mention Microsoft sign-in and how the tokens are stored, so they are shown again after the update

## [0.9.1] — new avatar set
- Added avatars: Axolotl, Cat and Wolf
- Removed the Steve and Alex avatars. An account that had one of them now shows **Auto** (the face of the skin, or the first letter of the nickname); nothing else changes, and the account can pick a new avatar at any time
- 11 avatars in total; `assets/avatars-preview.png` is updated

## [0.9.0] — account avatars
- A new **Avatar** button in **Accounts**: choose a head for the account from 10 presets (Steve, Alex, Creeper, Zombie, Skeleton, Enderman, Pig, Villager, Slime, Blaze) or **Auto** (the face of the skin, or the first letter of the nickname)
- The chosen avatar is saved with the account (`avatar` field in `config.json`) and is shown in the side panel and in the accounts list. Old configs work as before: no avatar means Auto
- The heads are simplified 8×8 pixel art drawn in code (`src/avatar.rs`); `assets/avatars-preview.png` shows all of them
- Tests check that every picture is well formed (8×8, every character is in the palette, ids are unique)
- GitHub Actions: the release is now published by one separate job after all builds finish (four parallel jobs could race and leave duplicate or incomplete releases)
- GitHub Actions: the release workflow checks that the tag matches the version in `Cargo.toml`
- GitHub Actions: builds only get read access to the repository, only the publishing job can write; actions are updated to current major versions (checkout 6, upload-artifact 7, download-artifact 8, action-gh-release 3, attest-build-provenance 4)
- GitHub Actions: CI cancels outdated runs for the same branch and can be started by hand

## [0.8.0] — full English translation
- The whole launcher interface is translated to English: sidebar and pages, all windows and dialogs, buttons, hints, status messages and the terms of use shown on first start
- Error messages, the launch log and the crash hints are in English too
- Play time and "last launched" are shown in a short English form: `12h 30m`, `5 min ago`
- Background palettes and accents have English names: Graphite, Midnight, Pine, Burgundy, Light, AMOLED, Ocean, Sakura, Sand, Neon; Rust, Patina, Brass, Cobalt, Heather, Rose, Lime, Cyan, Crimson
- README, the terms of use (`TERMS.md`), this changelog and the comments in the source code are translated as well
- README no longer contains the section about publishing a release to GitHub

## [0.7.0] — play time, new looks, code hardening
- Play time tracking for every instance: on the card, in the selected instance panel and a total on the page. The time is written every 30 seconds and is reconciled exactly when the game exits, so it is not lost even if you close the launcher
- Launch counter for every instance
- 5 new background palettes: AMOLED, Ocean, Sakura, Sand, Neon (10 in total) and 4 new accents: Rose, Lime, Cyan, Crimson (9 in total)
- Corner style: sharp, soft, round
- The accent glow at the top of the page can be turned off
- An instance cannot be deleted while it is launching or the game is running in it
- Background threads are protected from panics: on an internal error the launcher shows it instead of hanging on "Preparing…"
- Downloads are retried with a pause after network failures
- Old launch and crash logs are removed automatically (the last 40 launches and 20 crashes are kept)
- New instances get a guaranteed unique id
- Places where corrupted data could cause a panic are fixed (hash slices, poisoned locks)

## [0.6.1] — fixes
- Fixed the launch of NeoForge and new Forge versions: the error `Modules minecraft and _1._21._1 export package…`. The vanilla jar no longer ends up on the classpath for these loaders
- Natives libraries are taken only for your platform and architecture. Before, the DLLs for x64, arm64 and x86 overwrote each other. Old natives folders are recreated automatically
- Library paths no longer mix `/` and `\`
- Version compatibility: old configs are read by the new version, and on a format mismatch a copy `config.backup-…json` is made and everything that can be read is kept
- Unknown fields from configs of other versions are preserved on save
- Instances created by old versions do not get Sodium and OptiFine installed on their own
- The config is written through a temporary file, an interrupted write does not corrupt the old one
- A very long command line on Windows is passed through an @argfile
- On a crash the error window suggests a likely cause (memory, Java version, mod conflict, etc.)
- The author details are embedded into the `.exe`: ArturOriginos, description, copyright and manifest
- Releases now include files with SHA-256 checksums and build provenance attestation (GitHub attestation)

## [0.6.0] — modpack export and skins
- Export an instance to `.mrpack`: the "Export" button on the "Instances" page
- Mods are found on Modrinth by checksum and go into the archive as links, not as files
- Optionally the archive also gets mods that are not on Modrinth, settings, resource packs, shaders, `options.txt`
- Skins for offline accounts: PNG 64×64 or 64×32, the "Skin" button in "Accounts" or dragging a file into the window
- The face of the skin is shown in the account avatar
- In the game the skin is shown by the CustomSkinLoader mod, which the launcher installs itself for instances with Fabric, Quilt, Forge and NeoForge

## [0.5.0] — modpack import
- Import of Modrinth (`.mrpack`) and CurseForge (`.zip`) modpacks: the "Import" button on the "Instances" page or dragging a file into the window
- Before importing, the name, game version, loader and number of files are shown
- The loader version is exactly the one from the modpack
- Mods are downloaded in parallel with checksum verification, settings from `overrides` are copied into the instance
- Files that failed to download do not break the import: they go into the report
- Protection: paths are checked for escaping the instance folder, for `.mrpack` only `cdn.modrinth.com`, `github.com`, `raw.githubusercontent.com`, `gitlab.com` are allowed
- An unfinished instance is deleted on error

## [0.4.2] — CurseForge key
- Removed the ability to embed a CurseForge key at build time: a shared key in a public file violates the CurseForge rules and can be extracted from the program. Now the key belongs only to the user, in the settings

## [0.4.1] — tests
- Automated logic tests: Java versions, library rules, profile merging, argument substitution, UUIDs, temporary accounts, image limits
- GitHub Actions runs `cargo test` on Windows, Linux and macOS

## [0.4.0] — custom images
- Custom interface background: any picture and an adjustable dimming
- Custom launcher icon (window, taskbar, side menu)
- Pictures can be dragged right into the launcher window
- Icon limits: PNG, JPG or WebP, up to 5 MB, strictly square, from 128×128 to 2048×2048 px
- Background limits: PNG, JPG or WebP, up to 10 MB, from 800×450 to 7680×4320 px
- Clear messages when a picture does not fit

## [0.3.0] — new interface and color customization
- Redesigned interface: a side menu with icons, instance cards with an emblem and colored loader tags, smooth hover, a "Play" button with a glow
- Custom interface color: 5 ready-made accents and any color from a palette
- 5 background palettes: Graphite, Midnight, Pine, Burgundy, Light
- Interface scale from 90% to 150%
- A "Reset appearance" button

## [0.2.0] — icon, logs and optimization
- New application icon (window, taskbar, `.exe` on Windows)
- Terms of use on first start
- A log of every launch in `logs/launches`
- On a game crash a separate folder in `logs/crashes`: the launch log, `latest.log`, `crash-reports`, `hs_err`
- Automatic installation of Sodium (Fabric, Quilt, NeoForge) and OptiFine (Forge), turned on with a checkbox in the instance
- "Open logs folder" buttons in the settings and in the error window

## [0.1.0] — first release
- Launching Minecraft: Vanilla, Fabric, Quilt, Forge, NeoForge
- Instances with any name, their own folder, RAM and JVM arguments for each
- Mods from Modrinth and CurseForge right in the launcher (with required dependencies)
- Enabling, disabling and deleting installed mods
- Offline accounts and temporary accounts (deleted after 24 hours)
- Automatic selection and download of the required Java (8 / 17 / 21)
- Settings: memory, window size, fullscreen, minimizing on launch, interface color
- Release builds for Windows, Linux and macOS through GitHub Actions

## Plan
- 0.11.0 — one-click mod updates
- 1.0.0 — stable release
