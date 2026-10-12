// IMM32 answers for a top-level window with the default input context and
// no IME open (brief L43). Run in a 32-bit PowerShell 5.1, as the game is
// a 32-bit process:
//   C:\Windows\SysWOW64\WindowsPowerShell\v1.0\powershell.exe
//   Add-Type -Path Imm.cs; [Imm]::Run('<out>\imm.txt')
// Every call is made with the arguments DDDA.exe passes (0x01106C80,
// 0x01106680, 0x01106720, 0x011067F0, 0x01106970, 0x01106A80,
// 0x01106BB0, 0x01147CC0), on a window made like the game's (style
// 0x00CA0000, before it is shown), and again with a NULL context.
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

public static class Imm
{
    delegate IntPtr WndProc(IntPtr h, uint m, IntPtr w, IntPtr l);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct WNDCLASSEXW
    {
        public uint cbSize, style; public WndProc lpfnWndProc; public int cbClsExtra, cbWndExtra;
        public IntPtr hInstance, hIcon, hCursor, hbrBackground; public string lpszMenuName, lpszClassName; public IntPtr hIconSm;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam, lParam; public uint time; public int x, y; }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern ushort RegisterClassExW(ref WNDCLASSEXW c);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateWindowExW(uint ex, string cls, string name, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] static extern IntPtr DefWindowProcW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool PeekMessageW(out MSG m, IntPtr h, uint lo, uint hi, uint remove);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessageW(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr GetKeyboardLayout(uint thread);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("kernel32.dll")] static extern IntPtr GetModuleHandleW(IntPtr n);
    [DllImport("kernel32.dll")] static extern uint GetLastError();
    [DllImport("kernel32.dll")] static extern void SetLastError(uint e);

    [DllImport("imm32.dll")] static extern IntPtr ImmGetDefaultIMEWnd(IntPtr h);
    [DllImport("imm32.dll")] static extern IntPtr ImmGetContext(IntPtr h);
    [DllImport("imm32.dll")] static extern bool ImmReleaseContext(IntPtr h, IntPtr c);
    [DllImport("imm32.dll")] static extern bool ImmSetCompositionWindow(IntPtr c, int[] form);
    [DllImport("imm32.dll")] static extern bool ImmSetCandidateWindow(IntPtr c, int[] form);
    [DllImport("imm32.dll")] static extern bool ImmGetCompositionWindow(IntPtr c, int[] form);
    [DllImport("imm32.dll")] static extern bool ImmGetCandidateWindow(IntPtr c, uint index, int[] form);
    [DllImport("imm32.dll")] static extern bool ImmSetOpenStatus(IntPtr c, bool open);
    [DllImport("imm32.dll")] static extern bool ImmGetOpenStatus(IntPtr c);
    [DllImport("imm32.dll")] static extern int ImmGetCompositionStringA(IntPtr c, uint index, byte[] buf, uint len);
    [DllImport("imm32.dll")] static extern int ImmGetCompositionStringW(IntPtr c, uint index, byte[] buf, uint len);
    [DllImport("imm32.dll")] static extern uint ImmGetCandidateListCountA(IntPtr c, out uint count);
    [DllImport("imm32.dll")] static extern uint ImmGetCandidateListW(IntPtr c, uint index, byte[] buf, uint len);
    [DllImport("imm32.dll")] static extern uint ImmGetVirtualKey(IntPtr h);
    [DllImport("imm32.dll")] static extern bool ImmIsIME(IntPtr hkl);

    static StringBuilder o = new StringBuilder();
    static void W(string s) { o.AppendLine(s); }
    static string Err() { return " err=" + GetLastError(); }
    static string Fill(byte[] b) { int n = 0; foreach (byte x in b) if (x != 0xCC) n++; return n == 0 ? "untouched" : (n + " bytes written: " + BitConverter.ToString(b, 0, Math.Min(32, b.Length))); }
    static byte[] Buf(int n) { byte[] b = new byte[n]; for (int k = 0; k < n; k++) b[k] = 0xCC; return b; }
    static string Ints(int[] a) { return string.Join(",", Array.ConvertAll(a, x => "0x" + x.ToString("X"))); }

