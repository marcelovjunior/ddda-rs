//! New-game selection regression. Synthetic stock distinguishes selection
//! from bulk game data; installed records are checked in ddhost's test.
use ddpawn::cmc::Cmc;
use ddstory::prologue::party_from_stock;

#[test]
fn party_selection_preserves_installed_stock_and_rng_boundaries() {
    let mut stock = vec![Cmc::initial(); 20];
    for (i, member) in stock.iter_mut().enumerate() {
        member.rom_id = 0x2000_0000 + i as u32;
        member.param.level = i as u8;
        member.param.equip[0].item_no = i as i16;
    }
    let before = stock.clone();
    let name = |id| Some(format!("ledger-{id}").into_bytes());
    let setup = party_from_stock(&stock, &name, 0).unwrap();
    for (seat, member) in std::iter::once(&setup.hero).chain(&setup.companions).enumerate() {
        let mut expected = stock[10 + seat].clone();
        expected.edit.set_name(&name(896 + seat as u32).unwrap());
        assert_eq!(*member, expected);
    }
    assert_eq!(
        (setup.gold, setup.rift_crystals, setup.discipline),
        (38_000, 785, 1068)
    );
    assert_eq!(party_from_stock(&stock, &name, 4999).unwrap().gold, 42_999);
    assert_eq!(party_from_stock(&stock, &name, 5000).unwrap().gold, 38_000);
    assert_eq!(stock, before, "session templates must stay reusable for N seats");
}

#[test]
fn missing_stock_or_names_is_an_error() {
    let stock = vec![Cmc::initial(); 14];
    assert!(party_from_stock(&stock[..13], &|_| Some(vec![]), 0).is_err());
    assert!(party_from_stock(&stock, &|id| (id != 898).then(Vec::new), 0).is_err());
}
