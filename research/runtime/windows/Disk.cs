using System;
using System.IO;
using System.Runtime.InteropServices;
public static class Disk {
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Ansi)] static extern bool GetDiskFreeSpaceA(string root, out uint spc, out uint bps, out uint free, out uint total);
  [DllImport("shell32.dll", CharSet=CharSet.Unicode)] static extern int SHCreateDirectoryExW(IntPtr hwnd, string path, IntPtr sa);
  public static void Run(string dir) {
    var w = new StreamWriter(Path.Combine(dir, "disk.txt"));
    foreach (var r in new string[]{@"E:\", "E:", @"E:\SteamLibrary\", @"E:\SteamLibrary", null, @"Q:\", @"\\?\E:\", @"e:\"}) {
      uint a=0x55,b=0x55,c=0x55,d=0x55; SetLastError(0); bool ok = GetDiskFreeSpaceA(r, out a, out b, out c, out d);
      w.WriteLine("GDFS {0}: {1} err={2} spc={3} bps={4} free={5} total={6}", r ?? "(null)", ok, Marshal.GetLastWin32Error(), a, b, c, d);
    }
    string t = Path.Combine(dir, "shc");
    w.WriteLine("SHC new nested: {0}", SHCreateDirectoryExW(IntPtr.Zero, Path.Combine(t, @"a\b\c"), IntPtr.Zero));
    w.WriteLine("SHC exists: {0}", SHCreateDirectoryExW(IntPtr.Zero, Path.Combine(t, @"a\b\c"), IntPtr.Zero));
    File.WriteAllText(Path.Combine(t, "f"), "x");
    w.WriteLine("SHC is file: {0}", SHCreateDirectoryExW(IntPtr.Zero, Path.Combine(t, "f"), IntPtr.Zero));
    w.WriteLine("SHC under file: {0}", SHCreateDirectoryExW(IntPtr.Zero, Path.Combine(t, @"f\g"), IntPtr.Zero));
    w.WriteLine("SHC relative: {0}", SHCreateDirectoryExW(IntPtr.Zero, @"rel\dir", IntPtr.Zero));
    w.WriteLine("SHC trailing slash: {0}", SHCreateDirectoryExW(IntPtr.Zero, Path.Combine(t, @"d\"), IntPtr.Zero));
    w.WriteLine("SHC dotdot: {0} x={1}", SHCreateDirectoryExW(IntPtr.Zero, Path.Combine(t, @"x\..\y"), IntPtr.Zero), Directory.Exists(Path.Combine(t, "x")));
    w.WriteLine("SHC empty: {0}", SHCreateDirectoryExW(IntPtr.Zero, "", IntPtr.Zero));
    w.WriteLine("SHC drive Q: {0}", SHCreateDirectoryExW(IntPtr.Zero, @"Q:\nope\x", IntPtr.Zero));
    w.Close();
  }
}
