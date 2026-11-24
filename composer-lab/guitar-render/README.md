# guitar-render (GUITAR-RENDER-01 experiment)

Offline render of composer-lab guitar MIDI into amped electric guitars:
MIDI + MusicXML -> articulation JSON -> per-stem MIDI with keyswitches -> sfizz DI ->
NAM amp (NAudio.Nam) -> cabinet IR -> EQ / delay / reverb -> stereo mix.
Findings and measurements are in `PROBE.md`. Nothing here is used by Griff production code.

Large inputs and all audio live outside git in `_griff_out/guitar-render-01/` (sample libraries,
NAM captures, IRs, NuGet packages, the built `namrender`, renders).

## Steps

1. Host, Windows Python, standard library only:

   ```
   python prepare.py --input song.mid --musicxml song.musicxml --song v8_002 --mode strict --out PREP
   python prepare.py --input g1.mid --input g2.mid --input g3.mid --song v8_002 --out PREP   # separate files
   python smoke.py --instrument bg_green --out PREP_SMOKE
   ```

   Options: `--mode expressive`, `--preset-all SWAN-HEAVY`, `--preset guitar3=ALT-HEAVY-LITE`,
   `--instrument guitar1=bg_green`, `--stems guitar3`, `--no-fx`, `--di-only`.
   Output: `midi/<stem>.mid`, `articulations.json`, `midi_checks.json`, `plan.json`.

2. Build `namrender` once (compiles against NAudio.Nam; runs nothing):

   ```
   dotnet publish namrender -c Release -o _griff_out/guitar-render-01/bin/namrender --packages _griff_out/guitar-render-01/assets/nuget-packages
   ```

3. WSL (NixOS): render in the offline auditbox VM, one VM for several plans:

   ```
   REPEATS=3 bash jobs/render_job.sh RESULT_DIR PREP [PREP...]
   ```

   Per plan and run: `guitarN.di.wav`, `guitarN.processed.wav` (float32 mono 48 kHz),
   `guitars.mix.wav` (PCM24 48 kHz stereo), `guitars.mix.mp3`, `render-manifest.json`, `render.log`.
   A prep dir with `extra.sh` runs extra guest commands (FluidSynth baseline, A/B clips via `make_clips.py`).

One-off jobs: `jobs/fetch_wheels.sh` (auditbox fetch mode, Python wheels), `jobs/probe.sh` (tool probe).

## Files

- `grender/smf.py` MIDI reader/writer with a full tempo map; `grender/mxl.py` composer-lab MusicXML reader;
  `grender/ir.py` articulation JSON and MIDI checks; `grender/articulate.py` STRICT / EXPRESSIVE mapping.
- `instruments/*.json` per-instrument keyswitches and articulation map (`unirock.json` is mapping only).
- `presets/chains.json` SWAN-* and ALT-* chains; `presets/assets.json` NAM and IR files; `presets/songs.json` stem roles.
- `render.py`, `analyze.py`, `make_clips.py` run inside the guest.
- `unirock/` draft Windows Sandbox job for the Unirock backend; not run.
