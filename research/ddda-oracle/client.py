"""Client for the ddda-oracle probe (in-process DLL in DDDA.exe).

Library:
    from client import Oracle
    o = Oracle()                 # connects to 127.0.0.1:47800
    o.info(); o.u32(0x018d0f50); o.capture('frame.png')

CLI:
    python client.py inject            # load the DLL (no-op if already loaded)
    python client.py info
    python client.py capture [path]
    python client.py read ADDR LEN
    python client.py find CLASS        # live objects by MtDTI class (exact vtable)
    python client.py call ADDR [ARG..] [--this X] [--conv cdecl|stdcall|thiscall|fastcall] [--float]
    python client.py bg on|off
    python client.py unload

Numbers may be decimal or 0x-hex. Float arguments: f:1.5.
Everything goes through the DLL; nothing here touches the window or input.
"""
import json
import os
import socket
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
PORT = 47800
RELEASE = os.path.join(HERE, 'target', 'i686-pc-windows-msvc', 'release')
REGISTRY = os.environ.get('DDDA_DTI_REGISTRY', r'E:\ddda-re\notes\dti-registry.json')


class OracleError(Exception):
    pass


class Oracle:
    def __init__(self, host='127.0.0.1', port=PORT, timeout=60.0):
        self.sock = socket.create_connection((host, port), timeout=5)
        self.sock.settimeout(timeout)
        self.buf = b''
        self.next_id = 1
        self._reg = None

    def close(self):
        self.sock.close()

    def __enter__(self):
        return self

    def __exit__(self, *a):
        self.close()

    # ------------------------------------------------------------ protocol
    def cmd(self, cmd, **kw):
        req = dict(kw, cmd=cmd, id=self.next_id)
        self.next_id += 1
        self.sock.sendall((json.dumps(req) + '\n').encode())
        while b'\n' not in self.buf:
            chunk = self.sock.recv(1 << 20)
            if not chunk:
                raise OracleError('connection closed')
            self.buf += chunk
        line, self.buf = self.buf.split(b'\n', 1)
        rep = json.loads(line)
        if not rep.get('ok', False):
            raise OracleError(rep.get('error', rep))
        return rep

    # ------------------------------------------------------------ basics
    def info(self):
        return self.cmd('info')

    def read(self, addr, n):
        return bytes.fromhex(self.cmd('read', addr=addr, len=n)['hex'])

    def write(self, addr, data, main=False):
        return self.cmd('write', addr=addr, hex=bytes(data).hex(), main=main)

    def u8(self, a):
        return self.read(a, 1)[0]

    def u32(self, a):
        return struct.unpack('<I', self.read(a, 4))[0]

    def u32s(self, a, n):
        return struct.unpack(f'<{n}I', self.read(a, 4 * n))

    def f32(self, a):
        return struct.unpack('<f', self.read(a, 4))[0]

    def f32s(self, a, n):
        return struct.unpack(f'<{n}f', self.read(a, 4 * n))

    def cstr(self, a, n=256):
        b = self.read(a, n)
        return b.split(b'\0', 1)[0].decode('latin-1')

    def wu32(self, a, v, main=False):
        return self.write(a, struct.pack('<I', v & 0xffffffff), main)

    def wf32(self, a, *v, main=False):
        return self.write(a, struct.pack(f'<{len(v)}f', *v), main)

    def call(self, addr, *args, this=None, conv=None, float=False, edx=None, timeout_ms=10000):
        """Calls a game function on the main thread. Args: ints, or floats
        (sent as f32 bit patterns). Returns the reply (eax, edx[, st0])."""
        a = [{'f': x} if isinstance(x, float) else x for x in args]
        kw = dict(addr=addr, args=a, float=float, timeout_ms=timeout_ms)
        if this is not None:
            kw['this'] = this
        if conv:
            kw['conv'] = conv
        return self.cmd('call', **kw)

    def wait(self, frames=1, timeout_ms=10000):
        return self.cmd('wait', frames=frames, timeout_ms=timeout_ms)

    def bg(self, on=True):
        return self.cmd('bg', on=on)

    def capture(self, path=None, timeout_ms=5000):
        kw = {'timeout_ms': timeout_ms}
        if path:
            kw['path'] = os.path.abspath(path)
        return self.cmd('capture', **kw)

    def scan(self, values, limit=4096, image=False):
        r = self.cmd('scan', values=list(values), limit=limit, image=image)
        return {int(k, 16): v for k, v in r['hits'].items()}

    def trace(self, fields, frames, script=None, timeout_ms=None):
        """Samples memory after each of the next `frames` game frames.
        fields: [(addr, len), ..]; script: [(frame, [vk, ..]), ..] sets the
        held keys from that frame index on (released at the end).
        Returns one bytes row per frame (fields concatenated)."""
        kw = dict(fields=[[a, n] for a, n in fields], frames=frames,
                  script=[[f, list(k)] for f, k in (script or [])])
        if timeout_ms:
            kw['timeout_ms'] = timeout_ms
        r = self.cmd('trace', **kw)
        return [bytes.fromhex(h) for h in r['rows']]

    def unload(self):
        return self.cmd('unload')

    # ------------------------------------------------------------ MtDTI
    def registry(self):
        if self._reg is None:
            with open(REGISTRY) as f:
                self._reg = {c['name']: c for c in json.load(f)['classes']}
        return self._reg

    def vtable_of(self, cls):
        c = self.registry()[cls]
        if not c.get('vtable'):
            raise OracleError(f'{cls} has no unique vtable in the registry')
        return int(c['vtable'], 16)

    def find(self, cls, limit=4096):
        """Live objects of exactly this class (their first word is the
        class vtable). Heap words equal to the vtable that are not objects
        are rare but possible; check a field before trusting a hit."""
        vt = self.vtable_of(cls)
        return self.scan([vt], limit=limit)[vt]


