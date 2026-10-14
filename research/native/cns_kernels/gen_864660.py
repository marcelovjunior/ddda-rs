import sys; sys.path.insert(0,'.')
from sym import *
from emit import *
ASM='p_00864660.asm'
def region(start,stop,regs,xmm=None,slots=None,identity=False):
    d=Dag(); S=Sym(d,ASM,regs)
    for k,v in (xmm or {}).items(): S.st.x[k]=[d.leaf(v),S.z,S.z,S.z]
    for k,v in (slots or {}).items(): S.st.slot[k]=d.leaf(v)
    if identity:
        for k in range(16): S.st.mem[('ESI',4*k)]=d.leaf('C%x'%(0x16295c0+4*k))
    S.run(S.st,start,stop)
    outs=[S.st.mem.get(('ESI',4*k), S.initial('ESI',4*k)) for k in range(16)]
    return d,S,outs
out=[]
for name,start,stop,kw in [
  ('m6_aim',0x864779,0x86527c,dict(regs={'EAX':'N','EDI':'J','EBX':'A','ESI':'S'},slots={0x30:'A0'},identity=True)),
  ('m6_pitch',0x86528e,0x8655f6,dict(regs={'EBX':'A','ESI':'S'},xmm={0:'A1'})),
  ('m6_roll',0x865608,0x86597d,dict(regs={'EBX':'A','ESI':'S'},xmm={0:'A2'})),
]:
    d,S,outs=region(start,stop,**kw)
    txt,leaves,consts=emit_kernel('kernel_'+name,d,outs,'/// generated from the SSE code of 0x00864660 (%s, %#x..%#x)'%(name,start,stop),{})
    print(name,len(d.nodes),'nodes',len(leaves),'leaves',leaves[:40],consts)
    out.append(txt)
open('k_864660.rs','w').write('\n'.join(out))

ROLES={'N':'rd(m, pg, b.child + {off})','J':'rd(m, pg, b.joint + {off})','S':'s[{k}]','A':'b.angle'}
body=[]; allconsts={}
for name,start,stop,kw in [
  ('m6_aim',0x864779,0x86527c,dict(regs={'EAX':'N','EDI':'J','EBX':'A','ESI':'S'},slots={0x30:'A0'},identity=True)),
  ('m6_pitch',0x86528e,0x8655f6,dict(regs={'EBX':'A','ESI':'S'},xmm={0:'A1'})),
  ('m6_roll',0x865608,0x86597d,dict(regs={'EBX':'A','ESI':'S'},xmm={0:'A2'})),
]:
    d,S,outs=region(start,stop,**kw)
    txt,leaves,consts=emit_kernel('kernel_'+name,d,outs,'/// Kernel of 0x00864660 `%s` (%#x..%#x): float code decoded from its SSE instructions, every\n/// operation in the original order. Inputs `l` as listed by `load_%s`; output: the 16 floats of the matrix.'%(name,start,stop,name),{})
    allconsts.update(consts)
    body.append(emit_loader('load_'+name,leaves,'/// Inputs of `kernel_%s`, in order (child/joint record fields, the matrix being built, the angle).'%name,ROLES))
    body.append(txt)
open('k_864660.rs','w').write(emit_consts(allconsts)+'\n'+'\n'.join(body))
