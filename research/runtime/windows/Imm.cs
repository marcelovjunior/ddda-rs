using System;
using System.IO;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
// IMM32 at window creation (L21): what Windows answers for the calls
// DDDA's 0x01106C80/0x01106B30 make on its main window, and which messages
// reach the window procedure during each call. Run as a GUI exe:
//   Add-Type -Path Imm.cs -OutputAssembly imm.exe -OutputType WindowsApplication
//   Start-Process -Wait imm.exe -ArgumentList <out.txt>
public static class Imm {
  delegate IntPtr WndProc(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct WNDCLASSEXW { public uint cbSize, style; public WndProc lpfnWndProc; public int cbClsExtra, cbWndExtra; public IntPtr hInstance, hIcon, hCursor, hbrBackground; public string lpszMenuName, lpszClassName; public IntPtr hIconSm; }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern ushort RegisterClassExW(ref WNDCLASSEXW wc);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr CreateWindowExW(uint ex, string cls, string name, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
  [DllImport("user32.dll")] static extern IntPtr DefWindowProcW(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [DllImport("user32.dll")] static extern uint GetWindowLongW(IntPtr h, int i);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("kernel32.dll")] static extern IntPtr GetModuleHandleW(IntPtr n);
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("imm32.dll", SetLastError = true)] static extern IntPtr ImmGetDefaultIMEWnd(IntPtr h);
  [DllImport("imm32.dll", SetLastError = true)] static extern IntPtr ImmGetContext(IntPtr h);
  [DllImport("imm32.dll", SetLastError = true)] static extern bool ImmReleaseContext(IntPtr h, IntPtr c);
  [DllImport("imm32.dll", SetLastError = true)] static extern bool ImmSetCompositionWindow(IntPtr c, int[] form);
  [DllImport("imm32.dll", SetLastError = true)] static extern bool ImmSetCandidateWindow(IntPtr c, int[] form);
  [DllImport("imm32.dll", SetLastError = true)] static extern bool ImmSetOpenStatus(IntPtr c, bool open);
  [DllImport("imm32.dll", SetLastError = true)] static extern bool ImmGetOpenStatus(IntPtr c);
  static List<string> log = new List<string>();
  static WndProc proc = (h, msg, w, l) => { log.Add(string.Format("{0:X4}({1:X},{2:X})", msg, (long)w, (long)l)); return DefWindowProcW(h, msg, w, l); };
  static StreamWriter o;
  static string Flush() { var s = string.Join(" ", log); log.Clear(); return s; }
  public static void Main(string[] args) {
    o = new StreamWriter(args[0]);
    var wc = new WNDCLASSEXW { cbSize = (uint)Marshal.SizeOf(typeof(WNDCLASSEXW)), style = 3, lpfnWndProc = proc, hInstance = GetModuleHandleW(IntPtr.Zero), lpszClassName = "ddrt-imm" };
    RegisterClassExW(ref wc);
    IntPtr h = CreateWindowExW(0, "ddrt-imm", "imm", 0x00CA0000, 32, 32, 1286, 749, IntPtr.Zero, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
    o.WriteLine("create: {0}", Flush());
    foreach (var phase in new[] { "hidden", "shown" }) {
      if (phase == "shown") { ShowWindow(h, 1); o.WriteLine("show: {0}", Flush()); }
      SetLastError(0x77); IntPtr ime = ImmGetDefaultIMEWnd(h); int e1 = Marshal.GetLastWin32Error();
      var cls = new StringBuilder(64); GetClassNameW(ime, cls, 64);
      o.WriteLine("[{0}] ImmGetDefaultIMEWnd: {1} err={2} class={3} owner_is_main={4} style={5:X} visible={6} msgs: {7}", phase, ime == IntPtr.Zero ? "NULL" : "hwnd", e1, cls, GetWindow(ime, 4) == h, GetWindowLongW(ime, -16), IsWindowVisible(ime), Flush());
      SetLastError(0x77); IntPtr c = ImmGetContext(h); int e2 = Marshal.GetLastWin32Error();
      o.WriteLine("[{0}] ImmGetContext: {1} err={2} msgs: {3}", phase, c == IntPtr.Zero ? "NULL" : "himc", e2, Flush());
      SetLastError(0x77); bool b1 = ImmSetCompositionWindow(c, new[] { 1, 0, 0, 1, 1, 0, 0 }); int e3 = Marshal.GetLastWin32Error();
      o.WriteLine("[{0}] ImmSetCompositionWindow(CFS_RECT): {1} err={2} msgs: {3}", phase, b1, e3, Flush());
      SetLastError(0x77); bool b2 = ImmSetCandidateWindow(c, new[] { 0, 0x80, 0, 0, 0, 0x437, 1, 0x437 }); int e4 = Marshal.GetLastWin32Error();
      o.WriteLine("[{0}] ImmSetCandidateWindow(CFS_EXCLUDE): {1} err={2} msgs: {3}", phase, b2, e4, Flush());
      SetLastError(0x77); bool b5 = ImmGetOpenStatus(c); int e7 = Marshal.GetLastWin32Error();
      o.WriteLine("[{0}] ImmGetOpenStatus: {1} err={2} msgs: {3}", phase, b5, e7, Flush());
      SetLastError(0x77); bool b3 = ImmSetOpenStatus(c, false); int e5 = Marshal.GetLastWin32Error();
      o.WriteLine("[{0}] ImmSetOpenStatus(FALSE): {1} err={2} msgs: {3}", phase, b3, e5, Flush());
      SetLastError(0x77); bool b4 = ImmReleaseContext(h, c); int e6 = Marshal.GetLastWin32Error();
      o.WriteLine("[{0}] ImmReleaseContext: {1} err={2} msgs: {3}", phase, b4, e6, Flush());
      SetLastError(0x77); bool b6 = ImmSetCompositionWindow(IntPtr.Zero, new[] { 1, 0, 0, 1, 1, 0, 0 }); int e8 = Marshal.GetLastWin32Error();
      o.WriteLine("[{0}] ImmSetCompositionWindow(NULL): {1} err={2}", phase, b6, e8);
      SetLastError(0x77); IntPtr bad = ImmGetContext((IntPtr)0x1234); int e9 = Marshal.GetLastWin32Error();
      o.WriteLine("[{0}] ImmGetContext(bogus hwnd): {1} err={2}", phase, bad == IntPtr.Zero ? "NULL" : "himc", e9);
      SetLastError(0x77); IntPtr bad2 = ImmGetDefaultIMEWnd((IntPtr)0x1234); int e10 = Marshal.GetLastWin32Error();
      o.WriteLine("[{0}] ImmGetDefaultIMEWnd(bogus hwnd): {1} err={2}", phase, bad2 == IntPtr.Zero ? "NULL" : "hwnd", e10);
      SetLastError(0x77); IntPtr c2 = ImmGetContext(h); IntPtr c3 = ImmGetContext(h);
      o.WriteLine("[{0}] two ImmGetContext calls give the same HIMC: {1}; same as the first: {2}", phase, c2 == c3, c2 == c);
      ImmReleaseContext(h, c2); ImmReleaseContext(h, c3);
      o.WriteLine("[{0}] ImmGetDefaultIMEWnd stable: {1}", phase, ImmGetDefaultIMEWnd(h) == ime);
    }
    o.Close();
  }
}
