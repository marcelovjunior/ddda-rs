using System;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
public static class Probe3 {
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GlobalAlloc(uint f, UIntPtr n);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GlobalFree(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr LocalAlloc(uint f, UIntPtr n);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GlobalLock(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GlobalUnlock(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Ansi)] static extern bool SetEnvironmentVariableA(string n, string v);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GetEnvironmentStringsW();
  [DllImport("kernel32.dll", SetLastError=true)] static extern ulong VerSetConditionMask(ulong m, uint t, byte c);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool VerifyVersionInfoA(byte[] info, uint type, ulong mask);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern uint GetPrivateProfileStringW(string app, string key, string def, char[] buf, uint size, string file);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool WritePrivateProfileStringW(string app, string key, string val, string file);
  static StreamWriter w;
  static byte[] Ver(uint major, uint minor, uint build, uint plat, ushort spmaj, ushort spmin, byte product) {
    var b = new byte[156];
    BitConverter.GetBytes(156u).CopyTo(b, 0); BitConverter.GetBytes(major).CopyTo(b, 4); BitConverter.GetBytes(minor).CopyTo(b, 8);
    BitConverter.GetBytes(build).CopyTo(b, 12); BitConverter.GetBytes(plat).CopyTo(b, 16);
    BitConverter.GetBytes(spmaj).CopyTo(b, 148); BitConverter.GetBytes(spmin).CopyTo(b, 150); b[154] = product;
    return b;
  }
  static void V(string label, byte[] info, uint type, ulong mask) {
    SetLastError(0); bool r = VerifyVersionInfoA(info, type, mask); w.WriteLine("VVI {0}: {1} err={2}", label, r, Marshal.GetLastWin32Error());
  }
  static string Show(char[] b, uint n) { var s = new StringBuilder(); for (int i = 0; i < n + 2 && i < b.Length; i++) { char c = b[i]; s.Append(c == 0 ? "\\0" : (c == '' ? "#" : c.ToString())); } return s.ToString(); }
  static void P(string label, string app, string key, string def, uint size, string file) {
    var b = new char[size + 4]; for (int i = 0; i < b.Length; i++) b[i] = '';
    SetLastError(0); uint r = GetPrivateProfileStringW(app, key, def, b, size, file); int e = Marshal.GetLastWin32Error();
    w.WriteLine("GPPS {0}: r={1} err={2} [{3}]", label, r, e, Show(b, r));
  }
  static void WP(string label, string app, string key, string val, string file) {
    SetLastError(0); bool r = WritePrivateProfileStringW(app, key, val, file); int e = Marshal.GetLastWin32Error();
    string content = File.Exists(file) ? BitConverter.ToString(File.ReadAllBytes(file)) : "(none)";
    w.WriteLine("WPPS {0}: r={1} err={2} file={3}", label, r, e, content);
  }
  public static void Run(string dir) {
    w = new StreamWriter(Path.Combine(dir, "probe3.txt"));
    foreach (uint fl in new uint[]{1, 4, 8, 0x80, 0x200, 0x400, 0x800, 0x8000, 0x10000, 0x7F72, 0x7F7A}) {
      SetLastError(0); IntPtr h = GlobalAlloc(fl, (UIntPtr)8); int e = Marshal.GetLastWin32Error(); w.WriteLine("GA flags {0:X}: {1} err={2}", fl, h != IntPtr.Zero, e); if (h != IntPtr.Zero) GlobalFree(h);
      SetLastError(0); IntPtr l = LocalAlloc(fl, (UIntPtr)8); e = Marshal.GetLastWin32Error(); w.WriteLine("LA flags {0:X}: {1} err={2}", fl, l != IntPtr.Zero, e);
    }
    { IntPtr h = GlobalAlloc(2, (UIntPtr)8); GlobalLock(h); SetLastError(0); IntPtr r = GlobalFree(h); w.WriteLine("GF locked moveable: {0:X} err={1}", (long)r, Marshal.GetLastWin32Error()); }
    { IntPtr h = GlobalAlloc(2, (UIntPtr)8); GlobalFree(h); SetLastError(0); IntPtr r = GlobalFree(h); w.WriteLine("GF twice moveable: {0:X} err={1} same={2}", (long)r, Marshal.GetLastWin32Error(), r == h); }
    { IntPtr h = GlobalAlloc(2, (UIntPtr)8); GlobalFree(h); SetLastError(0); IntPtr p = GlobalLock(h); w.WriteLine("GL freed moveable: {0:X} err={1}", (long)p, Marshal.GetLastWin32Error()); }
    // environment ordering
    SetEnvironmentVariableA("ZZDDRT_B", "2"); SetEnvironmentVariableA("ZZDDRT_A", "1"); SetEnvironmentVariableA("zzddrt_c", "3"); SetEnvironmentVariableA("ZZDDRT_A", "9"); SetEnvironmentVariableA("zzddrt_a", "8");
    { IntPtr p = GetEnvironmentStringsW(); var all = new StringBuilder(); int k = 0; while (true) { string s = Marshal.PtrToStringUni(p + 2 * k); if (s.Length == 0) break; if (s.ToUpper().StartsWith("ZZDDRT") || s.StartsWith("=")) all.Append(s + " | "); k += s.Length + 1; } w.WriteLine("ENV order: {0}", all); }
    // version: this process sees its own (manifested) version
    ulong ge3 = VerSetConditionMask(0, 3, 3);
    V("5.1 maj|min ge", Ver(5,1,0,0,0,0,0), 3, ge3);
    V("10.0 maj|min ge", Ver(10,0,0,0,0,0,0), 3, ge3);
    V("10.1 maj|min ge", Ver(10,1,0,0,0,0,0), 3, ge3);
    V("11.0 maj|min ge", Ver(11,0,0,0,0,0,0), 3, ge3);
    V("typemask 0", Ver(5,1,0,0,0,0,0), 0, ge3);
    V("mask 0", Ver(5,1,0,0,0,0,0), 3, 0);
    V("minor only ge, cond in major slot", Ver(0,1,0,0,0,0,0), 1, ge3);
    V("minor only ge", Ver(0,1,0,0,0,0,0), 1, VerSetConditionMask(0, 1, 3));
    V("major eq 10", Ver(10,5,0,0,0,0,0), 2, VerSetConditionMask(0, 2, 1));
    V("maj eq min eq 10.0", Ver(10,0,0,0,0,0,0), 3, VerSetConditionMask(VerSetConditionMask(0, 2, 1), 1, 1));
    V("maj ge min le 6.9", Ver(6,9,0,0,0,0,0), 3, VerSetConditionMask(VerSetConditionMask(0, 2, 3), 1, 5));
    V("maj ge min le 10.9", Ver(10,9,0,0,0,0,0), 3, VerSetConditionMask(VerSetConditionMask(0, 2, 3), 1, 5));
    V("maj le min ge 10.1", Ver(10,1,0,0,0,0,0), 3, VerSetConditionMask(VerSetConditionMask(0, 2, 5), 1, 3));
    V("build ge 26200", Ver(0,0,26200,0,0,0,0), 4, VerSetConditionMask(0, 4, 3));
    V("build ge 26201", Ver(0,0,26201,0,0,0,0), 4, VerSetConditionMask(0, 4, 3));
    V("platform eq 2", Ver(0,0,0,2,0,0,0), 8, VerSetConditionMask(0, 8, 1));
    V("product eq 1", Ver(0,0,0,0,0,0,1), 0x80, VerSetConditionMask(0, 0x80, 1));
    V("sp ge 0.0 + major", Ver(10,0,0,0,0,0,0), 0x23, VerSetConditionMask(0, 0x23, 3));
    V("cond 0 mask with bit63", Ver(10,0,0,0,0,0,0), 2, 0x8000000000000000);
    V("cond 0 mask without bit63", Ver(10,0,0,0,0,0,0), 2, 0);
    V("cond ge without bit63", Ver(5,0,0,0,0,0,0), 2, 0x18);
    V("cond bad 6 major", Ver(5,0,0,0,0,0,0), 2, 0x8000000000000030);
    // profile files
    string f = Path.Combine(dir, "p.ini");
    File.WriteAllBytes(f, Encoding.GetEncoding(1252).GetBytes("[Display]\r\nWidth = 1280 \r\n  Name=\"quoted\"  \r\nSingle='q'\r\nEmpty=\r\nNoEq\r\n;comment=1\r\n[ other ]\r\nk=v\r\nk=second\r\n[Display]\r\nLater=x\r\n"));
    P("width", "Display", "Width", "d", 32, f);
    P("case", "display", "WIDTH", "d", 32, f);
    P("quoted", "Display", "Name", "d", 32, f);
    P("single quoted", "Display", "Single", "d", 32, f);
    P("empty", "Display", "Empty", "d", 32, f);
    P("noeq", "Display", "NoEq", "d", 32, f);
    P("comment", "Display", ";comment", "d", 32, f);
    P("missing def trail", "Display", "Missing", "def  ", 32, f);
    P("missing def lead", "Display", "Missing", "  def", 32, f);
    P("missing null def", "Display", "Missing", null, 32, f);
    P("spaced section", "other", "k", "d", 32, f);
    P("dup key", "other", "k", "d", 32, f);
    P("dup section", "Display", "Later", "d", 32, f);
    P("small", "Display", "Width", "d", 3, f);
    P("size1", "Display", "Width", "d", 1, f);
    P("small quoted", "Display", "Name", "d", 4, f);
    P("sections", null, null, "d", 64, f);
    P("sections small", null, null, "d", 10, f);
    P("keys", "Display", null, "d", 64, f);
    P("keys small", "Display", null, "d", 8, f);
    P("keys missing section", "Nope", null, "dd", 64, f);
    P("missing file", "Display", "Width", "dflt", 32, Path.Combine(dir, "nope.ini"));
    P("missing file sections", null, null, "dflt", 32, Path.Combine(dir, "nope.ini"));
    P("def small", "Display", "Missing", "default", 4, f);
    string g = Path.Combine(dir, "w.ini"); if (File.Exists(g)) File.Delete(g);
    WP("new file", "Sec", "Key", "Val", g);
    WP("second key", "Sec", "Key2", "V2", g);
    WP("update", "sec", "key", "New Value", g);
    WP("new section", "Two", "A", "1", g);
    WP("value with spaces", "Two", "B", "  x  ", g);
    WP("delete key", "Sec", "Key2", null, g);
    WP("delete section", "Two", null, null, g);
    WP("empty value", "Sec", "E", "", g);
    WP("non-ascii", "Sec", "U", "é一", g);
    P("read non-ascii", "Sec", "U", "d", 32, g);
    string u = Path.Combine(dir, "u.ini"); File.WriteAllBytes(u, new byte[]{0xFF, 0xFE});
    WP("unicode file", "S", "K", "é一", u);
    P("read unicode", "S", "K", "d", 32, u);
    string hf = Path.Combine(dir, "h.ini"); File.WriteAllBytes(hf, Encoding.ASCII.GetBytes("[A]\nx=1\n[B]\ny=2"));
    WP("lf file add", "A", "z", "3", hf);
    WP("no trailing newline add", "B", "w", "4", hf);
    WP("flush", null, null, null, hf);
    w.Close();
  }
}
