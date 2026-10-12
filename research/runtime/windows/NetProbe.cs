// Winsock contract probe (brief L32). Calls the WS2_32 functions DDDA.exe
// imports, in the order its socket class (0x00DA19D0..0x00DA2A90) and its
// resolver (0x00D8B8F0) use them, and prints what Windows answers:
//
//   Part A: before any WSAStartup in the process. DDDA.exe never calls
//           WSAStartup (it is not imported), so this is what the game sees
//           when no other module of its process has initialised Winsock.
//   Part B: after WSAStartup(2.2), only calls that stay on this machine
//           (loopback and unbound sockets; no DNS, no remote address).
//
// It never sends a packet off the machine. Build it as a 32-bit console
// program (the game is 32-bit), see README.md:
//
//   Add-Type -Path NetProbe.cs -OutputAssembly <out>\netprobe.exe -OutputType ConsoleApplication `
//     -CompilerParameters (New-Object System.CodeDom.Compiler.CompilerParameters -Property @{CompilerOptions='/platform:x86'})
//   <out>\netprobe.exe <out>\net.txt
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

public static class NetProbe
{
    const string WS = "ws2_32.dll";
    [DllImport(WS, SetLastError = true)] static extern int WSAStartup(ushort version, byte[] data);
    [DllImport(WS, SetLastError = true)] static extern int WSAGetLastError();
    [DllImport(WS, SetLastError = true)] static extern IntPtr socket(int af, int type, int protocol);
    [DllImport(WS, SetLastError = true)] static extern int closesocket(IntPtr s);
    [DllImport(WS, SetLastError = true)] static extern int ioctlsocket(IntPtr s, int cmd, ref uint arg);
    [DllImport(WS, SetLastError = true)] static extern int setsockopt(IntPtr s, int level, int name, byte[] value, int len);
    [DllImport(WS, SetLastError = true)] static extern int getsockopt(IntPtr s, int level, int name, byte[] value, ref int len);
    [DllImport(WS, SetLastError = true)] static extern int bind(IntPtr s, byte[] addr, int len);
    [DllImport(WS, SetLastError = true)] static extern int listen(IntPtr s, int backlog);
    [DllImport(WS, SetLastError = true)] static extern IntPtr accept(IntPtr s, byte[] addr, ref int len);
    [DllImport(WS, SetLastError = true)] static extern int connect(IntPtr s, byte[] addr, int len);
    [DllImport(WS, SetLastError = true)] static extern int getsockname(IntPtr s, byte[] addr, ref int len);
    [DllImport(WS, SetLastError = true)] static extern int send(IntPtr s, byte[] buf, int len, int flags);
    [DllImport(WS, SetLastError = true)] static extern int recv(IntPtr s, byte[] buf, int len, int flags);
    [DllImport(WS, SetLastError = true)] static extern int sendto(IntPtr s, byte[] buf, int len, int flags, byte[] to, int tolen);
    [DllImport(WS, SetLastError = true)] static extern int recvfrom(IntPtr s, byte[] buf, int len, int flags, byte[] from, ref int fromlen);
    [DllImport(WS, SetLastError = true)] static extern int select(int nfds, byte[] r, byte[] w, byte[] e, byte[] timeout);
    [DllImport(WS, SetLastError = true)] static extern int __WSAFDIsSet(IntPtr s, byte[] set);
    [DllImport(WS, SetLastError = true)] static extern IntPtr gethostbyname([MarshalAs(UnmanagedType.LPStr)] string name);
    [DllImport(WS)] static extern ushort ntohs(ushort v);
    [DllImport("kernel32.dll")] static extern void SetLastError(int e);
    [DllImport("kernel32.dll")] static extern int GetLastError();

    static StringBuilder o = new StringBuilder();
    const int SENTINEL = 0x5a5a5a5a;

    // Every failing call: return value, WSAGetLastError, the thread's
    // last error (set to a sentinel before the call).
    static void Line(string what, long ret)
    {
        int gle = Marshal.GetLastWin32Error();
        int wsa = WSAGetLastError();
        o.AppendFormat("{0}: ret={1} wsa={2} gle={3}\n", what, ret, wsa, gle == SENTINEL ? "unchanged" : gle.ToString());
    }
    static byte[] Addr(uint ip, ushort port)
    {
        var a = new byte[16];
        a[0] = 2; a[2] = (byte)(port >> 8); a[3] = (byte)port;
        a[4] = (byte)(ip >> 24); a[5] = (byte)(ip >> 16); a[6] = (byte)(ip >> 8); a[7] = (byte)ip;
        return a;
    }
    static byte[] FdSet(params IntPtr[] s)
    {
        var f = new byte[4 + 64 * 4];
        BitConverter.GetBytes(s.Length).CopyTo(f, 0);
        for (int i = 0; i < s.Length; i++) BitConverter.GetBytes(s[i].ToInt32()).CopyTo(f, 4 + 4 * i);
        return f;
    }
    static byte[] Int(int v) { return BitConverter.GetBytes(v); }
    static byte[] Tv0() { return new byte[8]; }
    static string Hex(byte[] b, int n) { return BitConverter.ToString(b, 0, n); }

