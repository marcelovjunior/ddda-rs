fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let root = args.get(1).ok_or("usage: inspect <nativePC> <GUI path>")?;
    let path = args.get(2).ok_or("usage: inspect <nativePC> <GUI path>")?;
    let vfs = ddvfs::Vfs::open(root)?;
    let b = vfs.read(path, mtarc::type_hash("rGUI"))?;
    let g = mtgui::Gui::parse(&b)?;
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
    }
    for r in &g.resources {
        println!("{} {} {:?}", r.class, r.id, r.path);
    }
    Ok(())
}
