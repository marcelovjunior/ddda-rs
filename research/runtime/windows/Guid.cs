using System;
using System.IO;
using System.Runtime.InteropServices;
// ole32 StringFromGUID2 (L21): the text form and the short-buffer result.
public static class Guid2 {
  [DllImport("ole32.dll")] static extern int StringFromGUID2(byte[] guid, [Out] ushort[] buf, int cch);
  public static void Run(string path) {
    var w = new StreamWriter(path);
    var g = new byte[16]; for (int i = 0; i < 16; i++) g[i] = (byte)(0x10 * i + 0xA + i);
    foreach (int n in new int[]{39, 40, 38, 10, 0}) {
      var b = new ushort[48]; for (int i = 0; i < b.Length; i++) b[i] = 0x23;
      int r = StringFromGUID2(g, b, n);
      w.WriteLine("cch {0}: r={1} [{2}]", n, r, string.Join("", Array.ConvertAll(b, c => c == 0 ? "|" : ((char)c).ToString())));
    }
    w.Close();
  }
}
