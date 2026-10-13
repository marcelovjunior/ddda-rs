"""Kernels of the remaining uCnsEdit providers (modes 8-15)."""
import sys; sys.path.insert(0,'.')
from sym import *
from emit import *
parts=[]; allconsts={}
def whole(fname, asmfile, start, regs, args, roles, doc, xmm=None, ireg=None, slots=None):
    d=Dag(); S=Sym(d,asmfile,regs); S.args=args
    for k,v in (xmm or {}).items(): S.st.x[k]=[d.leaf(v),S.z,S.z,S.z]
    for k,v in (ireg or {}).items(): S.ireg[k]=v
    for k,v in (slots or {}).items(): S.st.slot[k]=d.leaf(v)
    S.run(S.st,start,EXIT)
    outs=[S.st.mem.get(('S',4*k),S.initial('S',4*k)) for k in range(16)]
    txt,leaves,consts=emit_kernel('kernel_'+fname,d,outs,doc,{})
    allconsts.update(consts)
    parts.append(emit_loader('load_'+fname,leaves,'/// Inputs of `kernel_%s`, in order.'%fname,roles))
    parts.append(txt)
    print(fname,len(d.nodes),'nodes',len(leaves),'leaves',leaves,flush=True)

ROLES_STD={'Z':'rd(m, pg, b.this + {off})','A':'b.angles[{k}]','M':'b.mscale[{k} - 24]','J':'rd(m, pg, b.joint + {off})','S':'s[{k}]','T':'rd(m, pg, b.this + {off})'}
which=sys.argv[1:] or ['m9']
if 'm9' in which:
    whole('m9','p_00867b70.asm',0x867b79,{'EDI':'A','ESI':'S'},{8:('Z',0),0xc:('J',0),0x10:('M',0)},ROLES_STD,
          '/// Kernel of 0x00867B70 (modes 9 and 10): Euler rotation, rows scaled by the model scale divided by the joint\'s row lengths.')
if 'm11' in which:
    whole('m11','p_0086d180.asm',0x86d189,{'EDI':'J','ESI':'S'},{8:('Z',0),0xc:('M',0),0x10:('A',0)},ROLES_STD,
          '/// Kernel of 0x0086D180 (mode 11): Euler rotation, rows scaled by (this scale / joint row length) * model scale.')
if 'm15' in which:
    d=Dag(); S=Sym(d,'p_0086aff0.asm',{'EDI':'A','ESI':'S'})
    S.run(S.st,0x86b000,0x86b15d)
    outs=[S.st.mem.get(('S',4*k),S.initial('S',4*k)) for k in range(16)]
    txt,leaves,consts=emit_kernel('kernel_m15_euler',d,outs,'/// Kernel of 0x0086AFF0 (mode 15), its Euler rotation.',{})
    allconsts.update(consts); parts.append(emit_loader('load_m15_euler',leaves,'/// Inputs of `kernel_m15_euler`.',ROLES_STD)); parts.append(txt)
    print('m15_euler',leaves)
open('k_modes.rs','w').write(emit_consts(allconsts)+'\n'+'\n'.join(parts))