def is_loaded(port=PORT):
    try:
        socket.create_connection(('127.0.0.1', port), timeout=0.5).close()
        return True
    except OSError:
        return False


def inject(wait_s=10.0):
    """Loads the DLL into DDDA.exe unless it already answers on the port."""
    if is_loaded():
        return {'already': True}
    exe = os.path.join(RELEASE, 'ddda-inject.exe')
    dll = os.path.join(RELEASE, 'ddda_oracle.dll')
    out = subprocess.run([exe, '--dll', dll], capture_output=True, text=True)
    if out.returncode != 0:
        raise OracleError(out.stderr.strip() or out.stdout.strip())
    info = json.loads(out.stdout.strip().splitlines()[-1])
    t0 = time.time()
    while time.time() - t0 < wait_s:
        if is_loaded():
            return info
        time.sleep(0.1)
    raise OracleError(f'DLL loaded ({info}) but the server did not answer; see %TEMP%\\ddda-oracle\\oracle.log')


def _num(s):
    if s.startswith('f:'):
        return float(s[2:])
    return int(s, 0)


def main(argv):
    if not argv:
        print(__doc__)
        return 1
    c, rest = argv[0], argv[1:]
    if c == 'inject':
        print(json.dumps(inject()))
        return 0
    o = Oracle()
    if c == 'info':
        r = o.info()
        for k in ('sapp', 'hwnd', 'device', 'module'):
            r[k] = hex(r[k])
        print(json.dumps(r, indent=1))
    elif c == 'capture':
        print(json.dumps(o.capture(rest[0] if rest else None)))
    elif c == 'read':
        b = o.read(_num(rest[0]), _num(rest[1]))
        for i in range(0, len(b), 16):
            print(f'{_num(rest[0]) + i:08x}  {b[i:i + 16].hex(" ")}')
    elif c == 'find':
        for a in o.find(rest[0]):
            print(hex(a))
    elif c == 'call':
        kw, args, i = {}, [], 0
        while i < len(rest):
            if rest[i] == '--this':
                kw['this'] = _num(rest[i + 1]); i += 2
            elif rest[i] == '--conv':
                kw['conv'] = rest[i + 1]; i += 2
            elif rest[i] == '--float':
                kw['float'] = True; i += 1
            else:
                args.append(_num(rest[i])); i += 1
        print(json.dumps(o.call(args[0], *args[1:], **kw)))
    elif c == 'bg':
        print(json.dumps(o.bg(rest[0] == 'on')))
    elif c == 'unload':
        print(json.dumps(o.unload()))
    else:
        print(f'unknown command {c}')
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
