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
def region(fname, asmfile, start, stop, regs, roles, doc, args=None):
    d=Dag(); S=Sym(d,asmfile,regs); S.args=args or {}
    S.run(S.st,start,stop)
    outs=[S.st.mem.get(('S',4*k),S.initial('S',4*k)) for k in range(16)]
    txt,leaves,consts=emit_kernel('kernel_'+fname,d,outs,doc,{})
    allconsts.update(consts); parts.append(emit_loader('load_'+fname,leaves,'/// Inputs of `kernel_%s`.'%fname,roles)); parts.append(txt)
    print(fname,len(d.nodes),'nodes',len(leaves),'leaves',leaves,flush=True)
ROLES_P=dict(ROLES_STD); ROLES_P['P']='rd(m, pg, b.parent + {off})'
if 'm8' in which:
    region('m8_euler','p_00867420.asm',0x867433,0x8675b1,{'EBX':'T','EDI':'A','ESI':'S'},ROLES_P,'/// Kernel of 0x00867420 (mode 8): its Euler rotation.')
    region('m8_parent','p_00867420.asm',0x867606,0x867b1b,{'EAX':'P','EDI':'J','EBX':'T','ESI':'S'},ROLES_P,'/// Kernel of 0x00867420 (mode 8): translation row from the parent bone and the weights at this+0x84/0x88.')
    region('m8_finish','p_00867420.asm',0x867b1b,0x867b66,{'EBX':'T','ESI':'S'},ROLES_P,'/// Kernel of 0x00867420 (mode 8): adds the translation at this+0x90.')
if 'm12' in which:
    d=Dag(); S=Sym(d,'p_00868000.asm',{'ESI':'J','EDI':'S','EBX':'T'})
    S.add_inline('p_007e4b60.asm',0x7e4b60,0x7e4b69,{8:('B',0)})
    S.run(S.st,0x868010,0x868181)
    S.ireg['EAX']=('P',0)
    S.run(S.st,0x8681aa,EXIT)
    outs=[S.st.mem.get(('S',4*k),S.initial('S',4*k)) for k in range(16)]+[S.st.mem.get(('J',0x10+4*k),S.initial('J',0x10+4*k)) for k in range(16)]
    txt,leaves,consts=emit_kernel('kernel_m12',d,outs,'/// Kernel of 0x00868000 (mode 12): re-bases the joint matrix on the parent (and the joint scale) and updates the output translation. Outputs: 16 floats of the output matrix, then 16 of the joint matrix.',{})
    allconsts.update(consts); parts.append(emit_loader('load_m12',leaves,'/// Inputs of `kernel_m12`.',ROLES_P)); parts.append(txt)
    print('m12',len(d.nodes),'nodes',leaves,flush=True)
ROLES_N=dict(ROLES_P); ROLES_N['N']='rd(m, pg, b.child + {off})'
if 'm13' in which:
    region('m13_euler','p_00869260.asm',0x86926c,0x8693e1,{'EDI':'S','ESI':'J'},ROLES_N,'/// Kernel of 0x00869260 (mode 13): its Euler rotation.',args={0x10:('A',0),8:('T',0)})
    region('m13_ref','p_00869260.asm',0x869458,EXIT,{'EAX':'P','ECX':'N','EDI':'S','ESI':'J','EBX':'T'},ROLES_N,'/// Kernel of 0x00869260 (mode 13): translation row blended between the reference joint and the joint seen through the parent, per axis weights this+0x80..0x88.')
if 'm14' in which:
    region('m14_euler','p_0086a100.asm',0x86a10c,0x86a26a,{'EDI':'S','ESI':'J'},ROLES_N,'/// Kernel of 0x0086A100 (mode 14): its Euler rotation.',args={0x10:('A',0),8:('T',0)})
    d=Dag(); S=Sym(d,'p_0086a100.asm',{'EAX':'N','EDI':'S','ESI':'J','EBX':'T'}); S.args={0xc:('M',0)}
    S.run(S.st,0x86a2aa,0x86a31b)
    S.ireg['EAX']=('P',0)
    S.run(S.st,0x86a368,EXIT)
    outs=[S.st.mem.get(('S',4*k),S.initial('S',4*k)) for k in range(16)]
    txt,leaves,consts=emit_kernel('kernel_m14_ref',d,outs,'/// Kernel of 0x0086A100 (mode 14): translation row from the reference joint seen through the parent.',{})
    allconsts.update(consts); parts.append(emit_loader('load_m14_ref',leaves,'/// Inputs of `kernel_m14_ref`.',ROLES_N)); parts.append(txt)
    print('m14_ref',len(d.nodes),leaves,flush=True)
if 'blend' in which:
    d=Dag(); S=Sym(d,'p_00cfdc80.asm',{'ECX':'D'}); S.args={8:('D',0),0xc:('B',0)}
    S.ireg['EBP']=('W',0)
    S.run(S.st,0xcfdc8c,EXIT)
    outs=[S.st.mem.get(('D',4*k),S.initial('D',4*k)) for k in range(16)]
    roles={'D':'s[{k}]','B':'b.other[{k}]','W':'b.angle'}
    txt,leaves,consts=emit_kernel('kernel_blend',d,outs,'/// Kernel of 0x00CFDC80: blends the matrix toward the identity by the weight (rotation by a slerp-like angle scaling, translation lerp).',{})
    allconsts.update(consts); parts.append(emit_loader('load_blend',leaves,'/// Inputs of `kernel_blend`.',roles)); parts.append(txt)
    print('blend',len(d.nodes),leaves,flush=True)
open('k_modes.rs','w').write(emit_consts(allconsts)+'\n'+'\n'.join(parts))
