fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let root = args.get(1).ok_or("usage: inspect <nativePC> <GUI path>")?;
    let path = args.get(2).ok_or("usage: inspect <nativePC> <GUI path>")?;
    let vfs = ddvfs::Vfs::open(root)?;
    if args.iter().any(|a| a == "--font-header") {
        let bytes = vfs.read(path, mtarc::type_hash("rGUIFont"))?;
        println!(
            "len={} header={:?}",
            bytes.len(),
            bytes[..80]
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>()
        );
        let u32at = |p| u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap()) as usize;
        let p = 44 + u32at(32) * 4;
        let len = u32at(p);
        let p = p + 4;
        println!("texture base {:?}", std::str::from_utf8(&bytes[p..p + len]));
        for r in bytes[p + len + 1..].chunks_exact(16).take(100) {
            println!(
                "glyph {:?}",
                r.chunks_exact(2)
                    .map(|x| u16::from_le_bytes(x.try_into().unwrap()))
                    .collect::<Vec<_>>()
            );
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--messages") {
        let bytes = vfs.read(path, mtarc::type_hash("rGUIMessage"))?;
        let messages = mtgmd::MessageFile::parse(&bytes).map_err(|e| e.to_string())?;
        println!("{messages:#?}");
        return Ok(());
    }
    let b = vfs.read(path, mtarc::type_hash("rGUI"))?;
    let g = mtgui::Gui::parse(&b)?;
    let verbose = args.iter().any(|a| a == "--properties");
    println!(
        "{path}: {}x{}, {} nodes, {} animations",
        g.width,
        g.height,
        g.nodes.len(),
        g.animations.len()
    );
    for (i, depth) in g.preorder()? {
        let n = &g.nodes[i];
        println!(
            "{}[{i}] id={} class={} animation={}",
            "  ".repeat(depth),
            n.id,
            mtgui::node_class_name(n.class_hash)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{:08x}", n.class_hash)),
            g.local_animation(i).map(|a| a.name).unwrap_or("-")
        );
        if verbose {
            for selector in [5, 140 + 0x24, 140 + 0x25, 140 + 0x12, 140 + 0x39] {
                if n.class_hash == 0x7c431e8a {
                    println!("selector {selector}: {:?}", g.control_range(i, selector)?);
                }
            }
            for p in &g.properties[n.properties.clone()] {
                println!(
                    "  {} kind={} word={:?} float={:?}",
                    p.name,
                    p.kind,
                    p.word,
                    p.word.map(f32::from_bits)
                );
            }
        }
    }
    for r in &g.resources {
        println!("{} {} {:?}", r.class, r.id, r.path);
    }
    if verbose {
        for (i, p) in g.properties.iter().enumerate() {
            let vector = if p.kind == 4 {
                Some(
                    g.section(0x100).unwrap().bytes[p.value_offset as usize..p.value_offset as usize + 16]
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            };
            println!(
                "property {i} {} kind={} word={:?} float={:?} vector={vector:?}",
                p.name,
                p.kind,
                p.word,
                p.word.map(f32::from_bits)
            );
        }
        for a in &g.animations {
            println!("animation {} {} {:?}", a.id, a.name, a.words);
            for o in g.animation_objects(a.name)? {
                println!(
                    "  object {} textures {:?} vectors {:?}",
                    o.id, o.texture_keys, o.vector_keys
                );
            }
        }
        for h in [0xa0, 0xa4, 0xa8, 0xb0, 0xf4, 0xdc] {
            let s = g.section(h).unwrap();
            for (i, r) in s.bytes.chunks_exact(s.stride.unwrap()).enumerate() {
                let words: Vec<_> = r
                    .chunks_exact(4)
                    .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
                    .collect();
                println!("table {h:x} [{i}] {words:?}");
            }
        }
    }
    Ok(())
}