    static WndProc proc = (h, m, w, l) => DefWindowProcW(h, m, w, l);

    static void Calls(string tag, IntPtr hwnd, IntPtr c)
    {
        // 0x01106C80: composition window CFS_RECT? style 1 at (0,0), rect (1,1,0,0)
        int[] comp = { 1, 0, 0, 1, 1, 0, 0 };
        SetLastError(0); W(tag + " ImmSetCompositionWindow(style 1) = " + ImmSetCompositionWindow(c, comp) + Err());
        int[] gotc = new int[7]; for (int k = 0; k < 7; k++) gotc[k] = unchecked((int)0xCCCCCCCC);
        SetLastError(0); W(tag + " ImmGetCompositionWindow = " + ImmGetCompositionWindow(c, gotc) + " " + Ints(gotc) + Err());
        // candidate form: index 0, CFS_EXCLUDE 0x80, pt (0,0), rect (0,0x437,1,0x437)
        int[] cand = { 0, 0x80, 0, 0, 0, 0x437, 1, 0x437 };
        SetLastError(0); W(tag + " ImmSetCandidateWindow(index 0, style 0x80) = " + ImmSetCandidateWindow(c, cand) + Err());
        int[] gotk = new int[8]; for (int k = 0; k < 8; k++) gotk[k] = unchecked((int)0xCCCCCCCC);
        SetLastError(0); W(tag + " ImmGetCandidateWindow(0) = " + ImmGetCandidateWindow(c, 0, gotk) + " " + Ints(gotk) + Err());
        SetLastError(0); W(tag + " ImmGetOpenStatus = " + ImmGetOpenStatus(c) + Err());
        SetLastError(0); W(tag + " ImmSetOpenStatus(FALSE) = " + ImmSetOpenStatus(c, false) + Err());
        SetLastError(0); W(tag + " ImmGetOpenStatus = " + ImmGetOpenStatus(c) + Err());
        foreach (uint idx in new uint[] { 0x8, 0x10, 0x800 })
        {
            byte[] b = Buf(0x40);
            SetLastError(0); int r = ImmGetCompositionStringW(c, idx, b, 0x40);
            W(tag + " ImmGetCompositionStringW(0x" + idx.ToString("X") + ", buf, 0x40) = " + r + " buf " + Fill(b) + Err());
            SetLastError(0); r = ImmGetCompositionStringA(c, idx, null, 0);
            W(tag + " ImmGetCompositionStringA(0x" + idx.ToString("X") + ", NULL, 0) = " + r + Err());
            b = Buf(1);
            SetLastError(0); r = ImmGetCompositionStringA(c, idx, b, 1);
            W(tag + " ImmGetCompositionStringA(0x" + idx.ToString("X") + ", buf, 1) = " + r + " buf " + Fill(b) + Err());
        }
        uint count = 0xCCCCCCCC;
        SetLastError(0); uint rc = ImmGetCandidateListCountA(c, out count);
        W(tag + " ImmGetCandidateListCountA = " + rc + " count=0x" + count.ToString("X") + Err());
        byte[] list = Buf(0x1000);
        SetLastError(0); rc = ImmGetCandidateListW(c, 0, list, 0x1000);
        W(tag + " ImmGetCandidateListW(0, buf, 0x1000) = " + rc + " buf " + Fill(list) + Err());
        SetLastError(0); rc = ImmGetCandidateListW(c, 0, null, 0);
        W(tag + " ImmGetCandidateListW(0, NULL, 0) = " + rc + Err());
    }