    static void PartA()
    {
        o.Append("== A: no WSAStartup in this process\n");
        var fake = new IntPtr(0x1234);
        SetLastError(SENTINEL); var s = socket(2, 1, 0); Line("socket(AF_INET, SOCK_STREAM, 0)", s.ToInt64());
        SetLastError(SENTINEL); s = socket(2, 2, 0); Line("socket(AF_INET, SOCK_DGRAM, 0)", s.ToInt64());
        // Only while Winsock is uninitialised: then the call fails before any
        // resolution. Otherwise it would query DNS, so it is skipped.
        if (WSAGetLastError() == 10093)
        {
            SetLastError(SENTINEL); var dns = gethostbyname("dune.dragonsdogma.com"); Line("gethostbyname(dune.dragonsdogma.com)", dns.ToInt64());
        }
        else o.Append("Winsock was already initialised: part A is invalid\n");
        IntPtr h;
        SetLastError(SENTINEL); h = gethostbyname("127.0.0.1"); Line("gethostbyname(127.0.0.1)", h.ToInt64());
        uint arg = 1;
        SetLastError(SENTINEL); int r = ioctlsocket(fake, unchecked((int)0x8004667e), ref arg); Line("ioctlsocket(FIONBIO=1)", r);
        arg = 0x77777777;
        SetLastError(SENTINEL); r = ioctlsocket(fake, 0x4004667f, ref arg); Line("ioctlsocket(FIONREAD)", r);
        o.AppendFormat("  FIONREAD argp after: 0x{0:x8}\n", arg);
        SetLastError(SENTINEL); r = setsockopt(fake, 0xffff, 4, Int(1), 4); Line("setsockopt(SOL_SOCKET, SO_REUSEADDR)", r);
        SetLastError(SENTINEL); r = setsockopt(fake, 6, 1, Int(1), 4); Line("setsockopt(IPPROTO_TCP, TCP_NODELAY)", r);
        SetLastError(SENTINEL); r = bind(fake, Addr(0, 0), 16); Line("bind(0.0.0.0:0)", r);
        SetLastError(SENTINEL); r = listen(fake, 3); Line("listen(3)", r);
        var a = new byte[16]; for (int i = 0; i < 16; i++) a[i] = 0xa5; int alen = 16;
        SetLastError(SENTINEL); var acc = accept(fake, a, ref alen); Line("accept", acc.ToInt64());
        o.AppendFormat("  accept addr after: {0} len {1}\n", Hex(a, 16), alen);
        SetLastError(SENTINEL); r = connect(fake, Addr(0xc0000201, 12501), 16); Line("connect(192.0.2.1:12501)", r);
        for (int i = 0; i < 16; i++) a[i] = 0xa5; alen = 16;
        SetLastError(SENTINEL); r = getsockname(fake, a, ref alen); Line("getsockname", r);
        o.AppendFormat("  getsockname addr after: {0} len {1}\n", Hex(a, 16), alen);
        var buf = new byte[8];
        SetLastError(SENTINEL); r = send(fake, buf, 8, 0); Line("send", r);
        SetLastError(SENTINEL); r = recv(fake, buf, 5, 2); Line("recv(MSG_PEEK)", r);
        SetLastError(SENTINEL); r = sendto(fake, buf, 8, 0, Addr(0x7f000001, 9), 16); Line("sendto(127.0.0.1:9)", r);
        int flen = 16;
        SetLastError(SENTINEL); r = recvfrom(fake, buf, 8, 0, a, ref flen); Line("recvfrom", r);
        var rs = FdSet(fake); var es = FdSet(fake);
        SetLastError(SENTINEL); r = select(0, rs, null, es, Tv0()); Line("select(0, {s}, NULL, {s}, 0s)", r);
        o.AppendFormat("  read set after: {0}\n", Hex(rs, 8));
        SetLastError(SENTINEL); r = select(0, null, null, null, Tv0()); Line("select(0, NULL, NULL, NULL, 0s)", r);
        SetLastError(SENTINEL); r = __WSAFDIsSet(fake, FdSet(fake)); Line("__WSAFDIsSet(s in {s})", r);
        SetLastError(SENTINEL); r = __WSAFDIsSet(fake, FdSet()); Line("__WSAFDIsSet(s in {})", r);
        SetLastError(SENTINEL); r = closesocket(fake); Line("closesocket(0x1234)", r);
        SetLastError(SENTINEL); r = closesocket(new IntPtr(-1)); Line("closesocket(INVALID_SOCKET)", r);
        SetLastError(SENTINEL); int n = ntohs(0x3412); Line("ntohs(0x3412)=0x" + n.ToString("x4"), n);
        // The whole EAX ntohs returns, for an argument with upper bits set.
        o.AppendFormat("ntohs32(0xabcd3412) eax=0x{0:x8}\n", ntohs32(0xabcd3412));
        // __WSAFDIsSet: the value it returns when found at index 2, and the
        // fd_count it honours (65 entries, the socket only at index 64).
        var set = FdSet(new IntPtr(1), new IntPtr(2), fake);
        o.AppendFormat("__WSAFDIsSet(found at index 2) = {0}\n", __WSAFDIsSet(fake, set));
        var big = new byte[4 + 65 * 4];
        BitConverter.GetBytes(65).CopyTo(big, 0);
        BitConverter.GetBytes(0x1234).CopyTo(big, 4 + 64 * 4);
        o.AppendFormat("__WSAFDIsSet(fd_count 65, at index 64) = {0}\n", __WSAFDIsSet(fake, big));
        BitConverter.GetBytes(0).CopyTo(big, 0);
        BitConverter.GetBytes(0x1234).CopyTo(big, 4);
        o.AppendFormat("__WSAFDIsSet(fd_count 0, at index 0) = {0}\n", __WSAFDIsSet(fake, big));
    }
    [DllImport(WS, EntryPoint = "ntohs")] static extern uint ntohs32(uint v);

