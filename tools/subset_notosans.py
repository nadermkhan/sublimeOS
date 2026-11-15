#!/usr/bin/env python3
"""
Standalone zero-dependency TrueType subsetter for SublimeOS.
Subsets C:\\Windows\\Fonts\\NotoSans-{Regular,Bold}.ttf down to printable ASCII,
Latin-1 Supplement, and common UI/typography symbols while preserving authentic
TTF quadratic Bezier outlines, metrics, compound glyphs, and cmap format 4.
"""
import os
import struct

FONT_JOBS = [
    (r"C:\Windows\Fonts\NotoSans-Regular.ttf", os.path.join("libgui", "assets", "NotoSans-Regular.ttf")),
    (r"C:\Windows\Fonts\NotoSans-Bold.ttf", os.path.join("libgui", "assets", "NotoSans-Bold.ttf")),
]

def build_codepoint_set():
    cps = set(range(0x0020, 0x007F))  # Printable ASCII
    cps.update(range(0x00A0, 0x0100))  # Latin-1 Supplement (deg, +/-, x, /, accents, etc.)
    cps.update([
        0x2013, 0x2014, 0x2018, 0x2019, 0x201C, 0x201D,
        0x2022, 0x2026, 0x20AC,
        0x2190, 0x2191, 0x2192, 0x2193,
        0x2212, 0x25A0, 0x25B2, 0x25BC, 0x25C6, 0x25CF,
        0x2713, 0x2715, 0x2717,
    ])
    return sorted(cps)

def parse_tables(data):
    _, num_tables = struct.unpack(">IH", data[:6])
    tables = {}
    for i in range(num_tables):
        tag, _, off, ln = struct.unpack(">4sIII", data[12 + i * 16 : 28 + i * 16])
        tables[tag.decode("latin1")] = (off, ln)
    return tables

def parse_cmap(data, cmap_off):
    _, num_sub = struct.unpack(">HH", data[cmap_off : cmap_off + 4])
    best = None
    for i in range(num_sub):
        plat, enc, rel = struct.unpack(">HHI", data[cmap_off + 4 + i * 8 : cmap_off + 12 + i * 8])
        sub = cmap_off + rel
        fmt = struct.unpack(">H", data[sub : sub + 2])[0]
        prio = {(3, 10, 12): 5, (0, 4, 12): 4, (3, 1, 4): 3, (0, 3, 4): 2}.get((plat, enc, fmt), 0)
        if prio > 0 and (best is None or prio > best[0]):
            best = (prio, fmt, sub)
    assert best is not None, "No usable Unicode cmap subtable found"
    _, fmt, sub = best
    mapping = {}
    if fmt == 12:
        num_groups = struct.unpack(">I", data[sub + 12 : sub + 16])[0]
        for g in range(num_groups):
            s, e, sg = struct.unpack(">III", data[sub + 16 + g * 12 : sub + 28 + g * 12])
            for cp in range(s, min(e, 0xFFFF) + 1):
                mapping[cp] = sg + (cp - s)
    elif fmt == 4:
        seg_x2 = struct.unpack(">H", data[sub + 6 : sub + 8])[0]
        segs = seg_x2 // 2
        end_arr = sub + 14
        start_arr = end_arr + seg_x2 + 2
        delta_arr = start_arr + seg_x2
        range_arr = delta_arr + seg_x2
        for i in range(segs):
            end_c = struct.unpack(">H", data[end_arr + i * 2 : end_arr + i * 2 + 2])[0]
            start_c = struct.unpack(">H", data[start_arr + i * 2 : start_arr + i * 2 + 2])[0]
            delta = struct.unpack(">h", data[delta_arr + i * 2 : delta_arr + i * 2 + 2])[0]
            ro = struct.unpack(">H", data[range_arr + i * 2 : range_arr + i * 2 + 2])[0]
            if start_c == 0xFFFF and end_c == 0xFFFF:
                continue
            for cp in range(start_c, end_c + 1):
                if ro == 0:
                    gid = (cp + delta) & 0xFFFF
                else:
                    addr = range_arr + i * 2 + ro + (cp - start_c) * 2
                    gid = struct.unpack(">H", data[addr : addr + 2])[0]
                    if gid != 0:
                        gid = (gid + delta) & 0xFFFF
                if gid != 0:
                    mapping[cp] = gid
    return mapping

def get_loca_offsets(data, loca_off, loca_fmt, num_glyphs):
    offs = []
    if loca_fmt == 0:
        for i in range(num_glyphs + 1):
            offs.append(struct.unpack(">H", data[loca_off + i * 2 : loca_off + i * 2 + 2])[0] * 2)
    else:
        for i in range(num_glyphs + 1):
            offs.append(struct.unpack(">I", data[loca_off + i * 4 : loca_off + i * 4 + 4])[0])
    return offs

