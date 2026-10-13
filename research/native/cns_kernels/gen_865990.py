import sys; sys.path.insert(0,'.')
from sym import *
from emit import *
ASM='p_00865990.asm'
def mk(regs):
    d=Dag(); S=Sym(d,ASM,regs); return d,S
out_parts=[]; allconsts={}
def finish(name,d,outs,start,stop,roles,leaves_doc=''):
    txt,leaves,consts=emit_kernel('kernel_'+name,d,outs,'/// Kernel of 0x00865990 `%s` (%#x..%#x): float code decoded from its SSE instructions, every\n/// operation in the original order. Inputs `l` as listed by `load_%s`.'%(name,start,stop,name),{})
    allconsts.update(consts)
    out_parts.append(emit_loader('load_'+name,leaves,'/// Inputs of `kernel_%s`, in order.'%name,roles))
    out_parts.append(txt)
    print(name,len(d.nodes),'nodes',len(leaves),'leaves',leaves,len(outs),'outs')

# R1: euler matrix from the three angles at [ebx] (A), then this = [ebp+8] (T)
d,S=mk({'EBX':'A','ESI':'S','EDI':'J'})
S.run(S.st,0x8659a0,0x865a41)
S.regs['EBX']='T'
S.run(S.st,0x865a41,0x865b21)
outs=[S.st.mem[('ESI',4*k)] for k in range(16)]
finish('m7_euler',d,outs,0x8659a0,0x865b21,{'A':'b.angles[{k}]','T':'rd(m, pg, b.this + {off})','J':'rd(m, pg, b.joint + {off})','S':'s[{k}]'})

# R2: parent matrix P (EAX), inverse, J*Q -> stack slots 0x130..
d,S=mk({'EAX':'P','EDI':'J','EBX':'T','ESI':'S'})
S.run(S.st,0x865b62,0x866d45)
print(sorted(hex(k) for k in S.st.slot if k>=0x100))
roles2={'P':'rd(m, pg, b.parent + {off})','J':'rd(m, pg, b.joint + {off})','T':'rd(m, pg, b.this + {off})','S':'s[{k}]','R':'b.r[{k}]'}
outs=[S.st.slot[0x130+4*k] for k in range(16)]
finish('m7_in_parent_space',d,outs,0x865b62,0x866d45,roles2)

# R3: scale the decomposed rotation by (scale-1), build a second rotation, S = R2 * S
d,S=mk({'EAX':'R','EBX':'T','ESI':'S'})
S.run(S.st,0x866d4a,0x867384)
outs=[S.st.mem.get(('ESI',4*k),S.initial('ESI',4*k)) for k in range(16)]
finish('m7_scale_rotation',d,outs,0x866d4a,0x867384,roles2)

# R4: scale row 0 by this+0x88, add the translation
d,S=mk({'EBX':'T','ESI':'S'})
S.st.x[0]=[d.leaf('C15319b8'),S.z,S.z,S.z]
S.run(S.st,0x867384,0x86740e)
outs=[S.st.mem.get(('ESI',4*k),S.initial('ESI',4*k)) for k in range(16)]
finish('m7_scale_translate',d,outs,0x867384,0x86740e,roles2)
open('k_865990.rs','w').write(emit_consts(allconsts)+'\n'+'\n'.join(out_parts))
