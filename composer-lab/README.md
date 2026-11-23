# composer-lab

Isolated prototype of a two-guitar composer (Python 3.12, standard library
only; not a Cargo workspace member). It exists to answer one question with
audio: can a hierarchical motif-development generator write guitar music worth
keeping, where the current S6/Swang path cannot?

## Generate

```
python composer.py --seeds 101-120 --candidates 6 --out OUT --prefix new
python composer.py --seeds 5 --meter 7/8 --mode lydian --key E --tempo 150 --out OUT
python composer.py --recipe OUT/new_005.recipe.json --out OUT2   # rebuild or edit a piece
python composer.py --seeds 101-120 --out OUT --prefix shuf --control shuffle   # pitch-randomised control
```

Per seed it writes `.mid` (type 1, one track per guitar plus a palm-mute
track), `.musicxml` (tablature with string/fret, tap, hammer-on, pull-off,
slide, palm mute), `.tab.txt` (ASCII tab) and `.recipe.json` (every sampled
decision; editing it and passing `--recipe` rebuilds the piece). The manifest
lists the critic row of every candidate considered.

## Mechanism

1. Intent: meter and accent grouping (4/4, 7/8, 6/8, 5/4; e.g. 3+3+2+3+3+2),
   mode, key, tempo, which guitar leads.
2. Form: intro | A sentence (statement, sequence, fragment, cadence) | A'
   (variation, call and response) | B contrast (palm-mute riff from its own
   grouping, lead plays the hook inverted and augmented) | A'' climax (hook
   with parallel diatonic thirds) | final chord.
3. Harmony: diatonic Markov progression with phrase-end targets; A must touch
   the tonic.
4. Motif: a 2-4 note interval cell sequenced across the meter's groups; one
   anchor per phrase so variation comes from harmony and operators, not from
   re-planing; accented and phrase-final notes snap to chord tones.
5. Second guitar roles: arpeggio ladder with a common-tone upper shape,
   pedal-tone interleave, or a single-string tapping triad; all yield to the
   lead's attacks except on accents (interlock).
6. Fretboard: Viterbi over (string, fret, hand position), then techniques are
   derived from the chosen positions.
7. Selection: 6 candidates per seed, best by a simple critic (clashes,
   fingering cost, climax position).

## Audition

`render/audition_midi.py` forces every channel to program 0 and
`render/render.sh` renders with FluidSynth through the CC0 FreePats
"EGuitar FSBS clean" SoundFont plus sox reverb, the same for every arm.

## Tests

`python -m unittest test_composer` — byte determinism, string/fret
consistency, legato only on one string, taps above the fretting hand, rare
semitone clashes, every MusicXML measure filling its meter.

## Studio

Local web tool for section-level curation over v8 pieces (`127.0.0.1:8765`,
standard library only).

Windows (PowerShell):

```powershell
cd composer-lab
python -m studio.server --port 8765 --out out_studio
```

WSL (NixOS):

```bash
cd composer-lab
nix shell --offline nixpkgs#python312 -c python3 -m studio.server --port 8765 --out out_studio
```

Open `http://127.0.0.1:8765` in a browser. Usage log summary across all curated
pieces:

```bash
python -m studio.summary out_studio
```

### Как пользоваться (для владельца)