    static void PartB()
    {
        o.Append("== B: after WSAStartup(2.2), on this machine only\n");
        var data = new byte[400];
        SetLastError(SENTINEL); int r = WSAStartup(0x0202, data); Line("WSAStartup(2.2)", r);
        o.AppendFormat("  wVersion=0x{0:x4} wHighVersion=0x{1:x4}\n", BitConverter.ToUInt16(data, 0), BitConverter.ToUInt16(data, 2));
        SetLastError(SENTINEL); var t = socket(2, 1, 0); Line("socket(AF_INET, SOCK_STREAM, 0) valid=" + (t.ToInt64() != -1), t.ToInt64() == -1 ? -1 : 0);
        uint arg = 1;
        SetLastError(SENTINEL); r = ioctlsocket(t, unchecked((int)0x8004667e), ref arg); Line("ioctlsocket(FIONBIO=1)", r);
        SetLastError(SENTINEL); r = setsockopt(t, 0xffff, 4, Int(1), 4); Line("setsockopt(SO_REUSEADDR)", r);
        SetLastError(SENTINEL); r = setsockopt(t, 6, 1, Int(1), 4); Line("setsockopt(TCP_NODELAY)", r);
        SetLastError(SENTINEL); r = setsockopt(t, 0xffff, 0x1001, Int(0x8000), 4); Line("setsockopt(SO_SNDBUF=0x8000)", r);
        SetLastError(SENTINEL); r = setsockopt(t, 0xffff, 0x1002, Int(0x8000), 4); Line("setsockopt(SO_RCVBUF=0x8000)", r);
        arg = 0x77777777;
        SetLastError(SENTINEL); r = ioctlsocket(t, 0x4004667f, ref arg); Line("ioctlsocket(FIONREAD) unconnected", r);
        o.AppendFormat("  FIONREAD argp after: 0x{0:x8}\n", arg);
        var a = new byte[16]; int alen = 16;
        SetLastError(SENTINEL); r = getsockname(t, a, ref alen); Line("getsockname unbound", r);
        var buf = new byte[8];
        SetLastError(SENTINEL); r = send(t, buf, 8, 0); Line("send unconnected", r);
        SetLastError(SENTINEL); r = recv(t, buf, 5, 2); Line("recv(MSG_PEEK) unconnected", r);
        SetLastError(SENTINEL); r = select(0, null, null, null, Tv0()); Line("select(0, NULL, NULL, NULL, 0s)", r);
        SetLastError(SENTINEL); r = bind(t, Addr(0, 0), 16); Line("bind(0.0.0.0:0)", r);
        alen = 16;
        SetLastError(SENTINEL); r = getsockname(t, a, ref alen); Line("getsockname bound", r);
        o.AppendFormat("  family {0} addr {1}.{2}.{3}.{4} port nonzero={5} len {6}\n", a[0], a[4], a[5], a[6], a[7], (a[2] | a[3]) != 0, alen);
        SetLastError(SENTINEL); r = listen(t, 3); Line("listen(3)", r);
        alen = 16;
        SetLastError(SENTINEL); var acc = accept(t, a, ref alen); Line("accept nonblocking, none pending", acc.ToInt64());
        var rs = FdSet(t); var es = FdSet(t);
        SetLastError(SENTINEL); r = select(0, rs, null, es, Tv0()); Line("select(listening in read+except, 0s)", r);
        o.AppendFormat("  read set count after {0}, except count after {1}\n", BitConverter.ToInt32(rs, 0), BitConverter.ToInt32(es, 0));
        ushort lport = (ushort)((a[2] << 8) | a[3]);

        // A non-blocking connect to a closed loopback port (the listening
        // socket's port + 1 is not guaranteed closed; use port 1).
        var c = socket(2, 1, 0); arg = 1; ioctlsocket(c, unchecked((int)0x8004667e), ref arg);
        SetLastError(SENTINEL); r = connect(c, Addr(0x7f000001, 1), 16); Line("connect(127.0.0.1:1) nonblocking", r);
        alen = 16;
        SetLastError(SENTINEL); r = getsockname(c, a, ref alen); Line("getsockname while connecting", r);
        o.AppendFormat("  addr {0}.{1}.{2}.{3}\n", a[4], a[5], a[6], a[7]);
        var ws = FdSet(c); es = FdSet(c);
        var tv = new byte[8]; BitConverter.GetBytes(2).CopyTo(tv, 0);
        SetLastError(SENTINEL); r = select(0, null, ws, es, tv); Line("select(0, NULL, {c}, {c}, 2s)", r);
        o.AppendFormat("  write set count {0} except set count {1}\n", BitConverter.ToInt32(ws, 0), BitConverter.ToInt32(es, 0));
        var err = new byte[4]; int elen = 4;
        getsockopt(c, 0xffff, 0x1007, err, ref elen);
        o.AppendFormat("  SO_ERROR {0}\n", BitConverter.ToInt32(err, 0));
        SetLastError(SENTINEL); r = connect(c, Addr(0x7f000001, 1), 16); Line("connect again after refusal", r);
        closesocket(c);

        var u = socket(2, 2, 0);
        SetLastError(SENTINEL); r = setsockopt(u, 0xffff, 0x20, Int(1), 4); Line("setsockopt(SO_BROADCAST) udp", r);
        arg = 1; ioctlsocket(u, unchecked((int)0x8004667e), ref arg);
        SetLastError(SENTINEL); r = connect(u, Addr(0x7f000001, 9), 16) == 0 ? 0 : -1; Line("connect udp (not used by the game, reference)", r);
        closesocket(u);
        u = socket(2, 2, 0); ioctlsocket(u, unchecked((int)0x8004667e), ref arg);
        bind(u, Addr(0x7f000001, 0), 16);
        alen = 16; getsockname(u, a, ref alen);
        int flen = 16;
        SetLastError(SENTINEL); r = recvfrom(u, buf, 8, 0, a, ref flen); Line("recvfrom udp, nothing queued", r);
        SetLastError(SENTINEL); r = sendto(u, buf, 8, 0, Addr(0x7f000001, (ushort)((a[2] << 8) | a[3])), 16); Line("sendto udp to itself", r);
        System.Threading.Thread.Sleep(50);
        flen = 16;
        SetLastError(SENTINEL); r = recvfrom(u, buf, 8, 0, a, ref flen); Line("recvfrom udp after sendto", r);
        closesocket(u);

        SetLastError(SENTINEL); r = closesocket(t); Line("closesocket", r);
        SetLastError(SENTINEL); r = closesocket(t); Line("closesocket again", r);
        SetLastError(SENTINEL); var h = gethostbyname("127.0.0.1"); Line("gethostbyname(127.0.0.1) non-null=" + (h != IntPtr.Zero), h == IntPtr.Zero ? 0 : 1);
        if (h != IntPtr.Zero)
        {
            short type = Marshal.ReadInt16(h, 8), len = Marshal.ReadInt16(h, 10);
            IntPtr list = Marshal.ReadIntPtr(h, 12); IntPtr first = Marshal.ReadIntPtr(list);
            var ip = new byte[4]; Marshal.Copy(first, ip, 0, 4);
            o.AppendFormat("  h_addrtype {0} h_length {1} addr {2} name {3} second entry null={4}\n", type, len,
                string.Join(".", ip), Marshal.PtrToStringAnsi(Marshal.ReadIntPtr(h, 0)), Marshal.ReadIntPtr(list, 4) == IntPtr.Zero);
        }
    }

    public static void Main(string[] args)
    {
        o.AppendFormat("process is {0}-bit\n", IntPtr.Size * 8);
        PartA();
        PartB();
        File.WriteAllText(args[0], o.ToString());
    }
}
