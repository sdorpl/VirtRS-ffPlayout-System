# VizRS ffPlayout System

Fork [ffplayout](https://github.com/ffplayout/ffplayout) (Rust + FFmpeg, panel Vue 3) rozwijany jako **playout kanału 24/7 z brandingiem VizRS**
(w planie nazwany „VizRS Playout”). Odtwarza playlisty dnia i na ich podstawie steruje grafiką serwera `vizrs` z repo [../VizRS CG System](../VizRS%20CG%20System).
Plan i decyzje: `../VizRS CG System/PLAN-kanal.md`, §11–§12 (wcześniejsze sekcje o `vizrs-channel`/Tunarr/ErsatzTV są nieaktualne).

Język pracy z użytkownikiem i nowej dokumentacji VizRS: **polski**. Kod, komentarze, `docs/*.md` i tłumaczenia UI z ffplayout są po angielsku — pisz zgodnie z otoczeniem
(nowe teksty UI dodawaj do `frontend/src/locales/`, a nie na sztywno).

## Zacznij każdą sesję tutaj

1. `git log -5`, `git status`, przeczytaj §11–§12 `PLAN-kanal.md` (etapy P0–P5).
2. Przy zmianach w `crates/` stosuj [crates/AGENTS.md](crates/AGENTS.md) (architektura, układ importów, puste linie, testy) — to zasady upstreamu, obowiązują.
3. Budowa, FFmpeg, feature'y: [docs/developer.md](docs/developer.md).

## Niepodważalne zasady

- **Licencja GPL-3.0** zostaje (`LICENSE`); nie usuwaj informacji o autorach ffplayout (`authors` w `Cargo.toml`, nota w `README.md`).
- **Z VizRS tylko przez protokół**: TCP 6100 (komendy TAKE/OUT/UPDATE/…), HTTP `/api/*`, SSE `/events`, wyjście `/output`. Nie kopiuj kodu VizRS tutaj ani GPL-owego kodu tam (inna licencja).
- **Środowisko docelowe**: Docker na Oracle Cloud Free, Ampere A1 — `linux/arm64`, bez GUI i GPU. Kodowanie programowe (libx264), nakładka `/output` z bezgłowego Chromium (`--headless=new --disable-gpu`) nakładana filtrem `overlay`. Nie wprowadzaj zależności od GPU/NVENC/VAAPI na ścieżce emisji; pamiętaj o budżecie CPU.
- **Kanał**: domyślnie 25 fps (opcjonalnie 50), 1080p albo 720p; czas w UI `hh:mm:ss:ff`, reguły trzymają czas w ms.
- **Fork blisko upstreamu**: zdalne `upstream` = ffplayout. Zmiany VizRS rób w wydzielonych modułach/plikach, gdzie się da, żeby merge z upstreamu był prosty. Nie przebudowuj kodu ffplayout bez potrzeby.
- **Podział crate'ów**: `crates/engine` (`ff-engine`, przetwarzanie mediów) nie zależy od `crates/app`. SQL tylko w `crates/app/src/db/handles/`; schemat bazy wyłącznie przez nowe pliki w `migrations/` (nie edytuj istniejących migracji).
- Błędy przez `Result` z kontekstem, bez `unwrap`/`expect` na ścieżkach produkcyjnych — emisja ma trwać mimo błędów grafiki/VizRS.

## Uruchamianie

```bash
cargo run                                   # serwer + panel na http://127.0.0.1:8787 (pierwsze uruchomienie: kreator konfiguracji)
npm ci && npm run dev                       # panel Vite z proxy /api, /auth, /hls… do :8787
npm run build                               # frontend/dist (wbudowywany w binarkę release z feature embed_frontend)
cargo run -- --help                         # opcje CLI (crates/app/src/utils/args_parse.rs)
```

Wymagane biblioteki FFmpeg ≥ 7.0 (dev) i `pkg-config` — patrz `docs/developer.md`. Domyślne feature'y app: `desktop`, `embed_frontend`;
na serwer bez GUI buduj `--no-default-features --features embed_frontend`. Kontenery: `docker/*.Dockerfile`, `scripts/build.sh`.

Do testów ręcznych używaj osobnej bazy i katalogów (`--db`, `--storage`, `--logs`, `--public` w katalogu tymczasowym), żeby nie ruszać danych użytkownika.

## Testy (bramka przed commitem)

```bash
cargo fmt --all --
cargo clippy --all-features --all-targets -- --deny warnings
cargo test --no-default-features            # jak w CI
npm run type-check && npm run lint
node --test frontend/tests/                 # testy *.test.mjs frontendu
```

Przy pracy nad panelem **zobacz wynik** (zrzut ekranu; pliki PNG poza repo).

## Struktura

| Ścieżka | Rola |
|---|---|
| `crates/app/src/api/` | REST (`routes/` wg domen, `auth.rs`) |
| `crates/app/src/db/` | SQLite: `models.rs`, `handles/` (wszystkie zapytania) |
| `crates/app/src/player/` | sterowanie playoutem kanału |
| `crates/app/src/sse/`, `serve/`, `moq/` | SSE, pliki statyczne/frontend, odtwarzacz MoQ |
| `crates/app/src/utils/` | CLI (`args_parse.rs`), konfiguracja, playlisty, generator, powiadomienia, task runner |
| `crates/app/tests/` | testy integracyjne (API, playlisty) |
| `crates/engine/src/` | `ff-engine`: wejścia, kompozytor, mikser audio, wyjścia (HLS/stream/desktop), analiza |
| `frontend/src/` | panel Vue 3 + TS + Tailwind (`views/`, `components/`, `stores/` Pinia, `locales/`) |
| `migrations/` | migracje SQL (kolejne numery) |
| `docs/` | dokumentacja ffplayout (EN) |
| `tests_assets/` | media i playlisty testowe |
| `docker/`, `scripts/`, `debian/`, `assets/` | budowa obrazów/paczek, usługa systemd, konfiguracja domyślna |

## Commity

- Jedna spójna zmiana = jeden commit, po polsku, opisowo. Tylko zielone stany.
- Pliki dodawaj jawnie (`git add ścieżka`), nie `git add -A`. Nie commituj `target/`, `node_modules/`, `frontend/dist/`, zrzutów ani plików tymczasowych.
- Zmiana zachowania → aktualizacja odpowiedniego pliku w `docs/` (lub `README.md`) w tym samym commicie.
- Commit i push tylko na prośbę użytkownika.