def parse_compound_components(glyph_bytes):
    """Return list of (offset_of_comp_gid, comp_gid, flags) for a compound glyph."""
    if len(glyph_bytes) < 10:
        return []
    num_contours = struct.unpack(">h", glyph_bytes[:2])[0]
    if num_contours >= 0:
        return []
    cur = 10
    comps = []
    while cur + 4 <= len(glyph_bytes):
        flags, comp_gid = struct.unpack(">HH", glyph_bytes[cur : cur + 4])
        comps.append((cur + 2, comp_gid, flags))
        cur += 4
        if flags & 0x0001:  # ARG_1_AND_2_ARE_WORDS
            cur += 4
        else:
            cur += 2
        if flags & 0x0008:  # WE_HAVE_A_SCALE
            cur += 2
        elif flags & 0x0040:  # WE_HAVE_AN_X_AND_Y_SCALE
            cur += 4
        elif flags & 0x0080:  # WE_HAVE_A_TWO_BY_TWO
            cur += 8
        if (flags & 0x0020) == 0:  # MORE_COMPONENTS
            break
    return comps

def build_cmap_format4(cp_to_new_gid):
    items = sorted((cp, gid) for cp, gid in cp_to_new_gid.items() if cp <= 0xFFFF and gid > 0)
    # Build contiguous segments where glyph_id increments by +1 with codepoint
    segs = []
    for cp, gid in items:
        if segs:
            sc, ec, sg = segs[-1]
            if cp == ec + 1 and gid == sg + (ec - sc) + 1:
                segs[-1] = (sc, cp, sg)
                continue
        segs.append((cp, cp, gid))
    segs.append((0xFFFF, 0xFFFF, 0))

    seg_count = len(segs)
    seg_x2 = seg_count * 2
    search_range = 2 * (1 << (seg_count.bit_length() - 1))
    entry_selector = seg_count.bit_length() - 1
    range_shift = seg_x2 - search_range

    end_codes = b"".join(struct.pack(">H", ec) for _, ec, _ in segs)
    start_codes = b"".join(struct.pack(">H", sc) for sc, _, _ in segs)
    deltas = b"".join(
        struct.pack(">h", ((sg - sc + 0x8000) & 0xFFFF) - 0x8000) if sc != 0xFFFF else struct.pack(">h", 1)
        for sc, _, sg in segs
    )
    range_offs = b"\x00\x00" * seg_count

    subtable = (
        struct.pack(">HHHHHHH", 4, 16 + seg_count * 8, 0, seg_x2, search_range, entry_selector, range_shift)
        + end_codes
        + b"\x00\x00"
        + start_codes
        + deltas
        + range_offs
    )
    # Encoded header: format 0 (cmap header v0), 1 encoding record (platform 3, enc 1 -> offset 12)
    return struct.pack(">HH", 0, 1) + struct.pack(">HHI", 3, 1, 12) + subtable

def calc_checksum(tbl):
    padded = tbl + (b"\x00" * ((4 - len(tbl) % 4) % 4))
    total = 0
    for i in range(0, len(padded), 4):
        total = (total + struct.unpack(">I", padded[i : i + 4])[0]) & 0xFFFFFFFF
    return total

