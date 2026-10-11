using System;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
public static class Probe4 {
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("kernel32.dll", SetLastError=true)] static extern ulong VerSetConditionMask(ulong m, uint t, byte c);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool VerifyVersionInfoA(byte[] info, uint type, ulong mask);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern uint GetPrivateProfileStringW(string app, string key, string def, char[] buf, uint size, string file);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool WritePrivateProfileStringW(string app, string key, string val, string file);
  static StreamWriter w;
  static byte[] Ver(uint major, uint minor) {
    var b = new byte[156]; BitConverter.GetBytes(156u).CopyTo(b, 0); BitConverter.GetBytes(major).CopyTo(b, 4); BitConverter.GetBytes(minor).CopyTo(b, 8); return b;
  }
  static void V(string label, byte[] info, uint type, ulong mask) {
    SetLastError(0); bool r = VerifyVersionInfoA(info, type, mask); w.WriteLine("VVI {0}: {1} err={2}", label, r, Marshal.GetLastWin32Error());
  }
  static string Show(char[] b, uint n) { var s = new StringBuilder(); for (int i = 0; i < n + 2 && i < b.Length; i++) { char c = b[i]; s.Append(c == 0 ? "\\0" : (c == '\uEEEE' ? "#" : c.ToString())); } return s.ToString(); }
  static void P(string label, string app, string key, string def, uint size, string file) {
    var b = new char[size + 4]; for (int i = 0; i < b.Length; i++) b[i] = '\uEEEE';
    SetLastError(0); uint r = GetPrivateProfileStringW(app, key, def, b, size, file); int e = Marshal.GetLastWin32Error();
    w.WriteLine("GPPS {0}: r={1} err={2} [{3}]", label, r, e, Show(b, r));
  }
  static void WP(string label, string app, string key, string val, string file) {
    SetLastError(0); bool r = WritePrivateProfileStringW(app, key, val, file); int e = Marshal.GetLastWin32Error();
    string content = File.Exists(file) ? Encoding.GetEncoding(1252).GetString(File.ReadAllBytes(file)).Replace("\r", "\\r").Replace("\n", "\\n") : "(none)";
    w.WriteLine("WPPS {0}: r={1} err={2} file={3}", label, r, e, content);
  }
  public static void Run(string dir) {
    w = new StreamWriter(Path.Combine(dir, "probe4.txt"));
    ulong b63 = 0x8000000000000000;
    V("cond0 bit63 5 vs 10", Ver(5,0), 2, b63);
    V("cond0 bit63 11 vs 10", Ver(11,0), 2, b63);
    V("cond6 bit63 5", Ver(5,0), 2, b63 | (6ul << 3));
    V("cond6 bit63 10", Ver(10,0), 2, b63 | (6ul << 3));
    V("cond7 bit63 10", Ver(10,0), 2, b63 | (7ul << 3));
    V("gt 9", Ver(9,0), 2, VerSetConditionMask(0, 2, 2));
    V("gt 10", Ver(10,0), 2, VerSetConditionMask(0, 2, 2));
    V("lt 11", Ver(11,0), 2, VerSetConditionMask(0, 2, 4));
    V("maj gt min eq 10.0", Ver(10,0), 3, VerSetConditionMask(VerSetConditionMask(0, 2, 2), 1, 1));
    V("maj eq min gt 10.0", Ver(10,0), 3, VerSetConditionMask(VerSetConditionMask(0, 2, 1), 1, 2));
    V("maj eq min lt 10.1", Ver(10,1), 3, VerSetConditionMask(VerSetConditionMask(0, 2, 1), 1, 4));
    V("maj ge min eq 10.0", Ver(10,0), 3, VerSetConditionMask(VerSetConditionMask(0, 2, 3), 1, 1));
    V("maj ge min gt 10.0", Ver(10,0), 3, VerSetConditionMask(VerSetConditionMask(0, 2, 3), 1, 2));
    V("major only ge 5 (game, typemask 2)", Ver(5,1), 2, VerSetConditionMask(0, 3, 3));
    string d = dir;
    string e0 = Path.Combine(d, "empty.ini"); File.WriteAllBytes(e0, new byte[0]);
    WP("empty file", "S", "K", "V", e0);
    string n1 = Path.Combine(d, "nonl.ini"); File.WriteAllBytes(n1, Encoding.ASCII.GetBytes("[A]\r\nx=1"));
    WP("no newline new section", "B", "y", "2", n1);
    string b1 = Path.Combine(d, "blank.ini"); File.WriteAllBytes(b1, Encoding.ASCII.GetBytes("[A]\r\nx=1\r\n\r\n;c\r\n[B]\r\ny=2\r\n"));
    WP("add after blank", "A", "z", "3", b1);
    string s1 = Path.Combine(d, "spaces.ini"); File.WriteAllBytes(s1, Encoding.ASCII.GetBytes("[A]\r\n  x = 1  \r\n"));
    WP("update spaced key", "A", "X", "new", s1);
    WP("key with spaces", "A", " k ", " v ", s1);
    P("read key with spaces", "A", "k", "d", 32, s1);
    WP("delete missing key", "A", "nope", null, s1);
    WP("delete missing section", "Nope", null, null, s1);
    WP("app null key set", null, "k", "v", s1);
    WP("dup section write", "A", "q", "1", s1);
    string m1 = Path.Combine(d, "nodir\\x.ini");
    WP("missing dir", "A", "k", "v", m1);
    string q = Path.Combine(d, "quotes.ini"); File.WriteAllBytes(q, Encoding.ASCII.GetBytes("[A]\r\na=\"x\r\nb=x\"\r\nc=\"x'\r\nd=\"\"\r\ne=\"\r\nf= \"x y\" \r\ng=  \r\n[B] junk\r\nh=1\r\n[C\r\ni=1\r\n k=2\r\n"));
    foreach (var k in new string[]{"a","b","c","d","e","f","g"}) P("quote " + k, "A", k, "D", 32, q);
    P("section junk", "B", "h", "D", 32, q);
    P("section unclosed", "C", "i", "D", 32, q);
    P("indented key", "C", "k", "D", 32, q);
    P("sections q", null, null, "D", 64, q);
    P("keys exact fit", "A", null, "D", 15, q);
    P("value exact fit", "B", "h", "D", 2, q);
    P("sections nsize 2", null, null, "D", 2, q);
    P("sections nsize 1", null, null, "D", 1, q);
    P("key empty string", "A", "", "D", 32, q);
    P("section empty string", "", "a", "D", 32, q);
    string c1 = Path.Combine(d, "crlf.ini"); File.WriteAllBytes(c1, Encoding.ASCII.GetBytes("[A]\rx=1\r[B]\ny=2\n"));
    P("cr only", "A", "x", "D", 32, c1);
    P("cr only B", "B", "y", "D", 32, c1);
    string t1 = Path.Combine(d, "tab.ini"); File.WriteAllBytes(t1, Encoding.ASCII.GetBytes("[A]\r\n\tx\t=\t1\t\r\n"));
    P("tabs", "A", "x", "D", 32, t1);
    string u8 = Path.Combine(d, "utf8.ini"); File.WriteAllBytes(u8, new byte[]{0xEF,0xBB,0xBF,(byte)'[',(byte)'A',(byte)']',13,10,(byte)'x',(byte)'=',0xC3,0xA9,13,10});
    P("utf8 bom", "A", "x", "D", 32, u8);
    w.Close();
  }
}
