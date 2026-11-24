# Studio interface: controls that say what they do (specification)

Base: branch `claude/studio-controls` (PR #222). Applies to both front ends,
`studio/index.html` (server page) and Studio Web (`web/index.html`,
`web/app.js`); keep them consistent. No change to the music engine
(`composer.py`, `studio/engine.py` recipes) unless an item says so; all
version and default outputs stay byte-identical.

## Decisions taken by the owner

- Octave share: a two-way switch "Авто | Вручную".
- Locks keep today's meaning (piece-wide parameters still apply to locked
  sections); the UI must say so.

## Required changes

1. **Octave share switch.** Replace the single "Авто" toggle with two buttons
   "Авто" and "Вручную" plus the slider. Clicking the already selected button
   does nothing (no request, no undo entry). "Вручную" applies the slider's
   shown value. Moving the slider while in Auto switches to manual with the
   slider value; the slider stays usable in Auto but is shown dimmed, and the
   hint says that moving it switches to manual.
2. **Locks and piece parameters.** Under the "Параметры пьесы" heading add one
   line: "Параметры действуют на всю пьесу, в том числе на закреплённые
   секции." Same text in both front ends.
3. **Lock and toggle buttons show the action and the state clearly.** Lock
   button: "Закрепить" when unlocked, "🔒 Закреплено" when locked (title
   "Открепить"), with `aria-pressed`. The trailing-rest control becomes a
   two-button choice like the others: "Пауза в конце такта" / "Без паузы".
   A disabled re-roll button of a locked layer gets the title "Слой
   закреплён".
4. **Re-roll-only options.** In "Дополнительно" add one hint line at the top:
   "Эти параметры действуют только при следующей перегенерации ритм-гитары
   («Перегенерировать ритм-гитару» или «Перегенерировать всё
   незакреплённое»)."
5. **Slider commits.** A slider sends its value once per user gesture: on
   pointer release, and for keyboard changes after 600 ms without further
   key presses (debounce). Rapid arrow presses give one request and one
   undo entry.
6. **Busy feedback where the user is.** While a request for the current piece
   is running, show "Пересчёт…" in a fixed, non-blocking status chip at the
   bottom of the viewport (visible on phones while scrolled to the
   parameters) and disable the control that was pressed. The existing
   sidebar indicator may stay.
7. **No `alert()`.** Show errors in a dismissible inline error banner at the
   top of the workspace (and keep the existing load banner for loading
   errors). Both front ends.
8. **Undo says what it undoes; add redo.** The undo button's title (and
   visible text on wide screens) names the last action, e.g. "Отменить:
   глухие ноты 0.3", "Отменить: перегенерация «Куплет A · ритм-гитара»".
   Add "Вернуть" (redo) that re-applies the last undone state; any new change
   clears the redo stack. Studio Web: redo stack lives in the piece state
   next to `undo` (bounded like undo) and in `web_api` as `redo`; server
   Studio: `redo.json` next to `undo.json` and `/api/piece/{id}/redo`.
   History log records "redo" like "undo".
9. **Plain labels.** Remove internal names from visible text:
   "Соло в середине B" (no "(b_lead)"), its buttons "Медленнее
   (удвоенные длительности)", "В темпе рифа", "Без соло"; trailing rest as
   in item 3 (no "trailing_rest", no "--notail"). The "#N" badge on a re-roll
   button gets a title "Сколько раз перегенерировали" and is hidden at 0.
   The seed field gets a visible label "Номер пьесы (seed), необязательно".
10. **Two muted-note controls.** Rename the advanced one to "Глухие удары
    перед акцентами (только в октавных тактах, при перегенерации)".
11. **Timeline on phones.** Section names must not be cut to unreadable
    stubs at 360 px: use short labels on narrow screens (e.g. "Вст.", "A",
    "A2", "B", "Кульм.", "Конц.") with the full name in `title` and
    `aria-label`.
12. **Accessibility.** Every slider has an accessible name (`aria-label` or
    an associated `<label>`) and `aria-valuetext` with the shown value;
    every choice button group uses `aria-pressed` (or radio semantics) so the
    selected option is not conveyed by colour alone; the status chip of item
    6 is `role="status"`.

## Constraints

- Python standard library on the server side; plain JS in the browser, no
  new dependencies or CDNs.
- Existing tests stay green; `test_composer.py` not edited; version hashes
  unchanged (reviewer's script).
- Russian UI text, English code, minimal comments. Work only in
  `composer-lab/`. Do not commit or push.

## Tests (write first)

- Node harness (`web/tests/`): each item 1, 3, 5, 6, 7, 8, 9, 11, 12 has a
  test (e.g. clicking selected "Авто" sends nothing; three arrow presses
  produce one option request; an error shows the inline banner and no
  `alert`; undo title names the action; redo restores the undone state and
  a new change clears it).
- Python: `web_api.redo` and server redo (state transitions, bounds, history
  log), no-op cases; server page markup tests for items 2, 3, 4, 9, 10.
- Byte-identical defaults and version outputs as before.

## Deliverables

Code, tests, README sections updated for the new controls, and a short report:
what was built, commands and results, anything not done.
