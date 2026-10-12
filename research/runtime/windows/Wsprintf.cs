using System;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
// USER32 wsprintfA with "%S" (L21): the game's only format (0x00DA3300,
// 0x00DA3650). How the wide string becomes ANSI, the result, the 1024-byte
// limit.
public static class Wsprintf {
  [DllImport("user32.dll", CallingConvention = CallingConvention.Cdecl, CharSet = CharSet.Ansi)]
  static extern int wsprintfA(byte[] buf, string fmt, IntPtr wide);
  static string Hex(byte[] b, int n) { var s = new StringBuilder(); for (int i = 0; i < n; i++) s.AppendFormat("{0:X2} ", b[i]); return s.ToString().Trim(); }
  public static void Run(string path) {
    var w = new StreamWriter(path);
    foreach (var t in new string[] { "abc", "", "é€Ā一x", new string('a', 1100), "a😀b", new string('a', 1021), new string('a', 1022), new string('a', 1023), new string('a', 1024) }) {
      var wide = Marshal.StringToHGlobalUni(t);
      foreach (var fmt in new string[] { "%S", "<%S>%%" }) {
        var buf = new byte[1200]; for (int i = 0; i < buf.Length; i++) buf[i] = 0xEE;
        int r = wsprintfA(buf, fmt, wide);
        int n = Math.Min(r + 2, 40);
        w.WriteLine("len {0} fmt {1}: r={2} first [{3}] tail@r [{4}]", t.Length, fmt, r, Hex(buf, n), r >= 2 ? Hex(new byte[]{buf[r-2], buf[r-1], buf[r], buf[r+1]}, 4) : "");
      }
      Marshal.FreeHGlobal(wide);
    }
    var nul = new byte[16]; for (int i = 0; i < 16; i++) nul[i] = 0xEE;
    w.WriteLine("NULL %S: r={0} [{1}]", wsprintfA(nul, "%S", IntPtr.Zero), Hex(nul, 10));
    w.Close();
  }
}
