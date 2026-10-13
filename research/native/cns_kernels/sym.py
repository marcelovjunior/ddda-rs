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
EXIT = -1
def split_ops(s): return [o.strip() for o in re.split(r',(?![^\[]*\])',s)] if s else []
isx=lambda s: re.match(r'^XMM\d$',s.upper())

class State:
    def __init__(self): self.x={}; self.slot={}; self.islot={}; self.mem={}; self.flags=None; self.st=None
    def clone(self):
        s=State(); s.x={k:list(v) for k,v in self.x.items()}; s.slot=dict(self.slot); s.islot=dict(self.islot); s.mem=dict(self.mem); s.flags=self.flags; s.st=self.st
        return s

class Sym:
    def __init__(self,dag,asmfile,regs,leafname=None):
        """regs: register name -> role letter of the guest record it points at (loads become leaves 'role+dword index')."""
        self.d=dag; self.asm,self.nxt,self.prv=load_asm(asmfile); self.regs=regs; self.inline={}
        self.ireg={r:(role,0) for r,role in regs.items()}   # integer registers: int | (role, byte offset)
        self.z=dag.leaf('0.0'); self.st=State(); self.trace=[]; self.adj=0; self.args={}
    def add_inline(self, asmfile, entry, start, args):
        """Inline a small callee (asmfile: its own dump) at its CALLs: it starts executing at `start` (after its prologue)
        with stack arguments `args` (offset from its EBP -> value) and its own stack frame."""
        a,n,p=load_asm(asmfile)
        self.asm.update(a); self.nxt.update(n); self.prv.update(p)
        self.inline[entry]=(start,args)
    # ---- operands
    def ival(self,tok):
        tok=tok.strip()
        if re.match(r'^-?0x[0-9a-f]+$',tok): return int(tok,16)
        return self.ireg[tok.upper()]
    def addr(self,op):
        """('ESP', offset) | (role, offset) | (None, absolute address) for a memory operand."""
        k=re.search(r'\[(.*)\]',op).group(1).replace(' ','').replace('+-','-')
        terms=re.findall(r'[+-]?[^+-]+',k)
        base=None; off=0
        for tm in terms:
            sign=-1 if tm.startswith('-') else 1
            tm=tm.lstrip('+-')
            if '*' in tm:
                r,sc=tm.split('*'); off+=sign*self.ireg[r.upper()]*int(sc,16 if sc.startswith('0x') else 10)
            elif re.match(r'^0x[0-9a-f]+$',tm): off+=sign*int(tm,16)
            elif tm.upper()=='ESP': base=('ESP',0)
            else:
                v=self.ireg[tm.upper()]
                if isinstance(v,int): off+=sign*v
                else:
                    assert base is None; base=v
        if base is None: return (None,off)
        return (base[0],base[1]+off)
    def initial(self,role,off):
        return self.d.leaf('%s%d'%(role,off//4) if off%4==0 else '%s[%#x]'%(role,off))
    def load1(self,S,op,k=0):
        if isx(op): return S.x[int(op[3])][0]
        base,off=self.addr(op); off+=4*k
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
        base,off=self.addr(dst); off+=4*k
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
            elif mn=='MOV' and re.search(r'\[EBP', ops[1]):
                self.ireg[ops[0].upper()]=self.args[int(re.search(r'0x[0-9a-f]+',ops[1]).group(0),16)]
            elif mn=='CMP' and ops[0].startswith('byte ptr'):
                role,off=self.addr(ops[0])
                S.flags=('b',self.d.leaf('Y%s_%x'%(role,off)),self.d.leaf('I%d'%int(ops[1],16)))
            elif mn=='MOV' and ({ops[0].upper(),ops[1].upper()} & {'EBP','ESP'}): pass
            elif mn=='MOV' and not re.search(r'\[',ops[1]) :
                self.ireg[ops[0].upper()]=self.ival(ops[1])
            elif mn=='MOV' and re.search(r'\[ESP',ops[0]) and not re.search(r'\[',ops[1]):   # integer spill to the stack
                b,o=self.addr(ops[0]); S.islot[o+self.adj]=self.ival(ops[1])
            elif mn=='MOV' and re.search(r'\[ESP',ops[1]):
                b,o=self.addr(ops[1]); self.ireg[ops[0].upper()]=S.islot[o+self.adj]
            elif mn=='MOV': pass
            elif mn=='LEA':
                b,o=self.addr(ops[1]); self.ireg[ops[0].upper()]=o if b is None else (b,o)
            elif mn=='XOR' and ops[0]==ops[1]: self.ireg[ops[0].upper()]=0
            elif mn=='INC': self.ireg[ops[0].upper()]+=1
            elif mn=='ADD':
                v=self.ireg[ops[0].upper()]; k=self.ival(ops[1])
                self.ireg[ops[0].upper()]=(v[0],v[1]+k) if isinstance(v,tuple) else v+k
            elif mn=='CMP' and not re.search(r'\[',ops[0]+ops[1]): S.flags=('i',self.ival(ops[0]),self.ival(ops[1]))
            elif mn=='PUSH': self.adj-=4
            elif mn=='POP': self.adj+=4
            elif mn in ('LAHF','TEST'): pass
            elif mn=='FLD': S.st=self.load1(S,ops[0])
            elif mn=='FSTP': self.store1(S,ops[0],d.op('quiet',S.st))
            elif mn=='CALL' and int(ops[0],16) in self.inline:
                start,cargs=self.inline[int(ops[0],16)]
                saved=(S.slot,self.adj,self.args)
                S.slot={}; self.adj=0; self.args=cargs
                self.run(S,start,EXIT)
                S.slot,self.adj,self.args=saved
                self.adj+=4*len(cargs)   # the callee popped its stack arguments
                self._ipdom_stale=True
            elif mn=='CALL':
                name={0x12f0906:'sin',0x12f0a55:'cos'}[int(ops[0],16)]
                S.x[0]=[d.op(name,S.x[0][0])]+S.x[0][1:]
            elif mn in ('JC','JNZ','JZ') and S.flags and S.flags[0]=='i':
                a,b=S.flags[1:3]
                taken={'JC':a<b,'JNZ':a!=b,'JZ':a==b}[mn]
                if taken: nx=int(ops[0],16)
            elif mn=='RET': nx=EXIT
            elif mn in ('JBE','JA','JNC','JC','JP','JNP','JNZ','JZ'):
                tgt=int(ops[0],16)
                kind,a,b=S.flags[:3]
                if kind=='b':   # byte compared with zero: a is the loaded byte, b zero
                    cond={'JNZ':d.op('ne',a,b),'JZ':d.op('eq',a,b)}[mn]
                else:
                    cond={'JBE':d.op('not',d.op('gt',a,b)),'JA':d.op('gt',a,b),'JNC':d.op('ge',a,b),'JC':d.op('not',d.op('ge',a,b)),
                          'JP':d.op('not',d.op('eq',a,b)),'JNP':d.op('eq',a,b)}[mn]
                join=self.ipdom_of(pc)
                adj0=self.adj
                Sf=S.clone(); self.run(Sf,nx,join); adj_f=self.adj; self.adj=adj0
                St=S.clone(); self.run(St,tgt,join)
                if join!=EXIT: self.adj=adj_f
                S2=self.merge(cond,St,Sf)   # cond true => taken arm
                S.__dict__.update(S2.__dict__)
                nx=join
            else: raise NotImplementedError('%x %s %s'%(pc,mn,ops))
            pc=nx
    def ipdom_of(self,pc):
        """Immediate post-dominator of the branch at pc: where both arms are guaranteed to meet (EXIT if they return)."""
        if not hasattr(self,'_ipdom'):
            succ={}
            for a,(mn,ops) in self.asm.items():
                nx=self.nxt.get(a,EXIT)
                if mn=='JMP': succ[a]=[int(ops,16)]
                elif mn.startswith('J'): succ[a]=[int(ops,16),nx]
                elif mn=='RET': succ[a]=[EXIT]
                else: succ[a]=[nx]
            nodes=list(succ)
            pd={n:set(nodes)|{EXIT} for n in nodes}; pd[EXIT]={EXIT}
            changed=True
            while changed:
                changed=False
                for n in nodes:
                    inter=None
                    for s in succ[n]:
                        inter=set(pd[s]) if inter is None else inter&pd[s]
                    new={n}|(inter or set())
                    if new!=pd[n]: pd[n]=new; changed=True
            ip={}
            for n in nodes:
                cand=pd[n]-{n}
                # the immediate one: post-dominated by all the others, i.e. its own pd set equals the candidates
                for c in cand:
                    if pd.get(c,{c})==cand: ip[n]=c; break
            self._ipdom=ip
        return self._ipdom[pc]
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
