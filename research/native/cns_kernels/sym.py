"""Path-merging symbolic executor for the float code of 0x00860AC0's providers.
Straight-line SSE (scalar + a few packed ops) over a node DAG; conditional branches
(float compares) are executed on both arms and merged with 'sel' nodes, so a whole
region becomes one pure function of its inputs."""
import re, os, copy
class Dag:
    def __init__(self): self.nodes=[]; self.index={}
    def intern(self,n):
        if n[0] in ('mul','add'): n=(n[0],)+tuple(sorted(n[1:]))
        if n in self.index: return self.index[n]
        self.nodes.append(n); self.index[n]=len(self.nodes)-1
        return len(self.nodes)-1
    def leaf(self,name): return self.intern(('leaf',name))
    def op(self,o,*a): return self.intern((o,)+a)

def load_asm(path):
    import os, urllib.request
    if not os.path.exists(path):  # fetch from the read-only Ghidra server: p_XXXXXXXX.asm = function at 0xXXXXXXXX
        addr = re.search(r"p_([0-9a-f]+)", path).group(1)
        open(path, "w").write(urllib.request.urlopen("http://127.0.0.1:8089/disassemble_function?address=" + addr).read().decode())
    asm={}; order=[]
    for line in open(path):
        m=re.match(r'([0-9a-f]+): (\S+)\s*(.*?)\s*$',line)
        if m:
            a=int(m.group(1),16); asm[a]=(m.group(2),m.group(3)); order.append(a)
    nxt={order[i]:order[i+1] for i in range(len(order)-1)}
    prv={order[i+1]:order[i] for i in range(len(order)-1)}
    return asm,nxt,prv
def split_ops(s): return [o.strip() for o in re.split(r',(?![^\[]*\])',s)] if s else []
isx=lambda s: re.match(r'^XMM\d$',s.upper())

class State:
    def __init__(self): self.x={}; self.slot={}; self.mem={}; self.flags=None; self.st=None
    def clone(self):
        s=State(); s.x={k:list(v) for k,v in self.x.items()}; s.slot=dict(self.slot); s.mem=dict(self.mem); s.flags=self.flags; s.st=self.st
        return s

