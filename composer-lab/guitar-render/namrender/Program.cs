using System.Diagnostics;
using System.Text.Json;
using NAudio.Nam;

// namrender <model.nam> <size 0..1> <in.wav> <out.wav> <inGainDb> [<in.wav> <out.wav> <inGainDb>]...
// Input: mono float32 WAV at the model rate. Output: mono float32 WAV, same length, no makeup gain.
if (args.Length < 5 || (args.Length - 2) % 3 != 0)
{
    Console.Error.WriteLine("usage: namrender <model.nam> <size> (<in.wav> <out.wav> <inGainDb>)+");
    return 2;
}
var total = Stopwatch.StartNew();
var load = Stopwatch.StartNew();
var model = NamModel.Load(args[0]);
double size = double.Parse(args[1], System.Globalization.CultureInfo.InvariantCulture);
var sub = model.GetSubmodel(size);
load.Stop();
var results = new List<object>();
for (int i = 2; i < args.Length; i += 3)
{
    var (rate, x) = Wav.ReadMonoFloat(args[i]);
    if (rate != sub.SampleRate) throw new InvalidDataException($"{args[i]}: {rate} Hz, model wants {sub.SampleRate} Hz");
    float g = (float)Math.Pow(10, double.Parse(args[i + 2], System.Globalization.CultureInfo.InvariantCulture) / 20);
    for (int k = 0; k < x.Length; k++) x[k] *= g;
    var proc = new NamProcessor(sub);
    var y = new float[x.Length];
    var sw = Stopwatch.StartNew();
    const int block = 4096;
    for (int k = 0; k < x.Length; k += block)
    {
        int n = Math.Min(block, x.Length - k);
        proc.Process(x.AsSpan(k, n), y.AsSpan(k, n));
    }
    sw.Stop();
    Wav.WriteMonoFloat(args[i + 1], rate, y);
    results.Add(new { input = args[i], output = args[i + 1], frames = x.Length, seconds_audio = (double)x.Length / rate,
        seconds_process = sw.Elapsed.TotalSeconds, rtf = sw.Elapsed.TotalSeconds / ((double)x.Length / rate) });
}
var md = model.Metadata;
Console.WriteLine(JsonSerializer.Serialize(new {
    model = args[0], version = model.Version, architecture = sub.Architecture, a2_full = sub.IsA2Full, a2_lite = sub.IsA2Lite,
    channels = sub.Channels, weights = sub.WeightCount, sample_rate = sub.SampleRate,
    loudness = md?.Loudness, gain = md?.Gain, input_level_dbu = md?.InputLevelDbu, output_level_dbu = md?.OutputLevelDbu,
    load_seconds = load.Elapsed.TotalSeconds, total_seconds = total.Elapsed.TotalSeconds, files = results }));
return 0;

static class Wav
{
    public static (int rate, float[] data) ReadMonoFloat(string path)
    {
        using var br = new BinaryReader(File.OpenRead(path));
        if (new string(br.ReadChars(4)) != "RIFF") throw new InvalidDataException("not RIFF");
        br.ReadInt32();
        if (new string(br.ReadChars(4)) != "WAVE") throw new InvalidDataException("not WAVE");
        int rate = 0, ch = 0, bits = 0, fmt = 0;
        while (br.BaseStream.Position < br.BaseStream.Length)
        {
            string id = new(br.ReadChars(4));
            int len = br.ReadInt32();
            if (id == "fmt ")
            {
                fmt = br.ReadInt16(); ch = br.ReadInt16(); rate = br.ReadInt32();
                br.ReadInt32(); br.ReadInt16(); bits = br.ReadInt16();
                br.BaseStream.Seek(len - 16, SeekOrigin.Current);
                if (fmt == unchecked((short)0xFFFE)) fmt = 3;
            }
            else if (id == "data")
            {
                if (fmt != 3 || bits != 32 || ch != 1) throw new InvalidDataException($"need mono float32, got fmt={fmt} bits={bits} ch={ch}");
                var bytes = br.ReadBytes(len);
                var data = new float[len / 4];
                Buffer.BlockCopy(bytes, 0, data, 0, data.Length * 4);
                return (rate, data);
            }
            else br.BaseStream.Seek(len + (len & 1), SeekOrigin.Current);
        }
        throw new InvalidDataException("no data chunk");
    }

    public static void WriteMonoFloat(string path, int rate, float[] data)
    {
        using var bw = new BinaryWriter(File.Create(path));
        int bytes = data.Length * 4;
        bw.Write("RIFF"u8); bw.Write(36 + bytes); bw.Write("WAVE"u8);
        bw.Write("fmt "u8); bw.Write(16); bw.Write((short)3); bw.Write((short)1); bw.Write(rate);
        bw.Write(rate * 4); bw.Write((short)4); bw.Write((short)32);
        bw.Write("data"u8); bw.Write(bytes);
        var buf = new byte[bytes];
        Buffer.BlockCopy(data, 0, buf, 0, bytes);
        bw.Write(buf);
    }
}
