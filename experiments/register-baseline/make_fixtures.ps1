# Regenerates the two synthetic register-baseline MIDI fixtures.
# Format-0 SMF, PPQ 480, quarter notes; tempo + time-sig meta are required or
# griff's MIDI importer rejects the file ("no tempo event found").
param([string]$OutDir = "$PSScriptRoot\fixtures")
New-Item -ItemType Directory -Force $OutDir | Out-Null
function Write-Midi($path, [int[]]$pitches){
  $ev = New-Object System.Collections.Generic.List[byte]
  $ev.AddRange([byte[]]@(0x00,0xFF,0x58,0x04,0x04,0x02,0x18,0x08))   # time sig 4/4
  $ev.AddRange([byte[]]@(0x00,0xFF,0x51,0x03,0x07,0xA1,0x20))        # tempo 120 (500000us)
  foreach($p in $pitches){
    $ev.AddRange([byte[]]@(0x00,0x90,$p,0x64))                        # note on, delta 0
    $ev.AddRange([byte[]]@(0x83,0x60,0x80,$p,0x40))                   # note off, delta 480
  }
  $ev.AddRange([byte[]]@(0x00,0xFF,0x2F,0x00))                        # end of track
  $len = $ev.Count
  $hdr = [byte[]]@(0x4D,0x54,0x68,0x64,0,0,0,6,0,0,0,1,0x01,0xE0)     # MThd fmt0 1trk div480
  $trk = [byte[]]@(0x4D,0x54,0x72,0x6B, ([byte](($len -shr 24)-band 0xFF)), ([byte](($len -shr 16)-band 0xFF)), ([byte](($len -shr 8)-band 0xFF)), ([byte]($len -band 0xFF)))
  [System.IO.File]::WriteAllBytes($path, $hdr + $trk + $ev.ToArray())
}
# wide diatonic C major C2(36)..C6(84): 7 classes {0,2,4,5,7,9,11}, span 48
Write-Midi "$OutDir\synth_wide_diatonic.mid"   @(36,38,40,41,43,45,47,48,50,52,53,55,57,59,60,62,64,65,67,69,71,72,74,76,77,79,81,83,84)
# narrow control C major C4(60)..B4(71): same 7 classes, span 11
Write-Midi "$OutDir\synth_narrow_diatonic.mid" @(60,62,64,65,67,69,71)
"wrote fixtures to $OutDir"