class Sym:
    def __init__(self,dag,asmfile,regs,leafname=None):
        self.d=dag; self.asm,self.nxt,self.prv=load_asm(asmfile); self.regs=regs
        self.z=dag.leaf('0.0'); self.st=State(); self.trace=[]; self.adj=0
    # ---- operands
    def memkey(self,op):
        k=re.search(r'\[(.*)\]',op).group(1).replace(' ','')
        mm=re.match(r'^(\w+)(?:\+(0x[0-9a-f]+))?$',k)
        if mm and not k.startswith('0x'): return mm.group(1), int(mm.group(2),16) if mm.group(2) else 0
        return None, int(k,16)
    def initial(self,base,off):
        return self.d.leaf('%s%d'%(self.regs[base],off//4) if off%4==0 else '%s[%#x]'%(self.regs[base],off))
    def load1(self,S,op,k=0):
        if isx(op): return S.x[int(op[3])][0]
        base,off=self.memkey(op); off+=4*k
        if base is None: return self.d.leaf('C%x'%off)
        if base=='ESP':
            off+=self.adj
            if off not in S.slot: raise KeyError('ESP+%#x'%off)
            return S.slot[off]
        if (base,off) in S.mem: return S.mem[(base,off)]
        return self.initial(base,off)
    def loadv(self,S,op):
        if isx(op): return list(S.x[int(op[3])])
        return [self.load1(S,op,k) for k in range(4)]
    def store1(self,S,dst,v,k=0):
        base,off=self.memkey(dst); off+=4*k
        if base=='ESP': S.slot[off+self.adj]=v
        else: S.mem[(base,off)]=v
    # ---- execution
    def run(self,S,pc,stop):
        d=self.d
        while pc!=stop:
            mn,ops=self.asm[pc]; ops=split_ops(ops); nx=self.nxt.get(pc)
            if mn=='MOVSS':
                dst,src=ops
                if isx(dst):
                    S.x[int(dst[3])]=[S.x[int(src[3])][0]]+S.x[int(dst[3])][1:] if isx(src) else [self.load1(S,src),self.z,self.z,self.z]
                else: self.store1(S,dst,S.x[int(src[3])][0])
            elif mn=='MOVAPS':
                dst,src=ops
                if isx(dst): S.x[int(dst[3])]=self.loadv(S,src)
                else:
                    v=S.x[int(src[3])]
                    for k in range(4): self.store1(S,dst,v[k],k)
            elif mn in ('MULSS','ADDSS','SUBSS','DIVSS'):
                dd=int(ops[0][3]); o={'MULSS':'mul','ADDSS':'add','SUBSS':'sub','DIVSS':'div'}[mn]
                S.x[dd]=[d.op(o,S.x[dd][0],self.load1(S,ops[1]))]+S.x[dd][1:]
            elif mn=='SQRTSS':
                dd=int(ops[0][3]); S.x[dd]=[d.op('sqrt',self.load1(S,ops[1]))]+S.x[dd][1:]
            elif mn in ('MULPS','ADDPS'):
                dd=int(ops[0][3]); o={'MULPS':'mul','ADDPS':'add'}[mn]; b=self.loadv(S,ops[1])
                S.x[dd]=[d.op(o,S.x[dd][k],b[k]) for k in range(4)]
            elif mn=='SHUFPS':
                dd=int(ops[0][3]); assert ops[1]==ops[0] and ops[2]=='0x0'
                S.x[dd]=[S.x[dd][0]]*4
            elif mn=='XORPS':
                dd=int(ops[0][3])
                if ops[0]==ops[1]: S.x[dd]=[self.z]*4
                else: S.x[dd]=[d.op('neg',S.x[dd][0])]+S.x[dd][1:]
            elif mn=='JMP': nx=int(ops[0],16)
            elif mn in ('UCOMISS','COMISS'):
                S.flags=('f',S.x[int(ops[0][3])][0],self.load1(S,ops[1]))
            elif mn in ('MOV','LEA'): pass
            elif mn=='PUSH': self.adj-=4
            elif mn in ('LAHF','TEST'): S.flags=S.flags[:3]+(mn,) if S.flags and mn=='LAHF' else S.flags
            elif mn=='FLD': S.st=self.load1(S,ops[0])
            elif mn=='FSTP': self.store1(S,ops[0],d.op('quiet',S.st))
            elif mn=='CALL':
                name={0x12f0906:'sin',0x12f0a55:'cos'}[int(ops[0],16)]
                S.x[0]=[d.op(name,S.x[0][0])]+S.x[0][1:]
            elif mn in ('JBE','JA','JNC','JC','JP','JNP'):
                tgt=int(ops[0],16)
                kind,a,b=S.flags[:3]
                cond={'JBE':d.op('not',d.op('gt',a,b)),'JA':d.op('gt',a,b),'JNC':d.op('ge',a,b),'JC':d.op('not',d.op('ge',a,b)),
                      'JP':d.op('not',d.op('eq',a,b)),'JNP':d.op('eq',a,b)}[mn]
                # structure: if-then (fallthrough arm reaches tgt) or if-else (arm before tgt ends with JMP)
                prev=self.prv[tgt]; pm,po=self.asm[prev]
                if pm=='JMP' and int(po,16)>tgt:
                    join=int(po,16); fall_stop=join; taken_start=tgt
                else:
                    join=tgt; fall_stop=tgt; taken_start=None
                Sf=S.clone(); self.run(Sf,nx,fall_stop)
                St=S.clone()
                if taken_start is not None: self.run(St,taken_start,join)
                S2=self.merge(cond,St,Sf)   # cond true => taken arm
                S.__dict__.update(S2.__dict__)
                nx=join
            else: raise NotImplementedError('%x %s %s'%(pc,mn,ops))
            pc=nx
    def merge(self,cond,St,Sf):
        d=self.d; M=State()
        def sel(a,b): return a if a==b else d.op('sel',cond,a,b)
        for k in set(St.x)|set(Sf.x):
            a=St.x.get(k); b=Sf.x.get(k)
            if a is None or b is None: raise ValueError('xmm%d defined on one arm'%k)
            M.x[k]=[sel(a[i],b[i]) for i in range(4)]
        for k in set(St.slot)|set(Sf.slot):
            a=St.slot.get(k); b=Sf.slot.get(k)
            if a is None or b is None: M.slot[k]=a if b is None else b; continue  # defined on one arm only: dead afterwards
            M.slot[k]=sel(a,b)
        for k in set(St.mem)|set(Sf.mem):
            a=St.mem.get(k); b=Sf.mem.get(k)
            if a is None: a=self.initial(*k)
            if b is None: b=self.initial(*k)
            M.mem[k]=sel(a,b)
        M.flags=None; M.st=None
        return M
