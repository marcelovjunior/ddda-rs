"""Attribute L131 JSON stack samples to floating-point boundaries (no game run).

Counts are samples, never calls. Inclusive categories overlap. Instruction
mnemonics cannot be recovered from symbol-only stacks; inline saves/restores
remain charged to their containing function. CPU seconds are estimates scaled
by the non-wait sample share, not instruction timings.
"""
import argparse
import collections
import json
import re

WAITS = ("NtWait", "ZwWait", "NtDelayExecution", "NtRemoveIoCompletion",
         "NtUserMsgWaitForMultipleObjects", "NtUserGetMessage",
         "NtAlertThreadByThreadId", "NtWaitForAlertByThreadId",
         "ZwRemoveIoCompletion", "NtSignalAndWaitForSingleObject", "NtUserWaitMessage")
CATEGORIES = {
    "park": "ddnative_host::x87::park",
    "acquire": "ddnative_host::x87::acquire",
    "image": "ddnative_host::x87::image",
    "guest_frstor": "ddnative_base::x87::X87::frstor",
    "guest_fnsave": "ddnative_base::x87::X87::fnsave",
    "emu_frstor": "helper_frstor",
    "ldmxcsr": "::ldmxcsr",
    "stmxcsr": "::stmxcsr",
}


def report(data):
    symbols = data["symbols"]
    for thread in data["threads"]:
        stacks = [[symbols.get(str(a), hex(a)) for a in stack] for stack in thread["stacks"] if stack]
        busy = [s for s in stacks if not any(w in s[0] for w in WAITS)]
        if not busy:
            continue
        counts = collections.Counter()
        paths = collections.Counter()
        for stack in busy:
            for label, needle in CATEGORIES.items():
                if needle in stack[0]:
                    counts[label + "_self"] += 1
                indices = [i for i, s in enumerate(stack) if needle in s]
                if indices:
                    counts[label + "_incl"] += 1
                    i = indices[-1]
                    owners = [m.group(1) for frame in stack[i+1:]
                              if (m := re.search(r"f_([0-9a-fA-F]{8})", frame))]
                    caller = " <- ".join(stack[i+1:i+4])
                    paths[(label, caller + " [guest " + " <- ".join(owners[:3]) + "]")] += 1
        if counts:
            print(f"{thread['name']} tid={thread['tid']} cpu={thread['cpu']:.3f}s busy={len(busy)}/{len(stacks)}")
            for label in CATEGORIES:
                n = counts[label + "_incl"]
                if n:
                    print(f"  {label}: self={counts[label + '_self']} inclusive={n} ({100*n/len(busy):.2f}%; est {thread['cpu']*n/len(busy):.3f}s)")
            for (label, path), n in paths.most_common(15):
                print(f"  {n} {label} <- {path}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("profile")
    args = parser.parse_args()
    with open(args.profile, encoding="utf-8") as source:
        report(json.load(source))
