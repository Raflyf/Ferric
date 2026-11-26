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

    if len(data) < 0x100 or data[:2] != b"MZ":
        raise AssertionError("Invalid DOS header")

    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe_off : pe_off + 4] != b"PE\x00\x00":
        raise AssertionError("Invalid PE header")

    _, num_sec, _ts, _, _, opt_sz, _ = struct.unpack_from("<HHIIIHH", data, pe_off + 4)
    opt_off = pe_off + 24

    opt_magic = struct.unpack_from("<H", data, opt_off)[0]
    if opt_magic != 0x020B:
        raise AssertionError(f"Expected PE32+ (0x020B), got 0x{opt_magic:04X}")

    subsystem = struct.unpack_from("<H", data, opt_off + 68)[0]
    if subsystem != 2:
        raise AssertionError(f"Expected Windows GUI subsystem (2), got {subsystem}")

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
        sec_name = data[o : o + 8].rstrip(b"\x00").decode("ascii", errors="replace")
        vsz, vrva, rsz, rptr = struct.unpack_from("<IIII", data, o + 8)
        sec_chars = struct.unpack_from("<I", data, o + 36)[0]
        if (sec_chars & 0xA0000000) == 0xA0000000:
            raise AssertionError(
                f"Forbidden RWX section detected: {sec_name!r} (chars=0x{sec_chars:08X})"
            )
        sections.append((vrva, vsz, rptr, rsz))

    def rva_to_off(rva: int):
        for vrva, vsz, rptr, rsz in sections:
            if vrva <= rva < vrva + max(vsz, rsz):
                return rptr + (rva - vrva)
        return None

    # Verify Debug Directory (index 6): no PDB/POGO leaks, CETCOMPAT + BREPRO present.
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

    # Verify Load Config Directory (index 10): CFG instrumentation table + DependentLoadFlags=0x0800.
    lc_rva, lc_sz = struct.unpack_from("<II", data, opt_off + 112 + 10 * 8)
    if lc_rva == 0 or lc_sz < 0x94:
        raise AssertionError(
            f"Missing or truncated LOAD_CONFIG directory (rva=0x{lc_rva:X}, size={lc_sz})"
        )
    lc_off = rva_to_off(lc_rva)
    if lc_off is None:
        raise AssertionError("Unmapped LOAD_CONFIG RVA")
    lc_struct_sz = struct.unpack_from("<I", data, lc_off)[0]
    if lc_struct_sz < 0x94:
        raise AssertionError(f"LOAD_CONFIG Size too small: 0x{lc_struct_sz:X}")

    dep_load_flags = struct.unpack_from("<H", data, lc_off + 0x4E)[0]
    guard_cf_fn_count = struct.unpack_from("<Q", data, lc_off + 0x88)[0]
    guard_flags = struct.unpack_from("<I", data, lc_off + 0x90)[0]

    if not (dep_load_flags & 0x0800):
        raise AssertionError(
            f"Missing /DEPENDENTLOADFLAG:0x800 (LOAD_LIBRARY_SEARCH_SYSTEM32); got 0x{dep_load_flags:04X}"
        )
    if not (guard_flags & 0x00000100):
        raise AssertionError(
            f"Missing IMAGE_GUARD_CF_INSTRUMENTED in GuardFlags (0x{guard_flags:08X})"
        )
    if guard_cf_fn_count == 0:
        raise AssertionError("GuardCFFunctionCount is 0 (CFG table not populated)")

    # Verify zero leaked host/build paths or PDB references (ASCII and UTF-16LE).
    low_data = data.lower()
    forbidden_list = [
        b".pdb",
        b"runneradmin",
        b"d:\\a\\",
        b"d:\\code\\",
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
        f"PE HARDENING OK: PE32+ GUI, RWX=0, DllChars=0x{dll_chars:04X}, "
        f"GUARD_CF=True (GuardFlags=0x{guard_flags:08X}, CFTable={guard_cf_fn_count}), "
        f"DependentLoadFlags=0x{dep_load_flags:04X}, CETCOMPAT={has_cet}, "
        f"BREPRO={has_repro}, Size={len(data)}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
