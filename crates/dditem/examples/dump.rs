//! Print item/shop tables joined with English names.
//! usage: dump <nativePC> shops|lvup|curse|mix|items|npcs|msgpaths <filter>
use dditem::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let vfs = ddvfs::Vfs::open(&args[1]).unwrap();
    let read = |class: &str, path: &str| vfs.read(path, mtarc::type_hash(class)).unwrap();
    let names_bytes = read("rGUIMessage", r"id\message\item\itemName_eng");
    let names = mtgmd::MessageFile::parse(&names_bytes).unwrap();
    let name = |i: u32| names.message(i).unwrap_or("?").replace('\n', " ");
    let items = itl::ItemList::parse(&read("rItemList", r"etc\item\itemList")).unwrap();
    let npcs = npc::parse_npc_ledger(&read("rNpcLedgerList", r"etc\item\NpcList")).unwrap();
    match args[2].as_str() {
        "shops" => {
            let h = mtarc::type_hash("rShopList");
            let mut paths: Vec<_> = vfs.paths_of_type(h).collect();
            paths.sort();
            for p in paths {
                let s = shop::ShopList::parse(&vfs.read(p, h).unwrap()).unwrap();
                let no: u16 = p.rsplit('\\').next().unwrap()[1..4].parse().unwrap();
                let who = npcs.iter().find(|n| n.no == no);
                println!(
                    "== {p}  npc {no} {:?} ledger shop_type {:?}  mShopType {} default {}% gsf {:#x}@{}% ok {:?} rates {:?}",
                    who.map(|n| &n.name),
                    who.map(|n| n.shop_type),
                    s.shop_type,
                    s.default_rate,
                    s.discount_gsf,
                    s.discount_gsf_rate,
                    s.discount_ok_item,
                    s.discount_rate
                );
                for j in s.open_and.iter().chain(&s.open_or).filter(|j| j.command != 0) {
                    println!("   shop cond {:?}", j);
                }
                for l in &s.lineup {
                    let it = items.get(l.item).unwrap();
                    let conds: Vec<_> = l
                        .open_and
                        .iter()
                        .map(|j| ("A", j))
                        .chain(l.open_or.iter().map(|j| ("O", j)))
                        .filter(|(_, j)| j.command != 0)
                        .map(|(g, j)| format!("{g}{}:{:?}", j.command, j.params))
                        .collect();
                    println!(
                        "  {:4} {:32} buy {:6} sell {:6} num {:3} rearr {} {}",
                        l.item,
                        name(l.item),
                        it.buy_price(),
                        it.sell_price(),
                        l.num,
                        l.rearrival,
                        conds.join(" ")
                    );
                }
            }
        }
        "lvup" => {
            let t = lvup::EquipLvUp::parse(&read("rEquipLvUp", r"etc\item\equipLvUp")).unwrap();
            for r in &t.rows {
                let steps: Vec<_> = r
                    .steps
                    .iter()
                    .map(|s| match s.material {
                        Some(m) => format!("{}G+{}x{}", s.gold, name(m as u32), s.material_count),
                        None => format!("{}G", s.gold),
                    })
                    .collect();
                println!("{:4} {:28} u{} | {}", r.item, name(r.item as u32), r.unnamed, steps.join(" | "));
            }
        }
        "curse" => {
            let t = tables::parse_item_curse(&read("rItemCurseCnv", r"etc\item\itemCurse")).unwrap();
            for r in &t {
                let e: Vec<_> = r
                    .items
                    .iter()
                    .zip(r.rates)
                    .filter(|(i, _)| **i >= 0)
                    .map(|(i, w)| format!("{}:{}", name(*i as u32), w))
                    .collect();
                println!("lvl {} total {} | {}", r.level, r.total(), e.join(", "));
            }
        }
        "mix" => {
            for r in tables::parse_item_mix(&read("rItemMix", r"etc\item\itemMix")).unwrap() {
                println!("{} + {} -> {} x{}", name(r.base0), name(r.base1), name(r.result), r.count);
            }
        }
        "items" => {
            for (i, r) in items.records.iter().enumerate() {
                let n = name(i as u32);
                if args.len() > 3 && !n.to_lowercase().contains(&args[3].to_lowercase()) {
                    continue;
                }
                println!(
                    "{i:4} {n:32} kind {:2} ct {:2} ck {} lvt {:3} w {:.2} buy {} sell {} flags {:08x}",
                    r.kind(),
                    r.category_type(),
                    r.category_kind(),
                    r.level_up_type(),
                    r.weight(),
                    r.buy_price(),
                    r.sell_price(),
                    r.flags()
                );
            }
        }
        "npcs" => {
            for n in &npcs {
                if n.shop_type != 0 {
                    println!("{} {} stage {} shop_type {}", n.no, n.name, n.stage_no, n.shop_type);
                }
            }
        }
        "msgpaths" => {
            let h = mtarc::type_hash("rGUIMessage");
            let mut paths: Vec<_> = vfs.paths_of_type(h).filter(|p| p.contains(&args[3])).collect();
            paths.sort();
            for p in paths {
                println!("{p}");
            }
        }
        "msg" => {
            let b = read("rGUIMessage", &args[3]);
            let m = mtgmd::MessageFile::parse(&b).unwrap();
            for (i, s) in m.messages.iter().enumerate() {
                let label = m.labels.iter().find(|l| l.message_index as usize == i).map(|l| l.name);
                println!("{i:4} {:?} {}", label, s.replace('\n', "\\n"));
            }
        }
        _ => panic!("unknown mode"),
    }
}
