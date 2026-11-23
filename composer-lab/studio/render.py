"""Audio rendering backends for Studio."""
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from render import audition_midi

ROOT = Path(__file__).resolve().parents[1]
SOUNDFONT = os.environ.get(
    "GRIFF_SOUNDFONT",
    str(ROOT.parents[1] / "_griff_out" / "sf" / "fsbs_clean.sf2"),
)


WSL_DISTRO = "NixOS"


def win_to_wsl_path(path):
    s = str(path)
    if s.startswith("\\\\?\\") and len(s) > 6 and s[5] == ":":
        s = s[4:]
    unc = s.replace("/", "\\")
    for prefix in ("\\\\wsl.localhost\\", "\\\\wsl$\\"):
        if unc.lower().startswith(prefix):
            distro, _, rest = unc[len(prefix):].partition("\\")
            if distro.lower() != WSL_DISTRO.lower():
                raise ValueError(f"path is in WSL distribution {distro}, renderer runs in {WSL_DISTRO}")
            return "/" + rest.replace("\\", "/")
    if len(s) >= 2 and s[1] == ":":
        drive = s[0].lower()
        rest = s[2:].replace("\\", "/")
        if not rest.startswith("/"):
            rest = "/" + rest
        return f"/mnt/{drive}{rest}"
    return s.replace("\\", "/")


def fake_render(mid_path, mp3_path):
    """Write a small deterministic MP3 placeholder for tests."""
    mp3_path = Path(mp3_path)
    mp3_path.parent.mkdir(parents=True, exist_ok=True)
    tmp = mp3_path.with_suffix(".mp3.tmp")
    tmp.write_bytes(b"ID3\x03\x00\x00\x00\x00\x00\x00FAKE_MP3")
    tmp.replace(mp3_path)


def default_render(mid_path, mp3_path, soundfont=None):
    """Remap MIDI to program 0 and render WAV -> MP3 via fluidsynth + sox + ffmpeg."""
    sf = soundfont or SOUNDFONT
    mid_path = Path(mid_path).resolve()
    mp3_path = Path(mp3_path).resolve()
    mp3_path.parent.mkdir(parents=True, exist_ok=True)
    render_sh = (ROOT / "render" / "render.sh").resolve()
    to_mp3_sh = (ROOT / "render" / "to_mp3.sh").resolve()

    with tempfile.TemporaryDirectory(dir=str(mp3_path.parent)) as tmpdir:
        tmp = Path(tmpdir)
        aud_mid = tmp / "audition.mid"
        wav_path = tmp / "out.wav"
        rendered_mp3 = tmp / "out.mp3"
        audition_midi.remap(str(mid_path), str(aud_mid))

        if sys.platform == "win32":
            inner = " && ".join([
                f"bash {shlex.quote(win_to_wsl_path(render_sh))} "
                f"{shlex.quote(win_to_wsl_path(sf))} "
                f"{shlex.quote(win_to_wsl_path(aud_mid))} "
                f"{shlex.quote(win_to_wsl_path(wav_path))}",
                f"bash {shlex.quote(win_to_wsl_path(to_mp3_sh))} "
                f"{shlex.quote(win_to_wsl_path(wav_path))}",
            ])
            cmd = [
                "wsl", "-d", "NixOS", "--",
                "nix", "shell", "nixpkgs#fluidsynth", "nixpkgs#sox", "nixpkgs#ffmpeg",
                "-c", "bash", "-c", inner,
            ]
        else:
            inner = " && ".join([
                f"bash {shlex.quote(str(render_sh))} "
                f"{shlex.quote(str(sf))} {shlex.quote(str(aud_mid))} {shlex.quote(str(wav_path))}",
                f"bash {shlex.quote(str(to_mp3_sh))} {shlex.quote(str(wav_path))}",
            ])
            has_tools = all(shutil.which(x) for x in ("fluidsynth", "sox", "ffmpeg"))
            if has_tools:
                cmd = ["bash", "-c", inner]
            else:
                cmd = [
                    "nix", "shell", "nixpkgs#fluidsynth", "nixpkgs#sox", "nixpkgs#ffmpeg",
                    "-c", "bash", "-c", inner,
                ]

        subprocess.run(cmd, check=True, timeout=120)
        final_tmp = mp3_path.with_suffix(".mp3.tmp")
        final_tmp.write_bytes(rendered_mp3.read_bytes())
        final_tmp.replace(mp3_path)
