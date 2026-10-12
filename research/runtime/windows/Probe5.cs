using System;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
public static class Probe5 {
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern uint GetPrivateProfileStringW(string app, string key, string def, char[] buf, uint size, string file);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool WritePrivateProfileStringW(string app, string key, string val, string file);
  static StreamWriter w;
  static void P(string label, string app, string key, string def, uint size, string file) {
    var b = new char[size + 4]; for (int i = 0; i < b.Length; i++) b[i] = '\uEEEE';
    SetLastError(0x77); uint r = GetPrivateProfileStringW(app, key, def, b, size, file); int e = Marshal.GetLastWin32Error();
    var s = new StringBuilder(); for (int i = 0; i < r + 2 && i < b.Length; i++) { char c = b[i]; s.Append(c == 0 ? "\\0" : (c == '\uEEEE' ? "#" : c.ToString())); }
    w.WriteLine("GPPS {0}: r={1} err={2} [{3}]", label, r, e, s);
  }
  static void WP(string label, string app, string key, string val, string file) {
    SetLastError(0x77); bool r = WritePrivateProfileStringW(app, key, val, file); int e = Marshal.GetLastWin32Error();
    string content = File.Exists(file) ? Encoding.GetEncoding(1252).GetString(File.ReadAllBytes(file)).Replace("\r", "\\r").Replace("\n", "\\n") : "(none)";
    w.WriteLine("WPPS {0}: r={1} err={2} file={3}", label, r, e, content);
  }
  public static void Run(string dir) {
    w = new StreamWriter(Path.Combine(dir, "probe5.txt"));
    string f = Path.Combine(dir, "p5.ini");
    File.WriteAllBytes(f, Encoding.ASCII.GetBytes("[A]\r\nx=1\r\nx=2\r\n[ B ]\r\n"));
    P("found: last error", "A", "x", "d", 32, f);
    P("found empty section list", "B", null, "d", 32, f);
    P("dup keys list", "A", null, "d", 32, f);
    P("nsize 0 value", "A", "x", "d", 0, f);
    P("nsize 0 list", null, null, "d", 0, f);
    P("nsize 0 missing", "A", "nope", "d", 0, f);
    P("no sections list", null, null, "d", 32, Path.Combine(dir, "empty5.ini"));
    File.WriteAllBytes(Path.Combine(dir, "empty5.ini"), new byte[0]);
    P("empty file list", null, null, "d", 32, Path.Combine(dir, "empty5.ini"));
    P("empty file value", "A", "x", "d", 32, Path.Combine(dir, "empty5.ini"));
    string g = Path.Combine(dir, "missing5.ini"); if (File.Exists(g)) File.Delete(g);
    WP("delete key missing file", "A", "x", null, g);
    WP("delete section missing file", "A", null, null, g);
    WP("section name spaces", " S ", "k", "v", g);
    WP("write to spaced section", "B", "y", "1", f);
    WP("delete dup key", "A", "x", null, f);
    WP("update dup", "A", "x", "9", f);
    w.Close();
  }
}