def subset_font(src_path, dst_path):
    with open(src_path, "rb") as f:
        data = f.read()

    tables = parse_tables(data)
    head_off, head_len = tables["head"]
    hhea_off, hhea_len = tables["hhea"]
    maxp_off, maxp_len = tables["maxp"]
    hmtx_off, _ = tables["hmtx"]
    loca_off, _ = tables["loca"]
    glyf_off, _ = tables["glyf"]
    cmap_off, _ = tables["cmap"]

    loca_fmt = struct.unpack(">h", data[head_off + 50 : head_off + 52])[0]
    num_glyphs = struct.unpack(">H", data[maxp_off + 4 : maxp_off + 6])[0]
    num_hmetrics = struct.unpack(">H", data[hhea_off + 34 : hhea_off + 36])[0]

    cmap = parse_cmap(data, cmap_off)
    locas = get_loca_offsets(data, loca_off, loca_fmt, num_glyphs)

    target_cps = build_codepoint_set()
    kept_old_gids = [0]
    seen = {0}
    cp_to_old_gid = {}
    for cp in target_cps:
        gid = cmap.get(cp, 0)
        if gid != 0:
            cp_to_old_gid[cp] = gid
            if gid not in seen:
                seen.add(gid)
                kept_old_gids.append(gid)

    # Transitively add compound glyph component targets
    queue = list(kept_old_gids)
    point_matched_warnings = 0
    while queue:
        old_g = queue.pop(0)
        s, e = locas[old_g], locas[old_g + 1]
        if s < e:
            gbytes = data[glyf_off + s : glyf_off + e]
            for _, comp_gid, flags in parse_compound_components(gbytes):
                if (flags & 0x0002) == 0:
                    point_matched_warnings += 1
                if comp_gid not in seen and comp_gid < num_glyphs:
                    seen.add(comp_gid)
                    kept_old_gids.append(comp_gid)
                    queue.append(comp_gid)

    old_to_new = {old_g: new_g for new_g, old_g in enumerate(kept_old_gids)}
    cp_to_new_gid = {cp: old_to_new[og] for cp, og in cp_to_old_gid.items()}

    # Extract hmtx (last advanceWidth applies to >= num_hmetrics)
    last_adv = struct.unpack(">H", data[hmtx_off + (num_hmetrics - 1) * 4 : hmtx_off + (num_hmetrics - 1) * 4 + 2])[0]
    hmtx_out = bytearray()
    glyf_out = bytearray()
    loca_offs = []

    for old_g in kept_old_gids:
        if old_g < num_hmetrics:
            adv, lsb = struct.unpack(">Hh", data[hmtx_off + old_g * 4 : hmtx_off + old_g * 4 + 4])
        else:
            adv = last_adv
            lsb = struct.unpack(">h", data[hmtx_off + num_hmetrics * 4 + (old_g - num_hmetrics) * 2 : hmtx_off + num_hmetrics * 4 + (old_g - num_hmetrics) * 2 + 2])[0]
        hmtx_out.extend(struct.pack(">Hh", adv, lsb))

        loca_offs.append(len(glyf_out))
        s, e = locas[old_g], locas[old_g + 1]
        if s < e:
            gbytes = bytearray(data[glyf_off + s : glyf_off + e])
            for off_gid, comp_gid, _ in parse_compound_components(gbytes):
                struct.pack_into(">H", gbytes, off_gid, old_to_new[comp_gid])
            glyf_out.extend(gbytes)
            while len(glyf_out) % 4 != 0:
                glyf_out.append(0)
    loca_offs.append(len(glyf_out))

    use_long_loca = loca_offs[-1] >= 0x20000
    loca_out = bytearray()
    if use_long_loca:
        for o in loca_offs:
            loca_out.extend(struct.pack(">I", o))
    else:
        for o in loca_offs:
            assert (o & 1) == 0
            loca_out.extend(struct.pack(">H", o // 2))

    # Patch head, hhea, maxp cleanly
    head_out = bytearray(data[head_off : head_off + head_len])
    struct.pack_into(">I", head_out, 8, 0)  # checkSumAdjustment
    struct.pack_into(">h", head_out, 50, 1 if use_long_loca else 0)

    hhea_out = bytearray(data[hhea_off : hhea_off + hhea_len])
    struct.pack_into(">H", hhea_out, 34, len(kept_old_gids))

    maxp_out = bytearray(data[maxp_off : maxp_off + maxp_len])
    struct.pack_into(">H", maxp_out, 4, len(kept_old_gids))

    cmap_out = build_cmap_format4(cp_to_new_gid)

    out_tables = {
        "cmap": cmap_out,
        "glyf": bytes(glyf_out),
        "head": bytes(head_out),
        "hhea": bytes(hhea_out),
        "hmtx": bytes(hmtx_out),
        "loca": bytes(loca_out),
        "maxp": bytes(maxp_out),
    }

    nt = len(out_tables)
    max_p2 = 1 << (nt.bit_length() - 1)
    search_range = max_p2 * 16
    entry_sel = nt.bit_length() - 1
    range_shift = nt * 16 - search_range

    header = struct.pack(">IHHHH", 0x00010000, nt, search_range, entry_sel, range_shift)
    cur_off = 12 + nt * 16
    dir_entries = bytearray()
    body = bytearray()
    for tag in sorted(out_tables.keys()):
        raw = out_tables[tag]
        csum = calc_checksum(raw)
        dir_entries.extend(struct.pack(">4sIII", tag.encode("latin1"), csum, cur_off, len(raw)))
        body.extend(raw)
        pad = (4 - len(raw) % 4) % 4
        body.extend(b"\x00" * pad)
        cur_off += len(raw) + pad

    font_bytes = bytearray(header + dir_entries + body)
    os.makedirs(os.path.dirname(dst_path), exist_ok=True)
    with open(dst_path, "wb") as f:
        f.write(font_bytes)

    print(
        f"{dst_path}: {len(font_bytes)} bytes | glyphs={len(kept_old_gids)} | codepoints={len(cp_to_new_gid)} | "
        f"locaFmt={1 if use_long_loca else 0} | pointMatchedCompounds={point_matched_warnings}"
    )

if __name__ == "__main__":
    for src, dst in FONT_JOBS:
        subset_font(src, dst)
