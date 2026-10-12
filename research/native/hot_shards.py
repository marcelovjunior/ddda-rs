"""Deterministic hot/cold ownership for generated lifted packages.

The absent list preserves address shards. An explicit list (even empty)
opts into cold opt-level 0; only its listed functions use optimized shards.
"""
from pathlib import Path

COLD_SHARDS = 32
SHARD_SHIFT = 19
HOT_CAPACITY = 2000


def read_hot_list(path):
    addresses = set()
    for number, line in enumerate(Path(path).read_text(encoding="utf-8").splitlines(), 1):
        value = line.split("#", 1)[0].strip()
        if not value:
            continue
        try:
            address = int(value, 16)
        except ValueError as error:
            raise ValueError(f"{path}:{number}: expected one hexadecimal address") from error
        if not 0 <= address <= 0xFFFFFFFF:
            raise ValueError(f"{path}:{number}: address outside u32")
        addresses.add(address)
    return addresses


class Layout:
    def __init__(self, addresses, hot=None, capacity=HOT_CAPACITY):
        if capacity < 1:
            raise ValueError("hot shard capacity must be positive")
        addresses = set(addresses)
        self.split = hot is not None
        hot = set() if hot is None else set(hot)
        missing = hot - addresses
        if missing:
            raise ValueError(f"hot functions have no lifted body: {sorted(hex(a) for a in missing)}")
        self.owners = {a: f"{(a >> SHARD_SHIFT) % COLD_SHARDS:02}" for a in addresses}
        self.hot_shards = [f"hot_{i:02}" for i in range((len(hot) + capacity - 1) // capacity)]
        for i, address in enumerate(sorted(hot)):
            self.owners[address] = self.hot_shards[i // capacity]
        self.shards = [f"{i:02}" for i in range(COLD_SHARDS)] + self.hot_shards

    def owner(self, address):
        return self.owners[address]

    def opt_level(self, shard):
        return 1 if not self.split or shard in self.hot_shards else 0


def configure_packages(root, layout, write_changed):
    """Write only package metadata; retired hot packages become empty libraries.

    Keeping their tiny manifests avoids deleting directories which may contain
    user files. They are no longer dependencies of ddnative.
    """
    import re
    root = Path(root)
    template = (root / "crates/ddlift/lift_00/Cargo.toml").read_text(encoding="utf-8")
    library = (root / "crates/ddlift/lift_00/src/lib.rs").read_text(encoding="utf-8")
    for name in layout.hot_shards:
        folder = root / f"crates/ddlift/lift_{name}"
        write_changed(folder / "Cargo.toml", template.replace('name = "ddlift-00"', f'name = "ddlift-{name.replace("_", "-")}"'))
        write_changed(folder / "src/lib.rs", library.replace("Fixed address shard", "Sampled hot shard"))
    existing = sorted(p.name.removeprefix("lift_") for p in (root / "crates/ddlift").glob("lift_hot_[0-9][0-9]"))
    for name in set(existing) - set(layout.hot_shards):
        folder = root / f"crates/ddlift/lift_{name}/src"
        write_changed(folder / "lib.rs", "//! Inactive generated hot package.\n")
    begin, end = "# BEGIN generated hot shards", "# END generated hot shards"

    def replace_block(text, block):
        text = re.sub(r"\n?" + re.escape(begin) + r".*?" + re.escape(end) + r"\n?", "", text, flags=re.S)
        return text.rstrip() + ("\n\n" + begin + "\n" + block + end + "\n" if block else "\n")

    native_path = root / "crates/ddnative/Cargo.toml"
    native = native_path.read_text(encoding="utf-8")
    # Explicit tables are valid wherever the generated block is appended.
    dependencies = "".join(f'[dependencies.ddlift-{name.replace("_", "-")}]\npath = "../ddlift/lift_{name}"\n' for name in layout.hot_shards)
    write_changed(native_path, replace_block(native, dependencies))
    cargo_path = root / "Cargo.toml"
    cargo = cargo_path.read_text(encoding="utf-8")
    cargo = re.sub(r'(\[profile\.play\.package\.ddlift-\d\d\]\s*\nopt-level = )\d+',
                   lambda match: match[1] + str(0 if layout.split else 1), cargo)
    if existing and "[profile.play]" not in cargo:
        cargo = cargo.rstrip() + '\n\n[profile.play]\ninherits = "release"\n'
    profiles = "".join(
        f'[profile.dev.package.ddlift-{name.replace("_", "-")}]\nopt-level = 0\ndebug = 1\n'
        f'[profile.release.package.ddlift-{name.replace("_", "-")}]\nopt-level = 1\n'
        f'[profile.play.package.ddlift-{name.replace("_", "-")}]\nopt-level = 1\nincremental = false\n'
        for name in existing)
    write_changed(cargo_path, replace_block(cargo, profiles))
    return sorted(set(existing) - set(layout.hot_shards))


def route_calls(source, caller, layout):
    """Keep direct calls only when both functions are in the same package."""
    import re
    def target(match):
        address = int(match[1], 16)
        if layout.owner(address) == layout.owner(caller):
            return match[0]
        return f"(|m: &mut dyn Mem, cpu: &mut Cpu| x86::call_lifted(m, cpu, 0x{address:08X}))("
    return re.sub(r"super::lifted::f_([0-9a-f]{8})\(", target, source)
