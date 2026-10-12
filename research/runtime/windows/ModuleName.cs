using System;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
// GetModuleFileNameA/W truncation and errors (L21): what the buffer, the
// result and the last error are when nSize is too small.
public static class ModuleName {
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint GetModuleFileNameA(IntPtr h, byte[] buf, uint n);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint GetModuleFileNameW(IntPtr h, byte[] buf, uint n);
  public static void Run(string path) {
    var w = new StreamWriter(path);
    foreach (uint n in new uint[]{0, 1, 5, 260}) {
      foreach (bool wide in new bool[]{false, true}) {
        var b = new byte[600]; for (int i = 0; i < b.Length; i++) b[i] = 0xEE;
        SetLastError(0x77);
        uint r = wide ? GetModuleFileNameW(IntPtr.Zero, b, n) : GetModuleFileNameA(IntPtr.Zero, b, n);
        int e = Marshal.GetLastWin32Error();
        w.WriteLine("{0} n={1}: r={2} err={3} [{4}]", wide ? "W" : "A", n, r, e, BitConverter.ToString(b, 0, (int)Math.Min(24, b.Length)));
      }
    }
    SetLastError(0x77);
    var bb = new byte[600];
    uint rr = GetModuleFileNameA((IntPtr)0x12340000, bb, 260);
    w.WriteLine("A bogus module: r={0} err={1}", rr, Marshal.GetLastWin32Error());
    w.Close();
  }
}