    public static void Run(string path)
    {
        W("process bits: " + (IntPtr.Size * 8));
        IntPtr hkl = GetKeyboardLayout(0);
        W("keyboard layout: 0x" + hkl.ToInt64().ToString("X") + " ImmIsIME=" + ImmIsIME(hkl));
        WNDCLASSEXW wc = new WNDCLASSEXW();
        wc.cbSize = (uint)Marshal.SizeOf(typeof(WNDCLASSEXW)); wc.style = 3; wc.lpfnWndProc = proc;
        wc.hInstance = GetModuleHandleW(IntPtr.Zero); wc.lpszClassName = "ImmProbe";
        RegisterClassExW(ref wc);
        SetLastError(0);
        W("default IME window before any window: 0x" + ImmGetDefaultIMEWnd(IntPtr.Zero).ToInt64().ToString("X") + Err());
        IntPtr hwnd = CreateWindowExW(0, "ImmProbe", "probe", 0x00CA0000, 32, 32, 1286, 749, IntPtr.Zero, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
        W("window 0x" + hwnd.ToInt64().ToString("X"));
        SetLastError(0);
        IntPtr ime = ImmGetDefaultIMEWnd(hwnd);
        StringBuilder cls = new StringBuilder(64);
        GetClassNameW(ime, cls, 64);
        W("ImmGetDefaultIMEWnd(window) = " + (ime == IntPtr.Zero ? "NULL" : "non-NULL") + Err() + " class '" + cls + "' visible=" + IsWindowVisible(ime)
          + " owner=" + (GetWindow(ime, 4) == hwnd ? "the window" : "0x" + GetWindow(ime, 4).ToInt64().ToString("X"))
          + " same thread=" + (GetWindowThreadProcessId(ime, IntPtr.Zero) == GetCurrentThreadId()));
        SetLastError(0); W("ImmGetDefaultIMEWnd(NULL) = " + (ImmGetDefaultIMEWnd(IntPtr.Zero) == ime ? "the same window" : "0x" + ImmGetDefaultIMEWnd(IntPtr.Zero).ToInt64().ToString("X")) + Err());
        SetLastError(0);
        IntPtr c = ImmGetContext(hwnd);
        W("ImmGetContext(window) = " + (c == IntPtr.Zero ? "NULL" : "non-NULL") + Err());
        SetLastError(0); IntPtr c2 = ImmGetContext(hwnd);
        W("ImmGetContext(window) again = " + (c2 == c ? "the same context" : "another") + Err());
        SetLastError(0); W("ImmGetContext(NULL) = 0x" + ImmGetContext(IntPtr.Zero).ToInt64().ToString("X") + Err());
        Calls("ctx", hwnd, c);
        SetLastError(0); W("ImmReleaseContext(window, ctx) = " + ImmReleaseContext(hwnd, c) + Err());
        SetLastError(0); W("ImmReleaseContext(window, ctx) again = " + ImmReleaseContext(hwnd, c2) + Err());
        SetLastError(0); W("ImmReleaseContext(window, NULL) = " + ImmReleaseContext(hwnd, IntPtr.Zero) + Err());
        Calls("null", hwnd, IntPtr.Zero);
        // ImmGetVirtualKey: no message yet; after a key message dispatched with
        // and without TranslateMessage (the game calls it once per frame
        // after its message loop, 0x00DF0D9E -> 0x01106720)
        SetLastError(0); W("ImmGetVirtualKey(window), no message = 0x" + ImmGetVirtualKey(hwnd).ToString("X") + Err());
        SetLastError(0); W("ImmGetVirtualKey(NULL) = 0x" + ImmGetVirtualKey(IntPtr.Zero).ToString("X") + Err());
        foreach (bool translate in new bool[] { false, true })
        {
            PostMessageW(hwnd, 0x100, (IntPtr)0x25, (IntPtr)0x014B0001);
            MSG msg;
            while (PeekMessageW(out msg, IntPtr.Zero, 0, 0, 1))
            {
                if (translate) TranslateMessage(ref msg);
                DispatchMessageW(ref msg);
            }
            SetLastError(0); W("ImmGetVirtualKey(window) after WM_KEYDOWN(VK_LEFT)" + (translate ? " + TranslateMessage" : "") + " = 0x" + ImmGetVirtualKey(hwnd).ToString("X") + Err());
        }
        DestroyWindow(hwnd);
        File.WriteAllText(path, o.ToString());
    }
}