Нажмите «Новая пьеса» (при желании укажите seed), дождитесь короткого рендера и
слушайте пьесу в плеере; клик по любому цветному блоку на таймлайне под плеером
сразу перематывает на выбранную секцию. Если часть не зашла, перегенерируйте
только нужный слой в этой секции (ритм-гитару, фактуру, вариант середины,
вступление, концовку) или весь риф / темп целиком — всё остальное останется
нетронутым; удачные слои закрепите кнопкой «Закрепить» (на закреплённом слое
она показывает «🔒 Закреплено», повторное нажатие открепляет), после чего можно
нажать «Перегенерировать всё незакреплённое». Снизу настраиваются глухие ноты
ритм-гитары, доля октавных тактов («Авто | Вручную»; если сдвинуть ползунок в
«Авто», включится «Вручную»), длина вступления, повтор квадрата в куплетах,
соло в середине B и пауза в конце такта. Все эти параметры действуют на всю
пьесу, в том числе на закреплённые секции, и применяются сразу; дозы
перегенерации ритма вынесены в раздел «Дополнительно» и действуют только при
следующей перегенерации ритм-гитары. Ползунок отправляет значение один раз за
жест: при отпускании мыши или пальца, а при управлении с клавиатуры — через
0,6 с после последнего нажатия. Пока идёт пересчёт, внизу экрана горит плашка
«Пересчёт…», а нажатая кнопка неактивна; ошибки показываются красной строкой
вверху рабочей области (её можно закрыть). Кнопка «Отменить» называет то, что
она отменит (например, «Отменить: глухие ноты 0.3»), а «Вернуть» повторяет
последнее отменённое действие; любое новое изменение сбрасывает «Вернуть». На
узких экранах названия секций на таймлайне сокращены («Вст.», «A», «B»,
«Кульм.», «Конц.»), полное имя видно во всплывающей подсказке. «Скачать MIDI» и
«Скачать табы (MusicXML)» сохраняют готовый трек для DAW и Guitar Pro.

Стек «Вернуть» хранится в `redo.json` рядом с `undo.json` в папке пьесы и
доступен как `POST /api/piece/{id}/redo`; в журнал `history.jsonl` пишется
действие `redo`. Оба стека ограничены 50 шагами.

## Studio Web

Static browser version of Studio (`composer-lab/web/`): runs the Python generator directly in the browser via Pyodide (`v0.27.7` from `cdn.jsdelivr.net`) inside a Web Worker and plays pieces with a Web Audio multi-velocity MP3 sampler (`web/samples/`, rendered from FreePats EGuitarFSBS clean). Pieces and usage logs are stored in IndexedDB with JSON export/import and `history.jsonl` download (`studio/summary.py` compatible).

Build the self-contained Python bundle (`web/py/`, git-ignored) and verify the site:

```bash
cd composer-lab
python3 web/build.py
python3 web/check_site.py
```

Run locally:

```bash
cd composer-lab/web
python3 -m http.server 8000
```

Open `http://127.0.0.1:8000` in a browser. Under WSL (NixOS), replace `python3` with `nix shell --offline nixpkgs#python312 -c python3`.

Rebuild the 98 guitar samples (`web/samples/`, pitches 40–88 at velocities 72 and 100):

```bash
cd composer-lab/web
nix shell nixpkgs#fluidsynth nixpkgs#sox nixpkgs#ffmpeg -c bash build_samples.sh
```

### Как пользоваться в браузере (для владельца)

Откройте страницу на компьютере или телефоне: при первом запуске браузер загрузит Pyodide и сэмплы гитары (далее они берутся из кеша браузера). Нажмите «Новая пьеса» (при желании задайте `seed`), запускайте воспроизведение кнопкой «Играть» или кликом по блокам секций на таймлайне. Перегенерируйте отдельные слои, закрепляйте удачные кнопкой «Закрепить» (параметры снизу всё равно действуют и на закреплённые секции), отменяйте и возвращайте шаги кнопками «Отменить: …» и «Вернуть», сразу меняйте глухие ноты ритм-гитары, долю октавных тактов, длину вступления, повтор квадрата в куплетах и прочие параметры снизу (дозы перегенерации скрыты в «Дополнительно») и скачивайте готовый результат в MIDI, MusicXML или ASCII-табах. Стек «Вернуть» хранится в состоянии пьесы рядом со стеком «Отменить» (до 50 шагов, команда `redo` в `web_api`). Все пьесы и журнал действий сохраняются локально в браузере; в боковой панели доступны «Скачать журнал» (`history.jsonl`), «Экспорт пьес» и «Импорт пьес» для переноса между устройствами.


