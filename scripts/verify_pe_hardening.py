"""Post-build Windows PE hardening and string-leak verification gate for ferric.exe."""

import struct
import sys


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: verify_pe_hardening.py <path-to-ferric.exe>", file=sys.stderr)
        return 2

    path = sys.argv[1]
    with open(path, "rb") as f:
        data = f.read()

    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe_off : pe_off + 4] != b"PE\x00\x00":
        raise AssertionError("Invalid PE header")

    _, num_sec, _ts, _, _, opt_sz, _ = struct.unpack_from("<HHIIIHH", data, pe_off + 4)
    opt_off = pe_off + 24
    dll_chars = struct.unpack_from("<H", data, opt_off + 70)[0]

    required_dll = {
        0x0020: "HIGH_ENTROPY_VA",
        0x0040: "DYNAMIC_BASE",
        0x0100: "NX_COMPAT",
        0x4000: "GUARD_CF",
    }
    missing = [name for bit, name in required_dll.items() if not (dll_chars & bit)]
    if missing:
        raise AssertionError(
            f"Missing PE DllCharacteristics: {missing} (got 0x{dll_chars:04X})"
        )

    sec_off = opt_off + opt_sz
    sections = []
    for i in range(num_sec):
        o = sec_off + i * 40
        vsz, vrva, rsz, rptr = struct.unpack_from("<IIII", data, o + 8)
        sections.append((vrva, vsz, rptr, rsz))

    def rva_to_off(rva: int):
        for vrva, vsz, rptr, rsz in sections:
            if vrva <= rva < vrva + max(vsz, rsz):
                return rptr + (rva - vrva)
        return None

    dbg_rva, dbg_sz = struct.unpack_from("<II", data, opt_off + 112 + 6 * 8)
    has_cet = False
    has_repro = False
    if dbg_sz:
        off = rva_to_off(dbg_rva)
        if off is None:
            raise AssertionError("Unmapped Debug Directory RVA")
        for i in range(dbg_sz // 28):
            _, _, _, _, dtype, szd, _, ptrraw = struct.unpack_from(
                "<IIHHIIII", data, off + i * 28
            )
            if dtype == 2:
                raise AssertionError("Leaked CodeView RSDS PDB Debug Directory entry")
            if dtype == 13:
                raise AssertionError("Leaked POGO/COFFGRP Debug Directory entry")
            if dtype == 16:
                has_repro = True
            if dtype == 20 and ptrraw and szd >= 4:
                ex_flags = struct.unpack_from("<I", data, ptrraw)[0]
                has_cet = bool(ex_flags & 0x0001)

    if not has_cet:
        raise AssertionError("Missing CETCOMPAT (EX_DLLCHARACTERISTICS_CET_COMPAT)")
    if not has_repro:
        raise AssertionError("Missing BREPRO deterministic build entry")

    low_data = data.lower()
    forbidden_list = [
        b".pdb",
        b"runneradmin",
        b"d:\\a\\",
        b"c:\\users\\",
        b"ferric native rust hi-fi music player",
    ]
    for needle in forbidden_list:
        pos = low_data.find(needle)
        if pos != -1:
            samples = []
            start = 0
            while len(samples) < 5:
                idx = low_data.find(needle, start)
                if idx == -1:
                    break
                ctx = data[max(0, idx - 40) : min(len(data), idx + len(needle) + 80)]
                samples.append(ctx.decode("ascii", errors="replace"))
                start = idx + len(needle)
            raise AssertionError(
                f"Forbidden ASCII string leaked in binary: {needle!r}; samples={samples}"
            )
        u16 = needle.decode("ascii").encode("utf-16le")
        if u16 in low_data:
            idx = low_data.find(u16)
            ctx = data[max(0, idx - 60) : min(len(data), idx + len(u16) + 120)]
            sample = ctx.decode("utf-16le", errors="replace")
            raise AssertionError(
                f"Forbidden UTF-16 string leaked in binary: {needle!r}; sample={sample!r}"
            )

    print(
        f"PE HARDENING OK: DllChars=0x{dll_chars:04X}, GUARD_CF=True, "
        f"CETCOMPAT={has_cet}, BREPRO={has_repro}, Size={len(data)}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
